import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Item {
  id: number;
  content: string;
  pinned: boolean;
  category_id: number | null;
  created_at: number;
  use_count: number;
}

export interface Category {
  id: number;
  name: string;
}

export function listItems(search: string, category: number | null): Promise<Item[]> {
  return invoke("list_items", {
    search: search || null,
    category,
  });
}

export function listCategories(): Promise<Category[]> {
  return invoke("list_categories");
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

export function setCategory(id: number, category: number | null): Promise<void> {
  return invoke("set_category", { id, category });
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
