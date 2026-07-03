import {
  deleteItem,
  hideWindow,
  listItems,
  listKinds,
  onClipboardChanged,
  onWindowShown,
  pasteItem,
  togglePin,
  type Item,
  type KindCount,
} from "./api";

const searchInput = document.getElementById("search") as HTMLInputElement;
const listEl = document.getElementById("list") as HTMLUListElement;
const tabsEl = document.getElementById("tabs") as HTMLDivElement;

const KIND_META: Record<string, { label: string; icon: string }> = {
  sql: { label: "SQL", icon: "🗄️" },
  code: { label: "Code", icon: "⌨️" },
  url: { label: "URL", icon: "🔗" },
  email: { label: "Email", icon: "✉️" },
  json: { label: "JSON", icon: "🧩" },
  color: { label: "Couleur", icon: "🎨" },
  phone: { label: "Téléphone", icon: "📞" },
  path: { label: "Chemin", icon: "📁" },
  text: { label: "Texte", icon: "📄" },
};

function kindMeta(kind: string): { label: string; icon: string } {
  return KIND_META[kind] ?? { label: kind, icon: "📄" };
}

let items: Item[] = [];
let kinds: KindCount[] = [];
let selectedIndex = 0;
let activeKind: string | null = null;

const MONO_KINDS = new Set(["sql", "code", "json"]);

function relativeTime(unixSeconds: number): string {
  const diff = Math.floor(Date.now() / 1000) - unixSeconds;
  if (diff < 60) return "à l'instant";
  if (diff < 3600) return `il y a ${Math.floor(diff / 60)} min`;
  if (diff < 86400) return `il y a ${Math.floor(diff / 3600)} h`;
  return `il y a ${Math.floor(diff / 86400)} j`;
}

async function refresh(): Promise<void> {
  kinds = await listKinds();
  // L'onglet actif a pu se vider (suppression du dernier élément) : retour à Tous
  if (activeKind !== null && !kinds.some((k) => k.kind === activeKind)) {
    activeKind = null;
  }
  items = await listItems(searchInput.value.trim(), activeKind);
  if (selectedIndex >= items.length) selectedIndex = Math.max(0, items.length - 1);
  renderTabs();
  renderList();
}

function renderTabs(): void {
  tabsEl.innerHTML = "";

  const total = kinds.reduce((sum, k) => sum + k.count, 0);
  const all = document.createElement("button");
  all.className = activeKind === null ? "chip active" : "chip";
  all.textContent = total > 0 ? `Tous (${total})` : "Tous";
  all.addEventListener("click", () => {
    activeKind = null;
    selectedIndex = 0;
    void refresh();
  });
  tabsEl.appendChild(all);

  // Un onglet par type présent dans l'historique, les plus fournis d'abord
  for (const kc of kinds) {
    const meta = kindMeta(kc.kind);
    const btn = document.createElement("button");
    btn.className = activeKind === kc.kind ? "chip active" : "chip";
    btn.textContent = `${meta.icon} ${meta.label} (${kc.count})`;
    btn.addEventListener("click", () => {
      activeKind = activeKind === kc.kind ? null : kc.kind;
      selectedIndex = 0;
      void refresh();
    });
    tabsEl.appendChild(btn);
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
    preview.className = MONO_KINDS.has(item.kind) ? "preview mono" : "preview";
    fillPreview(preview, item.content, searchInput.value.trim());

    const meta = document.createElement("div");
    meta.className = "meta";
    if (item.pinned) {
      const pin = document.createElement("span");
      pin.className = "badge pin-badge";
      pin.textContent = "⭐";
      meta.appendChild(pin);
    }

    const kindBadge = document.createElement("span");
    kindBadge.className = "badge kind-badge";
    const km = kindMeta(item.kind);
    if (item.kind === "color") {
      // Pastille de la couleur réelle à côté du libellé
      const swatch = document.createElement("span");
      swatch.className = "swatch";
      swatch.style.backgroundColor = item.content.trim();
      kindBadge.appendChild(swatch);
      kindBadge.appendChild(document.createTextNode(` ${km.label}`));
    } else {
      kindBadge.textContent = `${km.icon} ${km.label}`;
    }
    meta.appendChild(kindBadge);

    const time = document.createElement("span");
    time.className = "time";
    time.textContent = relativeTime(item.created_at);
    time.title = new Date(item.created_at * 1000).toLocaleString("fr-FR");
    meta.appendChild(time);

    // Actions au survol : épingler, supprimer
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

    li.addEventListener("click", () => pasteWithFlash(index));
    li.addEventListener("mousemove", () => {
      if (selectedIndex !== index) {
        selectedIndex = index;
        updateSelection();
      }
    });

    listEl.appendChild(li);
  });
}

/// Remplit l'aperçu en surlignant les occurrences du terme recherché
/// (construction DOM sans innerHTML).
function fillPreview(el: HTMLElement, content: string, term: string): void {
  if (!term) {
    el.textContent = content;
    return;
  }
  const lower = content.toLowerCase();
  const needle = term.toLowerCase();
  let pos = 0;
  let idx = lower.indexOf(needle);
  while (idx !== -1) {
    if (idx > pos) el.appendChild(document.createTextNode(content.slice(pos, idx)));
    const mark = document.createElement("mark");
    mark.textContent = content.slice(idx, idx + needle.length);
    el.appendChild(mark);
    pos = idx + needle.length;
    idx = lower.indexOf(needle, pos);
  }
  if (pos < content.length) el.appendChild(document.createTextNode(content.slice(pos)));
}

/// Flash de confirmation sur l'élément, puis collage.
function pasteWithFlash(index: number): void {
  const item = items[index];
  if (!item) return;
  const node = listEl.querySelectorAll<HTMLLIElement>("li.item")[index];
  node?.classList.add("pasting");
  window.setTimeout(() => void pasteItem(item.id), 100);
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
      pasteWithFlash(selectedIndex);
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
