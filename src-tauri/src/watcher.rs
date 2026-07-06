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

/// Taille maximale d'un contenu enregistré dans l'historique (2 Mo). Au-delà,
/// la copie est ignorée : évite qu'un gros copier/coller (fichier, log entier)
/// ne sature la base et le rendu de la liste.
const MAX_CONTENT_LEN: usize = 2 * 1024 * 1024;

/// Beaucoup de gestionnaires de mots de passe (KeePass, Bitwarden, 1Password…)
/// posent ce format Windows standard sur le presse-papiers pour signaler aux
/// gestionnaires d'historique de ne pas capturer la copie. On le respecte pour
/// ne jamais stocker un mot de passe en clair dans la base.
#[cfg(windows)]
fn clipboard_marked_sensitive() -> bool {
    let Some(format) = clipboard_win::register_format("ExcludeClipboardContentFromMonitorProcessing")
    else {
        return false;
    };
    match clipboard_win::Clipboard::new_attempts(3) {
        Ok(_clip) => clipboard_win::is_format_avail(format.get()),
        Err(_) => false,
    }
}

#[cfg(not(windows))]
fn clipboard_marked_sensitive() -> bool {
    false
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
                log::error!("copicol: impossible d'accéder au presse-papiers: {e}");
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
                    if clipboard_marked_sensitive() {
                        continue;
                    }
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

                    if text.len() > MAX_CONTENT_LEN {
                        log::warn!("copicol: copie ignorée, contenu trop volumineux ({} octets)", text.len());
                        continue;
                    }

                    let state = app.state::<AppState>();
                    let inserted = {
                        let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
                        db::insert_or_touch(&conn, &text, now_unix())
                    };
                    match inserted {
                        Ok(()) => {
                            let _ = app.emit("clipboard-changed", ());
                        }
                        Err(e) => log::error!("copicol: erreur d'insertion: {e}"),
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}
