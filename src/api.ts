import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Item {
  id: number;
  content: string;
  kind: string;
  pinned: boolean;
  /** Nom descriptif du favori ; null tant qu'il n'a jamais été nommé. */
  label: string | null;
  created_at: number;
  use_count: number;
}

export interface KindCount {
  kind: string;
  count: number;
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

export function pasteItem(id: number): Promise<void> {
  return invoke("paste_item", { id });
}

/** Épingle l'élément avec un nom descriptif (obligatoire) ; sert aussi à renommer. */
export function pinItem(id: number, label: string): Promise<void> {
  return invoke("pin_item", { id, label });
}

/** Retire l'élément des favoris ; le nom est conservé côté base. */
export function unpinItem(id: number): Promise<void> {
  return invoke("unpin_item", { id });
}

export function deleteItem(id: number): Promise<void> {
  return invoke("delete_item", { id });
}

export function hideWindow(): Promise<void> {
  return invoke("hide_window");
}

export function onClipboardChanged(cb: () => void): Promise<UnlistenFn> {
  return listen("clipboard-changed", cb);
}

export function onWindowShown(cb: () => void): Promise<UnlistenFn> {
  return listen("window-shown", cb);
}
