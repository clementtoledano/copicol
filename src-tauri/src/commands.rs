use std::sync::mpsc;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

use crate::watcher::WatcherMsg;
use crate::{db, AppState};

#[tauri::command]
pub fn list_items(
    state: State<AppState>,
    search: Option<String>,
    kind: Option<String>,
) -> Result<Vec<db::Item>, String> {
    let conn = state.db.lock().unwrap();
    db::list_items(&conn, search.as_deref(), kind.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_kinds(state: State<AppState>) -> Result<Vec<db::KindCount>, String> {
    let conn = state.db.lock().unwrap();
    db::list_kinds(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_favorites(
    state: State<AppState>,
    search: Option<String>,
) -> Result<Vec<db::Item>, String> {
    let conn = state.db.lock().unwrap();
    db::list_favorites(&conn, search.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn count_favorites(state: State<AppState>) -> Result<i64, String> {
    let conn = state.db.lock().unwrap();
    db::count_favorites(&conn).map_err(|e| e.to_string())
}

/// Épingle un élément avec son nom descriptif (obligatoire côté UI).
/// Sert aussi à renommer un favori existant. Reconstruit le menu du tray.
#[tauri::command]
pub fn pin_item(app: AppHandle, id: i64, label: String) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        db::pin_item(&conn, id, &label).map_err(|e| e.to_string())?;
    }
    crate::refresh_tray_menu(&app);
    Ok(())
}

/// Retire un élément des favoris (son nom est conservé en base).
#[tauri::command]
pub fn unpin_item(app: AppHandle, id: i64) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        db::unpin_item(&conn, id).map_err(|e| e.to_string())?;
    }
    crate::refresh_tray_menu(&app);
    Ok(())
}

#[tauri::command]
pub fn delete_item(app: AppHandle, id: i64) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        db::delete_item(&conn, id).map_err(|e| e.to_string())?;
    }
    crate::refresh_tray_menu(&app);
    Ok(())
}

#[tauri::command]
pub fn hide_window(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

/// Colle un élément : écrit son contenu dans le presse-papiers, cache la
/// fenêtre pour rendre le focus à l'application précédente, puis simule
/// Ctrl+V (Cmd+V sur macOS). Si la simulation échoue, le contenu reste
/// dans le presse-papiers et l'utilisateur peut coller manuellement.
#[tauri::command]
pub async fn paste_item(app: AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();

    let content = {
        let conn = state.db.lock().unwrap();
        db::get_content(&conn, id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "élément introuvable".to_string())?
    };

    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }

    // L'écriture passe par le thread watcher (unique propriétaire du
    // presse-papiers), qui met aussi à jour son hash pour ne pas
    // réinsérer ce contenu dans l'historique.
    let (ack_tx, ack_rx) = mpsc::channel();
    state
        .watcher_tx
        .lock()
        .unwrap()
        .send(WatcherMsg::Write {
            text: content,
            ack: ack_tx,
        })
        .map_err(|e| e.to_string())?;
    let _ = ack_rx.recv_timeout(Duration::from_secs(1));

    {
        let conn = state.db.lock().unwrap();
        let _ = db::mark_used(&conn, id);
    }

    // Attend que le focus ait réellement quitté copicol avant de simuler
    // Ctrl+V : sinon le collage frappe notre propre webview et déclenche
    // le dialogue de permission presse-papiers de WebView2 sous Windows.
    if let Some(w) = app.get_webview_window("main") {
        for _ in 0..40 {
            if !w.is_focused().unwrap_or(false) {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    // Petite marge pour que l'application cible soit prête à recevoir la frappe
    std::thread::sleep(Duration::from_millis(100));
    simulate_paste();

    Ok(())
}

fn simulate_paste() {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};

    let Ok(mut enigo) = Enigo::new(&Settings::default()) else {
        return; // fallback : le contenu est déjà dans le presse-papiers
    };

    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    let _ = enigo.key(modifier, Direction::Press);
    let _ = enigo.key(Key::Unicode('v'), Direction::Click);
    let _ = enigo.key(modifier, Direction::Release);
}
