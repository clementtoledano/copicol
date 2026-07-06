import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  checkForUpdate,
  countFavorites,
  deleteItem,
  hideWindow,
  listFavorites,
  listItems,
  listKinds,
  onClipboardChanged,
  onWindowShown,
  pasteItem,
  pinItem,
  setItemKind,
  unpinItem,
  updateItemContent,
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

/// Catégories proposées pour la reclassification manuelle d'un favori
/// (miroir de `db::ALL_KINDS` côté backend).
const ALL_KINDS = ["sql", "code", "url", "email", "json", "color", "phone", "path", "text"];

let items: Item[] = [];
let kinds: KindCount[] = [];
let favCount = 0;
let selectedIndex = 0;
let activeKind: string | null = null;
// Onglet Favoris actif : affiche les épinglés (exclusifs des autres onglets).
let showFavorites = false;
// Favoris dont la carte est dépliée (aperçu visible) dans l'onglet Favoris.
const expandedFavIds = new Set<number>();

const MONO_KINDS = new Set(["sql", "code", "json"]);

function relativeTime(unixSeconds: number): string {
  const diff = Math.floor(Date.now() / 1000) - unixSeconds;
  if (diff < 60) return "à l'instant";
  if (diff < 3600) return `il y a ${Math.floor(diff / 60)} min`;
  if (diff < 86400) return `il y a ${Math.floor(diff / 3600)} h`;
  return `il y a ${Math.floor(diff / 86400)} j`;
}

// ── Modales ─────────────────────────────────────────────
// Tant qu'une modale est ouverte, la navigation clavier de la liste est
// suspendue (voir le garde en tête du gestionnaire keydown global).
let modalOpen = false;

/// Invite à saisir un nom descriptif. Le nom est OBLIGATOIRE : « Enregistrer »
/// reste désactivé tant que le champ est vide. Résout le nom (sans espaces
/// superflus) ou `null` si l'utilisateur renonce (Échap, Annuler, clic dehors).
function promptName(current: string): Promise<string | null> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    const box = document.createElement("div");
    box.className = "modal";

    const title = document.createElement("div");
    title.className = "modal-title";
    title.textContent = current ? "Renommer le favori" : "Nommer le favori";

    const input = document.createElement("input");
    input.className = "modal-input";
    input.type = "text";
    input.placeholder = "Nom descriptif…";
    input.value = current;
    input.autocomplete = "off";
    input.spellcheck = false;
    input.maxLength = 80;

    const actions = document.createElement("div");
    actions.className = "modal-actions";
    const cancel = document.createElement("button");
    cancel.className = "modal-btn";
    cancel.textContent = "Annuler";
    const save = document.createElement("button");
    save.className = "modal-btn primary";
    save.textContent = "Enregistrer";

    const value = (): string => input.value.trim();
    const sync = (): void => {
      save.disabled = value().length === 0;
    };
    const close = (result: string | null): void => {
      modalOpen = false;
      overlay.remove();
      resolve(result);
    };

    input.addEventListener("input", sync);
    input.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Enter") {
        e.preventDefault();
        if (value()) close(value());
      } else if (e.key === "Escape") {
        e.preventDefault();
        close(null);
      }
    });
    cancel.addEventListener("click", () => close(null));
    save.addEventListener("click", () => {
      if (value()) close(value());
    });
    overlay.addEventListener("mousedown", (e) => {
      if (e.target === overlay) close(null);
    });

    actions.append(cancel, save);
    box.append(title, input, actions);
    overlay.append(box);
    document.body.appendChild(overlay);

    modalOpen = true;
    sync();
    input.focus();
    input.select();
  });
}

/// Propose les catégories connues sous forme de grille de boutons ; celle du
/// favori est mise en évidence. Résout la catégorie choisie ou `null`
/// (Échap, Annuler, clic dehors, ou clic sur la catégorie déjà active).
function promptKind(current: string): Promise<string | null> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    const box = document.createElement("div");
    box.className = "modal";

    const title = document.createElement("div");
    title.className = "modal-title";
    title.textContent = "Changer de catégorie";

    const close = (result: string | null): void => {
      modalOpen = false;
      overlay.remove();
      resolve(result);
    };

    const grid = document.createElement("div");
    grid.className = "kind-picker";
    for (const kind of ALL_KINDS) {
      const meta = kindMeta(kind);
      const btn = document.createElement("button");
      btn.className = kind === current ? "modal-btn kind-option active" : "modal-btn kind-option";
      btn.textContent = `${meta.icon} ${meta.label}`;
      btn.addEventListener("click", () => close(kind === current ? null : kind));
      grid.appendChild(btn);
    }

    const actions = document.createElement("div");
    actions.className = "modal-actions";
    const cancel = document.createElement("button");
    cancel.className = "modal-btn";
    cancel.textContent = "Annuler";
    cancel.addEventListener("click", () => close(null));
    actions.appendChild(cancel);

    overlay.addEventListener("mousedown", (e) => {
      if (e.target === overlay) close(null);
    });
    overlay.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape") {
        e.preventDefault();
        close(null);
      }
    });

    box.append(title, grid, actions);
    overlay.append(box);
    document.body.appendChild(overlay);
    modalOpen = true;
  });
}

/// Édite le contenu d'un favori dans une zone de texte pré-remplie. Résout le
/// nouveau contenu ou `null` si inchangé/vide/annulé.
function promptContent(current: string): Promise<string | null> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    const box = document.createElement("div");
    box.className = "modal modal-wide";

    const title = document.createElement("div");
    title.className = "modal-title";
    title.textContent = "Modifier le contenu";

    const textarea = document.createElement("textarea");
    textarea.className = "modal-textarea";
    textarea.value = current;
    textarea.spellcheck = false;

    const actions = document.createElement("div");
    actions.className = "modal-actions";
    const cancel = document.createElement("button");
    cancel.className = "modal-btn";
    cancel.textContent = "Annuler";
    const save = document.createElement("button");
    save.className = "modal-btn primary";
    save.textContent = "Enregistrer";

    const close = (result: string | null): void => {
      modalOpen = false;
      overlay.remove();
      resolve(result);
    };
    const trySave = (): void => {
      const value = textarea.value;
      close(value.trim() && value !== current ? value : null);
    };

    textarea.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape") {
        e.preventDefault();
        close(null);
      } else if (e.key === "Enter" && e.ctrlKey) {
        e.preventDefault();
        trySave();
      }
    });
    cancel.addEventListener("click", () => close(null));
    save.addEventListener("click", trySave);
    overlay.addEventListener("mousedown", (e) => {
      if (e.target === overlay) close(null);
    });

    actions.append(cancel, save);
    box.append(title, textarea, actions);
    overlay.append(box);
    document.body.appendChild(overlay);

    modalOpen = true;
    textarea.focus();
    textarea.select();
  });
}

/// Demande une confirmation oui/non. Résout `true` si confirmé.
function confirmAction(message: string, confirmLabel: string): Promise<boolean> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    const box = document.createElement("div");
    box.className = "modal";

    const title = document.createElement("div");
    title.className = "modal-title";
    title.textContent = message;

    const actions = document.createElement("div");
    actions.className = "modal-actions";
    const cancel = document.createElement("button");
    cancel.className = "modal-btn";
    cancel.textContent = "Annuler";
    const confirm = document.createElement("button");
    confirm.className = "modal-btn danger";
    confirm.textContent = confirmLabel;

    const close = (result: boolean): void => {
      modalOpen = false;
      overlay.remove();
      resolve(result);
    };

    cancel.addEventListener("click", () => close(false));
    confirm.addEventListener("click", () => close(true));
    overlay.addEventListener("mousedown", (e) => {
      if (e.target === overlay) close(false);
    });
    overlay.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape") {
        e.preventDefault();
        close(false);
      } else if (e.key === "Enter") {
        e.preventDefault();
        close(true);
      }
    });

    actions.append(cancel, confirm);
    box.append(title, actions);
    overlay.append(box);
    document.body.appendChild(overlay);

    modalOpen = true;
    confirm.focus();
  });
}

/// Ferme toute modale résiduelle (ex. la fenêtre a été cachée pendant la saisie).
function closeModals(): void {
  document.querySelectorAll(".modal-overlay").forEach((el) => el.remove());
  modalOpen = false;
}

/// Épinglage (nom obligatoire) ou désépinglage (confirmation, nom conservé).
async function togglePinFlow(item: Item): Promise<void> {
  if (item.pinned) {
    const name = item.label ?? "ce favori";
    if (await confirmAction(`Retirer « ${name} » des favoris ?`, "Retirer")) {
      await unpinItem(item.id);
      await refresh();
    }
  } else {
    const name = await promptName("");
    if (name) {
      await pinItem(item.id, name);
      await refresh();
    }
  }
}

/// Renomme un favori via l'invite pré-remplie (nom toujours obligatoire).
async function renameFavorite(item: Item): Promise<void> {
  if (!item.pinned) return;
  const name = await promptName(item.label ?? "");
  if (name) {
    await pinItem(item.id, name);
    await refresh();
  }
}

/// Reclasse un favori via la grille de catégories.
async function changeFavoriteKind(item: Item): Promise<void> {
  if (!item.pinned) return;
  const kind = await promptKind(item.kind);
  if (kind) {
    await setItemKind(item.id, kind);
    await refresh();
  }
}

/// Édite le contenu d'un favori. Un conflit avec un contenu existant (même
/// texte déjà présent ailleurs) est rare ; en cas d'échec l'utilisateur est
/// prévenu et peut rouvrir l'édition pour corriger.
async function editFavoriteContent(item: Item): Promise<void> {
  if (!item.pinned) return;
  const content = await promptContent(item.content);
  if (content === null) return;
  try {
    await updateItemContent(item.id, content);
    await refresh();
  } catch (err) {
    console.error(err);
    alert("Impossible d'enregistrer : ce contenu existe déjà ailleurs dans l'historique.");
  }
}

async function refresh(): Promise<void> {
  const search = searchInput.value.trim();
  [kinds, favCount] = await Promise.all([listKinds(), countFavorites()]);
  // Les onglets ont pu se vider (dernier élément retiré) : retour à Tous
  if (showFavorites && favCount === 0) showFavorites = false;
  if (activeKind !== null && !kinds.some((k) => k.kind === activeKind)) {
    activeKind = null;
  }
  if (showFavorites) {
    // Réordonné pour un affichage groupé par catégorie (voir renderFavorites)
    items = groupFavoritesByKind(await listFavorites(search));
  } else {
    items = await listItems(search, activeKind);
  }
  if (selectedIndex >= items.length) selectedIndex = Math.max(0, items.length - 1);
  renderTabs();
  renderList();
}

function renderTabs(): void {
  tabsEl.innerHTML = "";

  const total = kinds.reduce((sum, k) => sum + k.count, 0);
  const all = document.createElement("button");
  all.className = !showFavorites && activeKind === null ? "chip active" : "chip";
  all.textContent = total > 0 ? `Tous (${total})` : "Tous";
  all.addEventListener("click", () => {
    showFavorites = false;
    activeKind = null;
    selectedIndex = 0;
    void refresh();
  });
  tabsEl.appendChild(all);

  // Onglet Favoris, dès qu'il existe au moins un favori
  if (favCount > 0) {
    const fav = document.createElement("button");
    fav.className = showFavorites ? "chip active" : "chip";
    fav.textContent = `⭐ Favoris (${favCount})`;
    fav.addEventListener("click", () => {
      showFavorites = !showFavorites;
      activeKind = null;
      selectedIndex = 0;
      void refresh();
    });
    tabsEl.appendChild(fav);
  }

  // Un onglet par type présent dans l'historique, les plus fournis d'abord
  for (const kc of kinds) {
    const meta = kindMeta(kc.kind);
    const btn = document.createElement("button");
    btn.className = !showFavorites && activeKind === kc.kind ? "chip active" : "chip";
    btn.textContent = `${meta.icon} ${meta.label} (${kc.count})`;
    btn.addEventListener("click", () => {
      showFavorites = false;
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
    if (showFavorites) {
      empty.textContent = searchInput.value
        ? "Aucun favori ne correspond"
        : "Aucun favori — épinglez un élément (Ctrl+P)";
    } else {
      empty.textContent = searchInput.value
        ? "Aucun résultat"
        : "L'historique est vide — copiez quelque chose !";
    }
    listEl.appendChild(empty);
    return;
  }

  if (showFavorites) {
    renderFavorites();
    return;
  }

  items.forEach((item, index) => {
    const li = document.createElement("li");
    li.className = index === selectedIndex ? "item selected" : "item";
    if (item.pinned) li.classList.add("pinned");

    // Titre d'un favori nommé : ⭐ + nom, surligné selon la recherche,
    // cliquable pour renommer.
    if (item.pinned && item.label) {
      const titleEl = document.createElement("div");
      titleEl.className = "item-title";
      const star = document.createElement("span");
      star.textContent = "⭐";
      titleEl.appendChild(star);
      const label = document.createElement("span");
      label.className = "item-title-text";
      fillPreview(label, item.label, searchInput.value.trim());
      titleEl.appendChild(label);
      titleEl.title = "Renommer (F2)";
      titleEl.addEventListener("click", (e) => {
        e.stopPropagation();
        void renameFavorite(item);
      });
      li.appendChild(titleEl);
    }

    const preview = document.createElement("div");
    preview.className = MONO_KINDS.has(item.kind) ? "preview mono" : "preview";
    fillPreview(preview, item.content, searchInput.value.trim());

    const meta = document.createElement("div");
    meta.className = "meta";
    // Favori hérité sans nom (base d'avant cette fonctionnalité) : on garde
    // le badge ⭐ ; les favoris nommés portent déjà leur ⭐ dans le titre.
    if (item.pinned && !item.label) {
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
      void togglePinFlow(item);
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

/// Réordonne les favoris (déjà triés par nom) en groupes par catégorie,
/// les catégories les plus fournies d'abord. L'ordre du tableau plat suit
/// l'ordre d'affichage, pour que la navigation clavier reste cohérente.
function groupFavoritesByKind(favs: Item[]): Item[] {
  const groups = new Map<string, Item[]>();
  for (const item of favs) {
    const arr = groups.get(item.kind);
    if (arr) arr.push(item);
    else groups.set(item.kind, [item]);
  }
  return [...groups.entries()]
    .sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0]))
    .flatMap(([, arr]) => arr);
}

/// Rend l'onglet Favoris : en-têtes de catégorie puis lignes compactes
/// (nom + loupe). `items` est déjà en ordre groupé (voir groupFavoritesByKind).
function renderFavorites(): void {
  let currentKind: string | null = null;
  items.forEach((item, index) => {
    if (item.kind !== currentKind) {
      currentKind = item.kind;
      const header = document.createElement("li");
      header.className = "fav-header";
      const meta = kindMeta(item.kind);
      header.textContent = `${meta.icon} ${meta.label}`;
      listEl.appendChild(header);
    }
    listEl.appendChild(buildFavoriteRow(item, index));
  });
}

/// Une ligne de favori : nom + loupe. La loupe déplie/replie la carte en place
/// (aperçu du contenu + actions), sans reconstruire toute la liste.
function buildFavoriteRow(item: Item, index: number): HTMLLIElement {
  const li = document.createElement("li");
  li.className = index === selectedIndex ? "item fav-item selected" : "item fav-item";
  const expanded = expandedFavIds.has(item.id);
  if (expanded) li.classList.add("expanded");

  const head = document.createElement("div");
  head.className = "fav-head";

  const name = document.createElement("span");
  name.className = "fav-name";
  fillPreview(name, item.label ?? "(sans nom)", searchInput.value.trim());
  head.appendChild(name);

  const loupe = document.createElement("button");
  loupe.className = "fav-loupe";
  loupe.title = expanded ? "Replier" : "Aperçu";
  loupe.textContent = expanded ? "▾" : "🔍";
  loupe.addEventListener("click", (e) => {
    e.stopPropagation();
    if (expanded) expandedFavIds.delete(item.id);
    else expandedFavIds.add(item.id);
    li.replaceWith(buildFavoriteRow(item, index)); // bascule en place
  });
  head.appendChild(loupe);
  li.appendChild(head);

  if (expanded) {
    const detail = document.createElement("div");
    detail.className = "fav-detail";

    const preview = document.createElement("div");
    preview.className = MONO_KINDS.has(item.kind) ? "preview mono" : "preview";
    preview.textContent = item.content;
    detail.appendChild(preview);

    const actions = document.createElement("div");
    actions.className = "fav-detail-actions";
    actions.appendChild(favAction("Coller", () => pasteWithFlash(index)));
    actions.appendChild(favAction("Renommer", () => void renameFavorite(item)));
    actions.appendChild(favAction("Modifier", () => void editFavoriteContent(item)));
    actions.appendChild(favAction("Catégorie", () => void changeFavoriteKind(item)));
    actions.appendChild(favAction("Retirer", () => void togglePinFlow(item)));
    detail.appendChild(actions);

    li.appendChild(detail);
  }

  li.addEventListener("click", () => pasteWithFlash(index));
  li.addEventListener("mousemove", () => {
    if (selectedIndex !== index) {
      selectedIndex = index;
      updateSelection();
    }
  });
  return li;
}

/// Bouton d'action de la carte dépliée ; n'interfère pas avec le collage (clic
/// sur la ligne).
function favAction(label: string, onClick: () => void): HTMLButtonElement {
  const btn = document.createElement("button");
  btn.className = "fav-action";
  btn.textContent = label;
  btn.addEventListener("click", (e) => {
    e.stopPropagation();
    onClick();
  });
  return btn;
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
  if (modalOpen) return; // une modale ouverte gère ses propres touches
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
    case "F2": {
      const item = items[selectedIndex];
      if (item && item.pinned) {
        e.preventDefault();
        void renameFavorite(item);
      }
      break;
    }
    case "p":
    case "P": {
      if (e.ctrlKey) {
        e.preventDefault();
        const item = items[selectedIndex];
        if (item) void togglePinFlow(item);
      }
      break;
    }
  }
});

searchInput.addEventListener("input", () => {
  selectedIndex = 0;
  void refresh();
});

/// Poignées de redimensionnement pour la fenêtre sans décorations :
/// 4 bords + 4 coins qui délèguent le drag natif au gestionnaire de fenêtres.
type ResizeDirection = Parameters<
  ReturnType<typeof getCurrentWindow>["startResizeDragging"]
>[0];

function setupResizeHandles(): void {
  const handles: Array<[string, ResizeDirection]> = [
    ["n", "North"],
    ["s", "South"],
    ["e", "East"],
    ["w", "West"],
    ["nw", "NorthWest"],
    ["ne", "NorthEast"],
    ["sw", "SouthWest"],
    ["se", "SouthEast"],
  ];
  for (const [cls, direction] of handles) {
    const zone = document.createElement("div");
    zone.className = `resize-handle resize-${cls}`;
    zone.addEventListener("mousedown", (e) => {
      e.preventDefault();
      void getCurrentWindow().startResizeDragging(direction);
    });
    document.body.appendChild(zone);
  }
}

async function init(): Promise<void> {
  setupResizeHandles();
  await refresh();

  await onClipboardChanged(() => void refresh());
  await onWindowShown(() => {
    closeModals(); // au cas où la fenêtre a été cachée pendant une saisie
    expandedFavIds.clear();
    searchInput.value = "";
    selectedIndex = 0;
    searchInput.focus();
    void refresh();
  });

  searchInput.focus();

  // Vérification silencieuse au démarrage : une seule fois, ne bloque rien
  // si hors ligne ou si aucune mise à jour n'est disponible.
  void checkForUpdate((version) =>
    confirmAction(`Mise à jour ${version} disponible. L'installer et redémarrer ?`, "Mettre à jour"),
  );
}

void init();
