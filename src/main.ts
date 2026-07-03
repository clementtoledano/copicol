import {
  deleteItem,
  hideWindow,
  listCategories,
  listItems,
  onClipboardChanged,
  onWindowShown,
  pasteItem,
  setCategory,
  togglePin,
  type Category,
  type Item,
} from "./api";

const searchInput = document.getElementById("search") as HTMLInputElement;
const listEl = document.getElementById("list") as HTMLUListElement;
const categoriesEl = document.getElementById("categories") as HTMLDivElement;

let items: Item[] = [];
let categories: Category[] = [];
let selectedIndex = 0;
let activeCategory: number | null = null;

function relativeTime(unixSeconds: number): string {
  const diff = Math.floor(Date.now() / 1000) - unixSeconds;
  if (diff < 60) return "à l'instant";
  if (diff < 3600) return `il y a ${Math.floor(diff / 60)} min`;
  if (diff < 86400) return `il y a ${Math.floor(diff / 3600)} h`;
  return `il y a ${Math.floor(diff / 86400)} j`;
}

function categoryName(id: number | null): string | null {
  if (id === null) return null;
  return categories.find((c) => c.id === id)?.name ?? null;
}

async function refresh(): Promise<void> {
  items = await listItems(searchInput.value.trim(), activeCategory);
  if (selectedIndex >= items.length) selectedIndex = Math.max(0, items.length - 1);
  renderList();
}

function renderCategories(): void {
  categoriesEl.innerHTML = "";
  const all = document.createElement("button");
  all.textContent = "Tous";
  all.className = activeCategory === null ? "chip active" : "chip";
  all.addEventListener("click", () => {
    activeCategory = null;
    selectedIndex = 0;
    renderCategories();
    void refresh();
  });
  categoriesEl.appendChild(all);

  for (const cat of categories) {
    const btn = document.createElement("button");
    btn.textContent = cat.name;
    btn.className = activeCategory === cat.id ? "chip active" : "chip";
    btn.addEventListener("click", () => {
      activeCategory = activeCategory === cat.id ? null : cat.id;
      selectedIndex = 0;
      renderCategories();
      void refresh();
    });
    categoriesEl.appendChild(btn);
  }
}

function renderList(): void {
  listEl.innerHTML = "";

  if (items.length === 0) {
    const empty = document.createElement("li");
    empty.className = "empty";
    empty.textContent = searchInput.value
      ? "Aucun résultat"
      : "L'historique est vide — copiez quelque chose !";
    listEl.appendChild(empty);
    return;
  }

  items.forEach((item, index) => {
    const li = document.createElement("li");
    li.className = index === selectedIndex ? "item selected" : "item";
    if (item.pinned) li.classList.add("pinned");

    const preview = document.createElement("div");
    preview.className = "preview";
    preview.textContent = item.content;

    const meta = document.createElement("div");
    meta.className = "meta";
    if (item.pinned) {
      const pin = document.createElement("span");
      pin.className = "badge pin-badge";
      pin.textContent = "⭐";
      meta.appendChild(pin);
    }
    const catName = categoryName(item.category_id);
    if (catName) {
      const badge = document.createElement("span");
      badge.className = "badge cat-badge";
      badge.textContent = catName;
      meta.appendChild(badge);
    }
    const time = document.createElement("span");
    time.className = "time";
    time.textContent = relativeTime(item.created_at);
    meta.appendChild(time);

    // Actions au survol : épingler, catégoriser, supprimer
    const actions = document.createElement("div");
    actions.className = "actions";

    const pinBtn = document.createElement("button");
    pinBtn.title = item.pinned ? "Désépingler" : "Épingler";
    pinBtn.textContent = item.pinned ? "📌" : "📍";
    pinBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      void togglePin(item.id).then(refresh);
    });
    actions.appendChild(pinBtn);

    const catSelect = document.createElement("select");
    catSelect.title = "Catégorie";
    const none = document.createElement("option");
    none.value = "";
    none.textContent = "—";
    catSelect.appendChild(none);
    for (const cat of categories) {
      const opt = document.createElement("option");
      opt.value = String(cat.id);
      opt.textContent = cat.name;
      if (item.category_id === cat.id) opt.selected = true;
      catSelect.appendChild(opt);
    }
    catSelect.addEventListener("click", (e) => e.stopPropagation());
    catSelect.addEventListener("change", (e) => {
      e.stopPropagation();
      const value = catSelect.value === "" ? null : Number(catSelect.value);
      void setCategory(item.id, value).then(refresh);
    });
    actions.appendChild(catSelect);

    const delBtn = document.createElement("button");
    delBtn.title = "Supprimer";
    delBtn.textContent = "🗑️";
    delBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      void deleteItem(item.id).then(refresh);
    });
    actions.appendChild(delBtn);

    li.appendChild(preview);
    li.appendChild(meta);
    li.appendChild(actions);

    li.addEventListener("click", () => void pasteItem(item.id));
    li.addEventListener("mousemove", () => {
      if (selectedIndex !== index) {
        selectedIndex = index;
        updateSelection();
      }
    });

    listEl.appendChild(li);
  });
}

function updateSelection(): void {
  const nodes = listEl.querySelectorAll<HTMLLIElement>("li.item");
  nodes.forEach((node, index) => {
    node.classList.toggle("selected", index === selectedIndex);
  });
  nodes[selectedIndex]?.scrollIntoView({ block: "nearest" });
}

function moveSelection(delta: number): void {
  if (items.length === 0) return;
  selectedIndex = (selectedIndex + delta + items.length) % items.length;
  updateSelection();
}

document.addEventListener("keydown", (e) => {
  switch (e.key) {
    case "ArrowDown":
      e.preventDefault();
      moveSelection(1);
      break;
    case "ArrowUp":
      e.preventDefault();
      moveSelection(-1);
      break;
    case "Enter": {
      e.preventDefault();
      const item = items[selectedIndex];
      if (item) void pasteItem(item.id);
      break;
    }
    case "Escape":
      e.preventDefault();
      void hideWindow();
      break;
    case "Delete": {
      const item = items[selectedIndex];
      if (item && document.activeElement !== searchInput) {
        e.preventDefault();
        void deleteItem(item.id).then(refresh);
      }
      break;
    }
    case "p":
    case "P": {
      if (e.ctrlKey) {
        e.preventDefault();
        const item = items[selectedIndex];
        if (item) void togglePin(item.id).then(refresh);
      }
      break;
    }
  }
});

searchInput.addEventListener("input", () => {
  selectedIndex = 0;
  void refresh();
});

async function init(): Promise<void> {
  categories = await listCategories();
  renderCategories();
  await refresh();

  await onClipboardChanged(() => void refresh());
  await onWindowShown(() => {
    searchInput.value = "";
    selectedIndex = 0;
    searchInput.focus();
    void refresh();
  });

  searchInput.focus();
}

void init();
