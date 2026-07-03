import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Item {
  id: number;
  content: string;
  kind: string;
  pinned: boolean;
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

export function pasteItem(id: number): Promise<void> {
  return invoke("paste_item", { id });
}

export function togglePin(id: number): Promise<void> {
  return invoke("toggle_pin", { id });
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
