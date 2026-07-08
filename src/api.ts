import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Item {
  id: number;
  content: string;
  kind: string;
  pinned: boolean;
  /** Nom descriptif du favori ; null tant qu'il n'a jamais été nommé. */
  label: string | null;
  /** Dossier du favori ; null = « sans dossier ». */
  group_id: number | null;
  created_at: number;
  use_count: number;
}

export interface KindCount {
  kind: string;
  count: number;
}

/** Un dossier de favoris créé par l'utilisateur. */
export interface Group {
  id: number;
  name: string;
}

export function listItems(search: string, kind: string | null): Promise<Item[]> {
  return invoke("list_items", {
    search: search || null,
    kind,
  });
}

export function listKinds(): Promise<KindCount[]> {
  return invoke("list_kinds");
}

/** Favoris (épinglés) classés par nom ; `search` filtre sur le nom et le contenu. */
export function listFavorites(search: string): Promise<Item[]> {
  return invoke("list_favorites", { search: search || null });
}

/** Nombre de favoris, pour le compteur de l'onglet. */
export function countFavorites(): Promise<number> {
  return invoke("count_favorites");
}

/** Copie l'élément dans le presse-papiers et cache la fenêtre (pas de collage auto). */
export function copyItem(id: number): Promise<void> {
  return invoke("copy_item", { id });
}

/** Épingle l'élément avec un nom descriptif (obligatoire) ; sert aussi à renommer. */
export function pinItem(id: number, label: string): Promise<void> {
  return invoke("pin_item", { id, label });
}

/** Retire l'élément des favoris ; le nom est conservé côté base. */
export function unpinItem(id: number): Promise<void> {
  return invoke("unpin_item", { id });
}

/** Reclasse un favori dans une autre catégorie (indépendamment de la détection auto). */
export function setItemKind(id: number, kind: string): Promise<void> {
  return invoke("set_item_kind", { id, kind });
}

/** Modifie le contenu d'un favori ; nom et catégorie conservés. */
export function updateItemContent(id: number, content: string): Promise<void> {
  return invoke("update_item_content", { id, content });
}

export function deleteItem(id: number): Promise<void> {
  return invoke("delete_item", { id });
}

export function hideWindow(): Promise<void> {
  return invoke("hide_window");
}

// ── Dossiers de favoris ─────────────────────────────────────────────

/** Liste les dossiers de favoris, classés par nom. */
export function listGroups(): Promise<Group[]> {
  return invoke("list_groups");
}

/** Crée un dossier (ou renvoie l'existant) et retourne son identifiant. */
export function createGroup(name: string): Promise<number> {
  return invoke("create_group", { name });
}

/** Renomme un dossier. */
export function renameGroup(id: number, name: string): Promise<void> {
  return invoke("rename_group", { id, name });
}

/** Supprime un dossier ; ses favoris repassent « sans dossier ». */
export function deleteGroup(id: number): Promise<void> {
  return invoke("delete_group", { id });
}

/** Affecte un favori à un dossier (ou l'en retire avec `groupId = null`). */
export function setItemGroup(id: number, groupId: number | null): Promise<void> {
  return invoke("set_item_group", { id, groupId });
}

/** Vide l'historique (les favoris sont conservés). */
export function clearHistory(): Promise<void> {
  return invoke("clear_history");
}

/** Quitte complètement l'application. */
export function quitApp(): Promise<void> {
  return invoke("quit_app");
}

/** Préférence actuelle de démarrage automatique avec la session (activée par défaut). */
export function getAutostartEnabled(): Promise<boolean> {
  return invoke("get_autostart_enabled");
}

/** Active ou désactive le démarrage automatique avec la session. */
export function setAutostartEnabled(enabled: boolean): Promise<void> {
  return invoke("set_autostart_enabled", { enabled });
}

/** Version de l'application (depuis tauri.conf.json). */
export function appVersion(): Promise<string> {
  return getVersion();
}

/** S'abonne à la demande d'ouverture de la fenêtre « À propos » (depuis le tray). */
export function onShowAbout(cb: () => void): Promise<UnlistenFn> {
  return listen("show-about", cb);
}

/** Ouvre le dépôt GitHub dans le navigateur par défaut (commande système côté Rust). */
export function openRepoUrl(): Promise<void> {
  return invoke("open_repo_url");
}

// ── Import / export des favoris ─────────────────────────────────────

/** Résumé d'un import : favoris ajoutés et favoris ignorés (déjà présents). */
export interface ImportSummary {
  imported: number;
  skipped: number;
}

/** Exporte les favoris (JSON) dans `path` ; renvoie le nombre exporté. */
export function exportFavorites(path: string): Promise<number> {
  return invoke("export_favorites", { path });
}

/** Importe/fusionne les favoris depuis le fichier JSON `path`. */
export function importFavorites(path: string): Promise<ImportSummary> {
  return invoke("import_favorites", { path });
}

/** Boîte « Enregistrer sous… » filtrée sur .json ; renvoie le chemin ou null. */
export async function pickSavePath(defaultName: string): Promise<string | null> {
  const { save } = await import("@tauri-apps/plugin-dialog");
  return save({ defaultPath: defaultName, filters: [{ name: "JSON", extensions: ["json"] }] });
}

/** Boîte « Ouvrir… » filtrée sur .json ; renvoie le chemin ou null. */
export async function pickOpenPath(): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const result = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  return typeof result === "string" ? result : null;
}

export function onClipboardChanged(cb: () => void): Promise<UnlistenFn> {
  return listen("clipboard-changed", cb);
}

export function onWindowShown(cb: () => void): Promise<UnlistenFn> {
  return listen("window-shown", cb);
}

export type UpdateCheckResult =
  | { available: true; version: string; notes: string }
  | { available: false; offline: boolean };

/** La mise à jour trouvée par le dernier `checkUpdate`, prête à installer. */
let pendingUpdate: Awaited<ReturnType<typeof import("@tauri-apps/plugin-updater")["check"]>> | null =
  null;

/// Cherche une mise à jour sans rien installer. Le résultat distingue « aucune
/// mise à jour » de « vérification impossible » (ex. hors ligne), pour que
/// l'appelant puisse adapter son message. La mise à jour trouvée est mémorisée
/// pour un appel ultérieur à `installPendingUpdate`.
export async function checkUpdate(): Promise<UpdateCheckResult> {
  const { check } = await import("@tauri-apps/plugin-updater");
  try {
    pendingUpdate = await check();
  } catch {
    pendingUpdate = null;
    return { available: false, offline: true };
  }
  if (!pendingUpdate) return { available: false, offline: false };
  return { available: true, version: pendingUpdate.version, notes: pendingUpdate.body ?? "" };
}

/// Télécharge et installe la mise à jour trouvée par `checkUpdate`, puis
/// redémarre l'application. Ne fait rien si aucune mise à jour n'est en attente.
export async function installPendingUpdate(): Promise<void> {
  if (!pendingUpdate) return;
  const { relaunch } = await import("@tauri-apps/plugin-process");
  await pendingUpdate.downloadAndInstall();
  await relaunch();
}

/// Demande la permission d'afficher des notifications système si elle n'a pas
/// déjà été accordée (ou refusée) lors d'un appel précédent.
export async function ensureNotificationPermission(): Promise<boolean> {
  const { isPermissionGranted, requestPermission } = await import("@tauri-apps/plugin-notification");
  if (await isPermissionGranted()) return true;
  return (await requestPermission()) === "granted";
}

/// Notifie via une notification système (visible même fenêtre cachée/en tray)
/// qu'une mise à jour est disponible.
export async function notifyUpdateAvailable(version: string): Promise<void> {
  const { sendNotification } = await import("@tauri-apps/plugin-notification");
  sendNotification({
    title: "Mise à jour copicol disponible",
    body: `Version ${version} — ouvrez copicol pour l'installer.`,
  });
}
