use std::sync::mpsc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::watcher::WatcherMsg;
use crate::{db, AppState};

#[tauri::command]
pub fn list_items(
    state: State<AppState>,
    search: Option<String>,
    kind: Option<String>,
) -> Result<Vec<db::Item>, String> {
    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    db::list_items(&conn, search.as_deref(), kind.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_kinds(state: State<AppState>) -> Result<Vec<db::KindCount>, String> {
    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    db::list_kinds(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_favorites(
    state: State<AppState>,
    search: Option<String>,
) -> Result<Vec<db::Item>, String> {
    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    db::list_favorites(&conn, search.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn count_favorites(state: State<AppState>) -> Result<i64, String> {
    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    db::count_favorites(&conn).map_err(|e| e.to_string())
}

/// Épingle un élément avec son nom descriptif (obligatoire côté UI).
/// Sert aussi à renommer un favori existant. Reconstruit le menu du tray.
#[tauri::command]
pub fn pin_item(app: AppHandle, id: i64, label: String) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
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
        let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
        db::unpin_item(&conn, id).map_err(|e| e.to_string())?;
    }
    crate::refresh_tray_menu(&app);
    Ok(())
}

/// Reclasse un favori dans une autre catégorie, indépendamment de la
/// détection automatique.
#[tauri::command]
pub fn set_item_kind(state: State<AppState>, id: i64, kind: String) -> Result<(), String> {
    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    db::set_kind(&conn, id, &kind).map_err(|e| e.to_string())
}

/// Modifie le contenu d'un favori (nom et catégorie conservés).
#[tauri::command]
pub fn update_item_content(state: State<AppState>, id: i64, content: String) -> Result<(), String> {
    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    db::update_content(&conn, id, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_item(app: AppHandle, id: i64) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
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

// ── Dossiers de favoris ─────────────────────────────────────────────

/// Liste les dossiers de favoris, classés par nom.
#[tauri::command]
pub fn list_groups(state: State<AppState>) -> Result<Vec<db::Group>, String> {
    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    db::list_groups(&conn).map_err(|e| e.to_string())
}

/// Crée un dossier (ou renvoie l'existant si le nom est déjà pris) et retourne
/// son identifiant.
#[tauri::command]
pub fn create_group(state: State<AppState>, name: String) -> Result<i64, String> {
    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    db::create_group(&conn, name.trim()).map_err(|e| e.to_string())
}

/// Renomme un dossier existant.
#[tauri::command]
pub fn rename_group(state: State<AppState>, id: i64, name: String) -> Result<(), String> {
    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    db::rename_group(&conn, id, name.trim()).map_err(|e| e.to_string())
}

/// Supprime un dossier ; ses favoris sont conservés mais repassent « sans dossier ».
#[tauri::command]
pub fn delete_group(app: AppHandle, id: i64) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
        db::delete_group(&conn, id).map_err(|e| e.to_string())?;
    }
    crate::refresh_tray_menu(&app);
    Ok(())
}

/// Affecte un favori à un dossier (ou l'en retire si `group_id` est `null`).
#[tauri::command]
pub fn set_item_group(app: AppHandle, id: i64, group_id: Option<i64>) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
        db::set_item_group(&conn, id, group_id).map_err(|e| e.to_string())?;
    }
    crate::refresh_tray_menu(&app);
    Ok(())
}

/// Vide l'historique (les favoris épinglés sont conservés).
#[tauri::command]
pub fn clear_history(app: AppHandle) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
        db::clear_history(&conn).map_err(|e| e.to_string())?;
    }
    let _ = app.emit("clipboard-changed", ());
    Ok(())
}

/// Quitte complètement l'application (depuis la barre de menu).
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

// ── Import / export des favoris ─────────────────────────────────────

/// Résumé d'un import, remonté à l'interface.
#[derive(serde::Serialize)]
pub struct ImportSummary {
    pub imported: usize,
    pub skipped: usize,
}

/// Écrit tous les favoris (et leurs dossiers) au format JSON dans `path`.
/// Renvoie le nombre de favoris exportés.
#[tauri::command]
pub fn export_favorites(app: AppHandle, path: String) -> Result<usize, String> {
    let data = {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
        db::export_favorites(&conn).map_err(|e| e.to_string())?
    };
    let json = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(data.favorites.len())
}

/// Lit un fichier JSON de favoris et le fusionne (doublons ignorés). Renvoie le
/// nombre d'éléments importés et ignorés.
#[tauri::command]
pub fn import_favorites(app: AppHandle, path: String) -> Result<ImportSummary, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let file: db::FavoritesFile = serde_json::from_str(&text)
        .map_err(|_| "Fichier invalide : un export JSON de copicol est attendu.".to_string())?;

    let (imported, skipped) = {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
        db::import_favorites(&conn, &file, crate::watcher::now_unix()).map_err(|e| e.to_string())?
    };
    crate::refresh_tray_menu(&app);
    let _ = app.emit("clipboard-changed", ());
    Ok(ImportSummary { imported, skipped })
}

/// Copie un élément : écrit son contenu dans le presse-papiers et cache la
/// fenêtre. Le collage reste à la charge de l'utilisateur (Ctrl+V dans
/// l'application de son choix) : copicol ne simule aucune frappe.
#[tauri::command]
pub fn copy_item(app: AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();

    let content = {
        let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
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
        .unwrap_or_else(|e| e.into_inner())
        .send(WatcherMsg::Write {
            text: content,
            ack: ack_tx,
        })
        .map_err(|e| e.to_string())?;
    let _ = ack_rx.recv_timeout(Duration::from_secs(1));

    {
        let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
        let _ = db::mark_used(&conn, id);
    }

    Ok(())
}
