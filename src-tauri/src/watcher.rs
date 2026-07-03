use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

use crate::{db, AppState};

/// Messages envoyés au thread propriétaire du presse-papiers.
pub enum WatcherMsg {
    /// Écrire ce texte dans le presse-papiers (collage depuis copicol).
    /// `ack` est notifié une fois l'écriture terminée.
    Write { text: String, ack: Sender<()> },
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Démarre le thread de surveillance : il est l'unique propriétaire de
/// l'objet presse-papiers. Toutes les ~400 ms il lit le contenu ; entre
/// deux ticks il traite les demandes d'écriture venant des commandes.
/// Une écriture faite par copicol met à jour `last_hash`, donc elle n'est
/// jamais réinsérée dans l'historique par le poll suivant.
pub fn start(app: AppHandle, rx: Receiver<WatcherMsg>) {
    std::thread::spawn(move || {
        let mut clipboard = match arboard::Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("copicol: impossible d'accéder au presse-papiers: {e}");
                return;
            }
        };
        let mut last_hash: Option<String> = None;

        loop {
            match rx.recv_timeout(Duration::from_millis(400)) {
                Ok(WatcherMsg::Write { text, ack }) => {
                    let hash = db::hash_content(&text);
                    if clipboard.set_text(text).is_ok() {
                        last_hash = Some(hash);
                    }
                    let _ = ack.send(());
                }
                Err(RecvTimeoutError::Timeout) => {
                    let Ok(text) = clipboard.get_text() else {
                        continue;
                    };
                    if text.trim().is_empty() {
                        continue;
                    }
                    let hash = db::hash_content(&text);
                    if last_hash.as_deref() == Some(hash.as_str()) {
                        continue;
                    }
                    last_hash = Some(hash);

                    let state = app.state::<AppState>();
                    let inserted = {
                        let conn = state.db.lock().unwrap();
                        db::insert_or_touch(&conn, &text, now_unix())
                    };
                    match inserted {
                        Ok(()) => {
                            let _ = app.emit("clipboard-changed", ());
                        }
                        Err(e) => eprintln!("copicol: erreur d'insertion: {e}"),
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}
