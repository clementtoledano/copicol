mod commands;
mod db;
mod detect;
mod watcher;

use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::menu::{Menu, MenuBuilder, MenuItem, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

use watcher::WatcherMsg;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub watcher_tx: Mutex<Sender<WatcherMsg>>,
}

fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.emit("window-shown", ());
    }
}

fn toggle_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            show_window(app);
        }
    }
}

/// Enregistre Ctrl+Shift+V pour afficher/cacher la fenêtre.
fn register_shortcut(handle: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use tauri_plugin_global_shortcut::ShortcutState;
    handle.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_shortcuts(["ctrl+shift+v"])?
            .with_handler(|app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    toggle_window(app);
                }
            })
            .build(),
    )?;
    Ok(())
}

/// Libellé de repli pour un favori sans nom (favori hérité) : contenu réduit à
/// une ligne et tronqué, pour rester lisible dans le menu.
fn tray_label_fallback(content: &str) -> String {
    let one_line = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > 48 {
        format!("{}…", one_line.chars().take(48).collect::<String>())
    } else {
        one_line
    }
}

/// Construit le menu du tray : Afficher, sous-menu « Favoris » (favoris
/// regroupés par dossier, chaque favori portant l'id « fav:<id> »), À propos,
/// Quitter. Les favoris et dossiers sont lus en base — donc présents dès le
/// démarrage, avant même l'ouverture de la fenêtre.
fn tray_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let (favorites, groups) = {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
        (
            db::list_favorites(&conn, None).unwrap_or_default(),
            db::list_groups(&conn).unwrap_or_default(),
        )
    };

    let mut fav = SubmenuBuilder::new(app, "Favoris");
    if favorites.is_empty() {
        let none = MenuItem::with_id(app, "fav_none", "(aucun favori)", false, None::<&str>)?;
        fav = fav.item(&none);
    } else {
        let fav_entry = |f: &db::Item| -> String {
            f.label
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| tray_label_fallback(&f.content))
        };
        let mut first_section = true;
        // Une section par dossier (dans l'ordre des dossiers), les favoris sans
        // dossier en dernier. Un en-tête désactivé introduit chaque section dès
        // qu'il existe au moins un dossier.
        for g in &groups {
            let members: Vec<&db::Item> =
                favorites.iter().filter(|f| f.group_id == Some(g.id)).collect();
            if members.is_empty() {
                continue;
            }
            if !first_section {
                fav = fav.separator();
            }
            first_section = false;
            let header =
                MenuItem::with_id(app, format!("grp:{}", g.id), &g.name, false, None::<&str>)?;
            fav = fav.item(&header);
            for f in members {
                fav = fav.text(format!("fav:{}", f.id), fav_entry(f));
            }
        }
        let ungrouped: Vec<&db::Item> = favorites.iter().filter(|f| f.group_id.is_none()).collect();
        if !ungrouped.is_empty() {
            if !first_section {
                fav = fav.separator();
                let header =
                    MenuItem::with_id(app, "grp_none", "Sans dossier", false, None::<&str>)?;
                fav = fav.item(&header);
            }
            for f in ungrouped {
                fav = fav.text(format!("fav:{}", f.id), fav_entry(f));
            }
        }
    }
    let fav = fav.build()?;

    MenuBuilder::new(app)
        .text("show", "Afficher")
        .item(&fav)
        .separator()
        .text("about", "À propos de copicol")
        .text("quit", "Quitter")
        .build()
}

/// Reconstruit le menu du tray sur le thread principal (requis pour les
/// opérations de menu). Appelé après tout changement de favoris. Sans effet si
/// le tray n'a pas pu être créé.
pub(crate) fn refresh_tray_menu(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Ok(menu) = tray_menu(&handle) {
            if let Some(tray) = handle.tray_by_id("main") {
                let _ = tray.set_menu(Some(menu));
            }
        }
    });
}

/// Aligne l'état système (registre / login items) sur la préférence stockée en
/// base (activée par défaut). Appelé au démarrage et après un import de
/// favoris (qui peut avoir changé la préférence). Non fatal : un environnement
/// sans support d'autostart ne doit pas empêcher l'app de fonctionner.
pub(crate) fn sync_autostart(app: &AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    let state = app.state::<AppState>();
    let enabled = {
        let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
        db::autostart_enabled(&conn).unwrap_or(true)
    };
    let autolaunch = app.autolaunch();
    let already = autolaunch.is_enabled().unwrap_or(!enabled);
    if already != enabled {
        let result = if enabled { autolaunch.enable() } else { autolaunch.disable() };
        if let Err(e) = result {
            log::warn!("copicol: démarrage automatique indisponible: {e}");
        }
    }
}

/// Depuis le menu du tray : place le contenu du favori dans le presse-papiers
/// (via le thread watcher, pour ne pas le réinsérer dans l'historique) et
/// incrémente son compteur d'usage. L'utilisateur colle ensuite (Ctrl+V).
fn copy_favorite_to_clipboard(app: &AppHandle, id: i64) {
    let state = app.state::<AppState>();
    let content = {
        let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
        match db::get_content(&conn, id) {
            Ok(Some(c)) => c,
            _ => return,
        }
    };

    let (ack_tx, ack_rx) = mpsc::channel();
    if state
        .watcher_tx
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .send(WatcherMsg::Write {
            text: content,
            ack: ack_tx,
        })
        .is_ok()
    {
        let _ = ack_rx.recv_timeout(std::time::Duration::from_secs(1));
    }

    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    let _ = db::mark_used(&conn, id);
}

/// Icône de la zone de notification : l'app vit dans le tray.
fn build_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let menu = tray_menu(app.handle())?;
    TrayIconBuilder::with_id("main")
        .icon(
            app.default_window_icon()
                .ok_or("icône par défaut manquante")?
                .clone(),
        )
        .tooltip("copicol — Ctrl+Shift+V")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_window(app),
            "about" => {
                show_window(app);
                let _ = app.emit("show-about", ());
            }
            "quit" => app.exit(0),
            other => {
                if let Some(fid) = other.strip_prefix("fav:").and_then(|s| s.parse::<i64>().ok()) {
                    copy_favorite_to_clipboard(app, fid);
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        // Journalisation fichier (AppData/logs) + stdout en dev — indispensable
        // pour diagnostiquer un problème une fois l'app installée (pas de console).
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        // Relancer l'exe affiche la fenêtre existante au lieu d'un doublon
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // Mises à jour automatiques (GitHub Releases, cf. .github/workflows/release.yml)
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // Ouverture de liens externes (dépôt GitHub depuis « À propos »)
        .plugin(tauri_plugin_opener::init())
        // Boîtes de dialogue fichier (import/export des favoris)
        .plugin(tauri_plugin_dialog::init())
        // Mémorise taille et position (pas la visibilité : démarrage caché)
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION,
                )
                .build(),
        )
        .setup(|app| {
            // Base SQLite dans le dossier de données de l'application
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let conn = db::open(&data_dir.join("copicol.db"))?;

            let (tx, rx) = mpsc::channel();
            app.manage(AppState {
                db: Mutex::new(conn),
                watcher_tx: Mutex::new(tx),
            });
            watcher::start(app.handle().clone(), rx);

            // Raccourci global et tray : non fatals — sans eux l'app reste
            // utilisable (ex. environnements Wayland ou sans zone de notification)
            if let Err(e) = register_shortcut(app.handle()) {
                log::warn!("copicol: raccourci global indisponible: {e}");
            }
            if let Err(e) = build_tray(app) {
                log::warn!("copicol: icône de notification indisponible: {e}");
            }

            // Démarrage automatique avec la session, selon la préférence de
            // l'utilisateur (activée par défaut) — non fatal
            sync_autostart(app.handle());

            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Fermer = cacher, l'application reste dans le tray
            WindowEvent::CloseRequested { api, .. } => {
                let _ = window.hide();
                api.prevent_close();
            }
            // Comportement lanceur : la fenêtre disparaît quand elle perd le focus.
            // Le redimensionnement natif (startResizeDragging) déclenche une perte de
            // focus transitoire du WebView : on temporise et revérifie avant de cacher,
            // pour ne pas fermer la fenêtre quand l'utilisateur redimensionne.
            WindowEvent::Focused(false) => {
                let window = window.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if !window.is_focused().unwrap_or(false) {
                        let _ = window.hide();
                    }
                });
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_items,
            commands::list_kinds,
            commands::list_favorites,
            commands::count_favorites,
            commands::copy_item,
            commands::pin_item,
            commands::unpin_item,
            commands::set_item_kind,
            commands::update_item_content,
            commands::delete_item,
            commands::hide_window,
            commands::list_groups,
            commands::create_group,
            commands::rename_group,
            commands::delete_group,
            commands::set_item_group,
            commands::clear_history,
            commands::quit_app,
            commands::export_favorites,
            commands::import_favorites,
            commands::get_autostart_enabled,
            commands::set_autostart_enabled,
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de copicol");
}
