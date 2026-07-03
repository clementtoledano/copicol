mod commands;
mod db;
mod detect;
mod watcher;

use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::menu::{Menu, MenuItem};
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

/// Icône de la zone de notification : l'app vit dans le tray.
fn build_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "Afficher", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quitter", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    TrayIconBuilder::new()
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
            "quit" => app.exit(0),
            _ => {}
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
        // Relancer l'exe affiche la fenêtre existante au lieu d'un doublon
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
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
                eprintln!("copicol: raccourci global indisponible: {e}");
            }
            if let Err(e) = build_tray(app) {
                eprintln!("copicol: icône de notification indisponible: {e}");
            }

            // Démarrage automatique avec la session (non fatal)
            {
                use tauri_plugin_autostart::ManagerExt;
                let autolaunch = app.autolaunch();
                if !autolaunch.is_enabled().unwrap_or(false) {
                    if let Err(e) = autolaunch.enable() {
                        eprintln!("copicol: démarrage automatique indisponible: {e}");
                    }
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Fermer = cacher, l'application reste dans le tray
            WindowEvent::CloseRequested { api, .. } => {
                let _ = window.hide();
                api.prevent_close();
            }
            // Comportement lanceur : la fenêtre disparaît quand elle perd le focus
            WindowEvent::Focused(false) => {
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_items,
            commands::list_kinds,
            commands::paste_item,
            commands::toggle_pin,
            commands::delete_item,
            commands::hide_window,
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de copicol");
}
