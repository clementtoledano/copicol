use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

use crate::{db, imaging, AppState};

/// Messages envoyés au thread propriétaire du presse-papiers.
pub enum WatcherMsg {
    /// Écrire ce texte dans le presse-papiers (collage depuis copicol).
    /// `ack` est notifié une fois l'écriture terminée.
    Write { text: String, ack: Sender<()> },
    /// Écrire cette image (bitmap). `ack` reçoit le hash de l'image **relue**
    /// dans le presse-papiers après écriture (`None` si l'écriture a échoué) :
    /// Windows convertit l'image, les octets relus peuvent différer légèrement
    /// de ceux écrits, et c'est ce hash que le poll suivant retrouvera.
    WriteImage { image: imaging::Rgba, ack: Sender<Option<String>> },
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

/// Taille maximale d'un fichier image copié depuis l'Explorateur (50 Mo).
const MAX_IMAGE_FILE_LEN: u64 = 50 * 1024 * 1024;

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

/// Numéro de séquence du presse-papiers Windows : change à chaque copie. Évite
/// de décoder à chaque poll (400 ms) une grosse capture d'écran déjà traitée.
#[cfg(windows)]
fn clipboard_seq() -> Option<u32> {
    clipboard_win::seq_num().map(|n| n.get())
}

#[cfg(not(windows))]
fn clipboard_seq() -> Option<u32> {
    None
}

/// Convertit l'image d'arboard en [`imaging::Rgba`].
fn to_rgba(data: arboard::ImageData<'_>) -> imaging::Rgba {
    imaging::Rgba {
        width: data.width as u32,
        height: data.height as u32,
        pixels: data.bytes.into_owned(),
    }
}

/// Enregistre une image dans l'historique et prévient le frontend.
fn store_image(app: &AppHandle, rgba: &imaging::Rgba, hash: String) {
    let new = match db::new_image(rgba, hash) {
        Ok(n) => n,
        Err(e) => {
            log::warn!("copicol: image ignorée: {e}");
            return;
        }
    };
    let state = app.state::<AppState>();
    let inserted = {
        let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
        db::insert_or_touch_image(&conn, &new, now_unix())
    };
    match inserted {
        Ok(()) => {
            let _ = app.emit("clipboard-changed", ());
        }
        Err(e) => log::error!("copicol: erreur d'insertion de l'image: {e}"),
    }
}

/// Clé de dédoublonnage d'un fichier copié : chemin + date de modification,
/// pour ne pas relire le fichier à chaque poll.
fn file_key(path: &std::path::Path) -> Option<String> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let secs = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(format!("file:{}:{secs}", path.display()))
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
        // Dernier numéro de séquence dont l'image (ou le fichier) a été traité.
        let mut last_image_seq: Option<u32> = None;

        loop {
            match rx.recv_timeout(Duration::from_millis(400)) {
                Ok(WatcherMsg::Write { text, ack }) => {
                    let hash = db::hash_content(&text);
                    if clipboard.set_text(text).is_ok() {
                        last_hash = Some(hash);
                    }
                    let _ = ack.send(());
                }
                Ok(WatcherMsg::WriteImage { image, ack }) => {
                    let written = clipboard
                        .set_image(arboard::ImageData {
                            width: image.width as usize,
                            height: image.height as usize,
                            bytes: std::borrow::Cow::Borrowed(&image.pixels),
                        })
                        .is_ok();
                    let hash = if written {
                        let reread = clipboard.get_image().ok().map(|d| imaging::image_hash(&to_rgba(d)));
                        let hash = reread.unwrap_or_else(|| imaging::image_hash(&image));
                        last_hash = Some(hash.clone());
                        Some(hash)
                    } else {
                        None
                    };
                    let _ = ack.send(hash);
                }
                Err(RecvTimeoutError::Timeout) => {
                    if clipboard_marked_sensitive() {
                        continue;
                    }
                    let text = clipboard.get_text().ok();
                    let has_text = text.as_deref().map(|t| !t.trim().is_empty()).unwrap_or(false);
                    // Le HTML n'est lu que s'il y a du texte : sert à reconnaître
                    // « Copier l'image » d'un navigateur (texte = URL, HTML = <img>).
                    let html = if has_text { clipboard.get().html().ok() } else { None };
                    let browser_image = html.as_deref().map(imaging::html_is_single_img).unwrap_or(false);

                    let seq = clipboard_seq();
                    let image_already_done = seq.is_some() && seq == last_image_seq;
                    let image = if (!has_text || browser_image) && !image_already_done {
                        clipboard.get_image().ok()
                    } else {
                        None
                    };
                    let has_image = image.is_some();

                    match imaging::capture_source(text.as_deref(), html.as_deref(), has_image) {
                        imaging::CaptureSource::Text => {
                            let text = text.unwrap_or_default();
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
                        imaging::CaptureSource::Image => {
                            last_image_seq = seq;
                            let data = image.expect("has_image est vrai");
                            if !imaging::within_limits(data.width as u32, data.height as u32) {
                                log::warn!("copicol: image ignorée, trop grande ({}×{})", data.width, data.height);
                                continue;
                            }
                            let rgba = to_rgba(data);
                            let hash = imaging::image_hash(&rgba);
                            if last_hash.as_deref() == Some(hash.as_str()) {
                                continue;
                            }
                            last_hash = Some(hash.clone());
                            store_image(&app, &rgba, hash);
                        }
                        imaging::CaptureSource::Nothing => {
                            if image_already_done {
                                continue;
                            }
                            last_image_seq = seq;
                            // Ni texte ni bitmap : un unique fichier image copié
                            // depuis l'Explorateur ?
                            let Ok(files) = clipboard.get().file_list() else {
                                continue;
                            };
                            let [path] = files.as_slice() else {
                                continue;
                            };
                            if !imaging::is_image_path(path) {
                                continue;
                            }
                            let Some(key) = file_key(path) else {
                                continue;
                            };
                            if last_hash.as_deref() == Some(key.as_str()) {
                                continue;
                            }
                            last_hash = Some(key);
                            let too_big = std::fs::metadata(path).map(|m| m.len() > MAX_IMAGE_FILE_LEN).unwrap_or(true);
                            if too_big {
                                log::warn!("copicol: fichier image ignoré (trop volumineux ou illisible)");
                                continue;
                            }
                            let rgba = match std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| imaging::decode(&b)) {
                                Ok(r) => r,
                                Err(e) => {
                                    log::warn!("copicol: fichier image illisible: {e}");
                                    continue;
                                }
                            };
                            if !imaging::within_limits(rgba.width, rgba.height) {
                                log::warn!("copicol: image ignorée, trop grande ({}×{})", rgba.width, rgba.height);
                                continue;
                            }
                            let hash = imaging::image_hash(&rgba);
                            store_image(&app, &rgba, hash);
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

/// Écrit une image dans le presse-papiers via le thread watcher. Renvoie le
/// hash de l'image relue (voir [`WatcherMsg::WriteImage`]).
pub fn write_image_to_clipboard(app: &AppHandle, image: imaging::Rgba) -> Result<Option<String>, String> {
    let state = app.state::<AppState>();
    let (ack_tx, ack_rx) = mpsc::channel();
    state
        .watcher_tx
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .send(WatcherMsg::WriteImage { image, ack: ack_tx })
        .map_err(|e| e.to_string())?;
    match ack_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Some(hash)) => Ok(Some(hash)),
        Ok(None) => Err("écriture de l'image dans le presse-papiers impossible".to_string()),
        Err(_) => Ok(None),
    }
}

/// Place le contenu d'un élément (texte ou image) dans le presse-papiers via le
/// thread watcher — pour ne pas le réinsérer dans l'historique — puis
/// incrémente son compteur d'usage. L'utilisateur colle ensuite (Ctrl+V).
pub fn write_item_to_clipboard(app: &AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();

    let (content, png) = {
        let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
        let png = db::get_image_png(&conn, id).map_err(|e| e.to_string())?;
        let content = if png.is_some() {
            String::new()
        } else {
            db::get_content(&conn, id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "élément introuvable".to_string())?
        };
        (content, png)
    };

    if let Some(png) = png {
        write_image_to_clipboard(app, imaging::decode(&png)?)?;
    } else {
        let (ack_tx, ack_rx) = mpsc::channel();
        state
            .watcher_tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .send(WatcherMsg::Write { text: content, ack: ack_tx })
            .map_err(|e| e.to_string())?;
        let _ = ack_rx.recv_timeout(Duration::from_secs(1));
    }

    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    let _ = db::mark_used(&conn, id);
    Ok(())
}
