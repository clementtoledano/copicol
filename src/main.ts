import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  appVersion,
  checkUpdate,
  clearHistory,
  copyItem,
  countFavorites,
  createGroup,
  deleteGroup,
  deleteItem,
  ensureNotificationPermission,
  exportFavorites,
  getAutostartEnabled,
  hideWindow,
  importFavorites,
  installPendingUpdate,
  listFavorites,
  listGroups,
  listItems,
  listKinds,
  notifyUpdateAvailable,
  onClipboardChanged,
  onShowAbout,
  onWindowShown,
  openRepoUrl,
  pickOpenPath,
  pickSavePath,
  pinItem,
  quitApp,
  renameGroup,
  setAutostartEnabled,
  setItemGroup,
  setItemKind,
  unpinItem,
  updateItemContent,
  type Group,
  type Item,
  type KindCount,
} from "./api";

/** Dépôt du projet, affiché dans la fenêtre « À propos ». */
const REPO_URL = "https://github.com/clementtoledano/copicol";

const searchInput = document.getElementById("search") as HTMLInputElement;
const listEl = document.getElementById("list") as HTMLUListElement;
const tabsEl = document.getElementById("tabs") as HTMLDivElement;
const menuBtn = document.getElementById("menu-btn") as HTMLButtonElement;

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
let groups: Group[] = [];
let favCount = 0;
let selectedIndex = 0;
let activeKind: string | null = null;
// Onglet Favoris actif : affiche les épinglés (exclusifs des autres onglets).
let showFavorites = false;
// Mode de regroupement dans l'onglet Favoris : par dossier créé par
// l'utilisateur, ou par catégorie auto-détectée.
type FavView = "groups" | "kinds";
let favView: FavView = "groups";
// Favoris dont la carte est dépliée (aperçu visible) dans l'onglet Favoris.
const expandedFavIds = new Set<number>();
// Préférence de démarrage automatique avec Windows ; activée par défaut,
// rechargée au démarrage (voir init) et à chaque ouverture du menu ☰.
let autostartEnabled = true;
// Mise à jour détectée au démarrage (fenêtre encore cachée à ce moment) :
// signalée par notification système, puis proposée à la prochaine ouverture
// de la fenêtre (voir onWindowShown).
let pendingStartupUpdate: { version: string; notes: string } | null = null;

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

/// Identifiant unique pour relier un titre de modale à `aria-labelledby`.
let nextModalTitleId = 0;

/// Piège le focus (Tab / Maj+Tab) à l'intérieur d'une modale, pour éviter de
/// tabuler vers des éléments cachés derrière l'overlay.
function trapFocus(container: HTMLElement): void {
  container.addEventListener("keydown", (e) => {
    if (e.key !== "Tab") return;
    const focusable = container.querySelectorAll<HTMLElement>(
      "input, textarea, button:not(:disabled), [tabindex]:not([tabindex='-1'])",
    );
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  });
}

/// Ajoute sous un champ un compteur « n/max » mis à jour à chaque saisie.
function attachCharCounter(input: HTMLInputElement, max: number): HTMLDivElement {
  const counter = document.createElement("div");
  counter.className = "char-counter";
  const sync = (): void => {
    counter.textContent = `${input.value.length}/${max}`;
    counter.classList.toggle("limit", input.value.length >= max);
  };
  input.addEventListener("input", sync);
  sync();
  return counter;
}

/// Invite à saisir un nom descriptif. Le nom est OBLIGATOIRE : « Enregistrer »
/// reste désactivé tant que le champ est vide. Résout le nom (sans espaces
/// superflus) ou `null` si l'utilisateur renonce (Échap, Annuler, clic dehors).
function promptName(current: string, titleText?: string): Promise<string | null> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    const box = document.createElement("div");
    box.className = "modal";
    box.setAttribute("role", "dialog");
    box.setAttribute("aria-modal", "true");

    const titleId = `modal-title-${nextModalTitleId++}`;
    const title = document.createElement("div");
    title.className = "modal-title";
    title.id = titleId;
    title.textContent = titleText ?? (current ? "Renommer le favori" : "Nommer le favori");
    box.setAttribute("aria-labelledby", titleId);

    const input = document.createElement("input");
    input.className = "modal-input";
    input.type = "text";
    input.placeholder = "Nom descriptif…";
    input.value = current;
    input.autocomplete = "off";
    input.spellcheck = false;
    input.maxLength = 80;
    const counter = attachCharCounter(input, 80);

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
    // Écouteur sur l'overlay (et non sur le seul champ) : Échap et Entrée
    // fonctionnent même si le focus a été déplacé vers un bouton (Tab).
    overlay.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape") {
        e.preventDefault();
        close(null);
      } else if (e.key === "Enter" && document.activeElement === input) {
        e.preventDefault();
        if (value()) close(value());
      }
    });
    cancel.addEventListener("click", () => close(null));
    save.addEventListener("click", () => {
      if (value()) close(value());
    });
    overlay.addEventListener("mousedown", (e) => {
      if (e.target === overlay) close(null);
    });

    trapFocus(box);

    actions.append(cancel, save);
    box.append(title, input, counter, actions);
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

/// Choix du dossier d'un favori : liste des dossiers existants, « Sans dossier »
/// pour l'en retirer, et « Nouveau dossier… » pour en créer un.
/// Résout la sélection ou `null` (Échap, Annuler, clic dehors).
type GroupChoice = { kind: "set"; id: number | null } | { kind: "new" };

function promptGroup(current: number | null): Promise<GroupChoice | null> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    const box = document.createElement("div");
    box.className = "modal";

    const title = document.createElement("div");
    title.className = "modal-title";
    title.textContent = "Ranger dans un dossier";

    const close = (result: GroupChoice | null): void => {
      modalOpen = false;
      overlay.remove();
      resolve(result);
    };

    const list = document.createElement("div");
    list.className = "group-picker";

    const option = (label: string, active: boolean, onClick: () => void): void => {
      const btn = document.createElement("button");
      btn.className = active ? "modal-btn group-option active" : "modal-btn group-option";
      btn.textContent = label;
      btn.addEventListener("click", onClick);
      list.appendChild(btn);
    };

    option("Sans dossier", current === null, () => close({ kind: "set", id: null }));
    for (const g of groups) {
      option(`📁 ${g.name}`, current === g.id, () => close({ kind: "set", id: g.id }));
    }
    option("＋ Nouveau dossier…", false, () => close({ kind: "new" }));

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

    box.append(title, list, actions);
    overlay.append(box);
    document.body.appendChild(overlay);
    modalOpen = true;
  });
}

/// Nomme un nouveau favori et choisit (ou crée) son dossier dans une seule
/// modale, pour éviter l'aller-retour ultérieur par « Dossier » dans les
/// actions. Résout `{ name, groupId }` ou `null` si annulé.
function promptFavoriteCreate(): Promise<{ name: string; groupId: number | null } | null> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    const box = document.createElement("div");
    box.className = "modal";
    box.setAttribute("role", "dialog");
    box.setAttribute("aria-modal", "true");

    const titleId = `modal-title-${nextModalTitleId++}`;
    const title = document.createElement("div");
    title.className = "modal-title";
    title.id = titleId;
    title.textContent = "Nommer le favori";
    box.setAttribute("aria-labelledby", titleId);

    const input = document.createElement("input");
    input.className = "modal-input";
    input.type = "text";
    input.placeholder = "Nom descriptif…";
    input.autocomplete = "off";
    input.spellcheck = false;
    input.maxLength = 80;
    const nameCounter = attachCharCounter(input, 80);

    const groupLabel = document.createElement("div");
    groupLabel.className = "modal-subtitle";
    groupLabel.textContent = "Dossier";

    const list = document.createElement("div");
    list.className = "group-picker";

    const newGroupInput = document.createElement("input");
    newGroupInput.className = "modal-input";
    newGroupInput.type = "text";
    newGroupInput.placeholder = "Nom du nouveau dossier…";
    newGroupInput.autocomplete = "off";
    newGroupInput.spellcheck = false;
    newGroupInput.maxLength = 80;
    newGroupInput.hidden = true;
    const newGroupCounter = attachCharCounter(newGroupInput, 80);
    newGroupCounter.hidden = true;

    let selectedGroupId: number | null = null;
    let creatingNew = false;

    const option = (label: string, active: boolean, onClick: () => void): void => {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = active ? "modal-btn group-option active" : "modal-btn group-option";
      btn.textContent = label;
      btn.addEventListener("click", onClick);
      list.appendChild(btn);
    };
    const renderOptions = (): void => {
      list.innerHTML = "";
      option("Sans dossier", !creatingNew && selectedGroupId === null, () => {
        creatingNew = false;
        selectedGroupId = null;
        newGroupInput.hidden = true;
        newGroupCounter.hidden = true;
        renderOptions();
        sync();
      });
      for (const g of groups) {
        option(`📁 ${g.name}`, !creatingNew && selectedGroupId === g.id, () => {
          creatingNew = false;
          selectedGroupId = g.id;
          newGroupInput.hidden = true;
          newGroupCounter.hidden = true;
          renderOptions();
          sync();
        });
      }
      option("＋ Nouveau dossier…", creatingNew, () => {
        creatingNew = true;
        newGroupInput.hidden = false;
        newGroupCounter.hidden = false;
        renderOptions();
        newGroupInput.focus();
        sync();
      });
    };
    renderOptions();

    const actions = document.createElement("div");
    actions.className = "modal-actions";
    const cancel = document.createElement("button");
    cancel.className = "modal-btn";
    cancel.textContent = "Annuler";
    const save = document.createElement("button");
    save.className = "modal-btn primary";
    save.textContent = "Enregistrer";

    const nameValue = (): string => input.value.trim();
    const newGroupValue = (): string => newGroupInput.value.trim();
    const sync = (): void => {
      save.disabled = nameValue().length === 0 || (creatingNew && newGroupValue().length === 0);
    };
    const close = (result: { name: string; groupId: number | null } | null): void => {
      modalOpen = false;
      overlay.remove();
      resolve(result);
    };
    const submit = async (): Promise<void> => {
      if (save.disabled) return;
      const name = nameValue();
      if (!name) return;
      if (creatingNew) {
        const newName = newGroupValue();
        if (!newName) return;
        const id = await createGroup(newName);
        close({ name, groupId: id });
      } else {
        close({ name, groupId: selectedGroupId });
      }
    };

    input.addEventListener("input", sync);
    newGroupInput.addEventListener("input", sync);
    overlay.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape") {
        e.preventDefault();
        close(null);
      } else if (
        e.key === "Enter" &&
        (document.activeElement === input || document.activeElement === newGroupInput)
      ) {
        e.preventDefault();
        void submit();
      }
    });
    cancel.addEventListener("click", () => close(null));
    save.addEventListener("click", () => void submit());
    overlay.addEventListener("mousedown", (e) => {
      if (e.target === overlay) close(null);
    });

    trapFocus(box);

    actions.append(cancel, save);
    box.append(title, input, nameCounter, groupLabel, list, newGroupInput, newGroupCounter, actions);
    overlay.append(box);
    document.body.appendChild(overlay);

    modalOpen = true;
    sync();
    input.focus();
  });
}

/// Échappe les caractères spéciaux d'une chaîne pour l'utiliser dans un RegExp.
function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/// Édite le contenu d'un favori dans une zone de texte pré-remplie, avec une
/// barre d'outils d'édition (undo/redo, presse-papiers, casse, indentation,
/// rechercher/remplacer). Résout le nouveau contenu ou `null` si
/// inchangé/vide/annulé.
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

    // ── Barre d'outils d'édition ──
    const toolbar = document.createElement("div");
    toolbar.className = "editor-toolbar";

    const makeSep = (): HTMLDivElement => {
      const sep = document.createElement("div");
      sep.className = "sep";
      return sep;
    };
    const makeBtn = (label: string, hint: string, onClick: () => void): HTMLButtonElement => {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "editor-btn";
      btn.textContent = label;
      btn.title = hint;
      // Empêche le bouton de voler le focus/la sélection du textarea.
      btn.addEventListener("mousedown", (e) => e.preventDefault());
      btn.addEventListener("click", (e) => {
        e.preventDefault();
        onClick();
      });
      return btn;
    };

    const focusArea = (): void => textarea.focus();

    const transformCase = (fn: (s: string) => string): void => {
      focusArea();
      const noSelection = textarea.selectionStart === textarea.selectionEnd;
      const start = noSelection ? 0 : textarea.selectionStart;
      const end = noSelection ? textarea.value.length : textarea.selectionEnd;
      textarea.setSelectionRange(start, end);
      const transformed = fn(textarea.value.slice(start, end));
      document.execCommand("insertText", false, transformed);
      textarea.setSelectionRange(start, start + transformed.length);
    };

    const lineBlockBounds = (start: number, end: number): { lineStart: number; lineEnd: number } => {
      const value = textarea.value;
      const lineStart = value.lastIndexOf("\n", start - 1) + 1;
      let lineEnd = value.indexOf("\n", end);
      if (lineEnd === -1) lineEnd = value.length;
      return { lineStart, lineEnd };
    };
    const indentSelection = (remove: boolean): void => {
      focusArea();
      const { lineStart, lineEnd } = lineBlockBounds(textarea.selectionStart, textarea.selectionEnd);
      const newBlock = textarea.value
        .slice(lineStart, lineEnd)
        .split("\n")
        .map((line) => (remove ? line.replace(/^ {1,2}/, "") : "  " + line))
        .join("\n");
      textarea.setSelectionRange(lineStart, lineEnd);
      document.execCommand("insertText", false, newBlock);
      textarea.setSelectionRange(lineStart, lineStart + newBlock.length);
    };

    const pasteFromClipboard = async (): Promise<void> => {
      try {
        const text = await navigator.clipboard.readText();
        focusArea();
        document.execCommand("insertText", false, text);
      } catch {
        // Presse-papiers inaccessible : on ignore silencieusement.
      }
    };

    toolbar.append(
      makeBtn("↶ Annuler", "Annuler (Ctrl+Z)", () => {
        focusArea();
        document.execCommand("undo");
      }),
      makeBtn("↷ Refaire", "Refaire (Ctrl+Y)", () => {
        focusArea();
        document.execCommand("redo");
      }),
      makeSep(),
      makeBtn("✂️ Couper", "Couper la sélection", () => {
        focusArea();
        document.execCommand("cut");
      }),
      makeBtn("📋 Copier", "Copier la sélection", () => {
        focusArea();
        document.execCommand("copy");
      }),
      makeBtn("📥 Coller", "Coller depuis le presse-papiers", () => void pasteFromClipboard()),
      makeSep(),
      makeBtn("⬚ Tout sélectionner", "Tout sélectionner (Ctrl+A)", () => {
        focusArea();
        textarea.select();
      }),
      makeSep(),
      makeBtn("🔠 MAJ", "Mettre en majuscules", () => transformCase((s) => s.toUpperCase())),
      makeBtn("🔡 min", "Mettre en minuscules", () => transformCase((s) => s.toLowerCase())),
      makeSep(),
      makeBtn("⇥ Indenter", "Indenter (Tab)", () => indentSelection(false)),
      makeBtn("⇤ Désindenter", "Désindenter (Maj+Tab)", () => indentSelection(true)),
      makeSep(),
    );

    // ── Barre rechercher/remplacer (repliable) ──
    const findBar = document.createElement("div");
    findBar.className = "find-replace-bar";
    findBar.style.display = "none";
    const findInput = document.createElement("input");
    findInput.type = "text";
    findInput.placeholder = "Rechercher…";
    const replaceInput = document.createElement("input");
    replaceInput.type = "text";
    replaceInput.placeholder = "Remplacer par…";
    const findStatus = document.createElement("span");
    findStatus.className = "find-status";

    const findNext = (): void => {
      const term = findInput.value;
      if (!term) return;
      const re = new RegExp(escapeRegExp(term), "gi");
      const value = textarea.value;
      const matches: number[] = [];
      let m: RegExpExecArray | null;
      while ((m = re.exec(value)) !== null) matches.push(m.index);
      if (matches.length === 0) {
        findStatus.textContent = "Aucune correspondance";
        return;
      }
      const from = textarea.selectionEnd;
      const next = matches.find((i) => i >= from) ?? matches[0];
      focusArea();
      textarea.setSelectionRange(next, next + term.length);
      findStatus.textContent = `${matches.indexOf(next) + 1} / ${matches.length}`;
    };
    const replaceOne = (): void => {
      const term = findInput.value;
      if (!term) return;
      const selected = textarea.value.slice(textarea.selectionStart, textarea.selectionEnd);
      if (selected.length > 0 && selected.toLowerCase() === term.toLowerCase()) {
        focusArea();
        document.execCommand("insertText", false, replaceInput.value);
      }
      findNext();
    };
    const replaceAll = (): void => {
      const term = findInput.value;
      if (!term) return;
      const re = new RegExp(escapeRegExp(term), "gi");
      const value = textarea.value;
      const count = (value.match(re) || []).length;
      if (count === 0) {
        findStatus.textContent = "Aucune correspondance";
        return;
      }
      focusArea();
      textarea.setSelectionRange(0, value.length);
      document.execCommand("insertText", false, value.replace(re, replaceInput.value));
      findStatus.textContent = `${count} remplacement${count > 1 ? "s" : ""}`;
    };

    const findNextBtn = makeBtn("Suivant", "Chercher la prochaine occurrence", findNext);
    const replaceOneBtn = makeBtn("Remplacer", "Remplacer l'occurrence sélectionnée", replaceOne);
    const replaceAllBtn = makeBtn("Tout remplacer", "Remplacer toutes les occurrences", replaceAll);
    findInput.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape") {
        e.preventDefault();
        toggleFindBar(false);
      } else if (e.key === "Enter") {
        e.preventDefault();
        findNext();
      }
    });
    replaceInput.addEventListener("keydown", (e) => e.stopPropagation());
    findBar.append(findInput, replaceInput, findNextBtn, replaceOneBtn, replaceAllBtn, findStatus);

    const toggleFindBar = (show?: boolean): void => {
      const visible = show ?? findBar.style.display === "none";
      findBar.style.display = visible ? "flex" : "none";
      if (visible) findInput.focus();
      else focusArea();
    };
    const findToggleBtn = makeBtn("🔍 Rechercher / Remplacer", "Afficher/masquer la recherche (Ctrl+F)", () =>
      toggleFindBar(),
    );
    const wrapToggleBtn = makeBtn("↩️ Retour à la ligne", "Activer/désactiver le retour à la ligne", () => {
      const wrapped = textarea.classList.toggle("wrap-off");
      wrapToggleBtn.classList.toggle("active", wrapped);
      focusArea();
    });
    toolbar.append(findToggleBtn, makeSep(), wrapToggleBtn);

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
      } else if (e.key === "f" && e.ctrlKey) {
        e.preventDefault();
        toggleFindBar(true);
      } else if (e.key === "Tab") {
        e.preventDefault();
        indentSelection(e.shiftKey);
      }
    });
    cancel.addEventListener("click", () => close(null));
    save.addEventListener("click", trySave);
    overlay.addEventListener("mousedown", (e) => {
      if (e.target === overlay) close(null);
    });

    actions.append(cancel, save);
    box.append(title, toolbar, findBar, textarea, actions);
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
  document.querySelectorAll(".modal-overlay, .menu-overlay").forEach((el) => el.remove());
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
    const result = await promptFavoriteCreate();
    if (result) {
      await pinItem(item.id, result.name);
      if (result.groupId !== null) {
        await setItemGroup(item.id, result.groupId);
      }
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

/// Range un favori dans un dossier existant, dans un nouveau dossier, ou l'en
/// retire. La création d'un nouveau dossier enchaîne sur la saisie de son nom.
async function changeFavoriteGroup(item: Item): Promise<void> {
  if (!item.pinned) return;
  const choice = await promptGroup(item.group_id);
  if (!choice) return;
  if (choice.kind === "new") {
    const name = await promptName("", "Nouveau dossier");
    if (!name) return;
    const id = await createGroup(name);
    await setItemGroup(item.id, id);
  } else {
    await setItemGroup(item.id, choice.id);
  }
  await refresh();
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
  [kinds, favCount, groups] = await Promise.all([listKinds(), countFavorites(), listGroups()]);
  // Les onglets ont pu se vider (dernier élément retiré) : retour à Tous
  if (showFavorites && favCount === 0) showFavorites = false;
  if (activeKind !== null && !kinds.some((k) => k.kind === activeKind)) {
    activeKind = null;
  }
  if (showFavorites) {
    // Réordonné pour l'affichage groupé (par dossier ou par catégorie, voir renderFavorites)
    const favs = await listFavorites(search);
    items = favView === "groups" ? groupFavoritesByGroup(favs) : groupFavoritesByKind(favs);
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

    li.addEventListener("click", () => copyWithFlash(index));
    li.addEventListener("mousemove", () => {
      if (selectedIndex !== index) {
        selectedIndex = index;
        updateSelection();
      }
    });

    listEl.appendChild(li);
  });
}

/// Réordonne les favoris (déjà triés par nom) groupés par catégorie,
/// les catégories les plus fournies d'abord. L'ordre du tableau plat suit
/// l'ordre d'affichage, pour que la navigation clavier reste cohérente.
function groupFavoritesByKind(favs: Item[]): Item[] {
  const byKind = new Map<string, Item[]>();
  for (const item of favs) {
    const arr = byKind.get(item.kind);
    if (arr) arr.push(item);
    else byKind.set(item.kind, [item]);
  }
  return [...byKind.entries()]
    .sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0]))
    .flatMap(([, arr]) => arr);
}

/// Réordonne les favoris (déjà triés par nom) par dossier, dans
/// l'ordre des dossiers (par nom), les favoris sans dossier en dernier.
function groupFavoritesByGroup(favs: Item[]): Item[] {
  const ordered: Item[] = [];
  for (const g of groups) {
    ordered.push(...favs.filter((f) => f.group_id === g.id));
  }
  ordered.push(...favs.filter((f) => f.group_id === null));
  return ordered;
}

/// Tag de la dimension complémentaire, affiché avant le nom du favori : dans la
/// vue « dossiers » on montre la catégorie ; dans la vue « catégories » on montre
/// le dossier (rien si le favori n'appartient à aucun dossier).
function favTag(item: Item): HTMLSpanElement | null {
  const span = document.createElement("span");
  span.className = "fav-tag";
  if (favView === "groups") {
    const meta = kindMeta(item.kind);
    span.textContent = `${meta.icon} ${meta.label}`;
    return span;
  }
  if (item.group_id === null) return null;
  const g = groups.find((x) => x.id === item.group_id);
  if (!g) return null;
  span.textContent = `📁 ${g.name}`;
  return span;
}

/// Clé et libellé de la section d'un favori selon la vue active (dossier ou
/// catégorie), pour afficher les en-têtes et savoir quand en insérer un.
function favSection(item: Item): { key: string; label: string } {
  if (favView === "groups") {
    const g = item.group_id !== null ? groups.find((x) => x.id === item.group_id) : undefined;
    return g ? { key: `g${g.id}`, label: `📁 ${g.name}` } : { key: "none", label: "Sans dossier" };
  }
  const meta = kindMeta(item.kind);
  return { key: item.kind, label: `${meta.icon} ${meta.label}` };
}

/// Rend l'onglet Favoris : sélecteur de vue (dossiers/catégories), puis en-têtes
/// de section et lignes compactes (nom + loupe). `items` est déjà en ordre
/// groupé (voir groupFavoritesByGroup / groupFavoritesByKind).
function renderFavorites(): void {
  renderFavViewToggle();
  let currentKey: string | null = null;
  items.forEach((item, index) => {
    const section = favSection(item);
    if (section.key !== currentKey) {
      currentKey = section.key;
      const header = document.createElement("li");
      header.className = "fav-header";
      header.textContent = section.label;
      listEl.appendChild(header);
    }
    listEl.appendChild(buildFavoriteRow(item, index));
  });
}

/// Change le mode de regroupement des favoris (catégorie ou dossier) et
/// rafraîchit l'affichage. Point d'entrée unique du menu ☰ et de la barre de
/// bascule de l'onglet Favoris.
function setFavView(view: FavView): void {
  if (favView === view) return;
  favView = view;
  selectedIndex = 0;
  void refresh();
}

/// Bascule la préférence de démarrage automatique avec Windows. En cas
/// d'échec (ex. plateforme sans support), la case n'est pas cochée et
/// l'utilisateur est prévenu.
async function toggleAutostart(): Promise<void> {
  const next = !autostartEnabled;
  try {
    await setAutostartEnabled(next);
    autostartEnabled = next;
  } catch (err) {
    console.error(err);
    alert("Impossible de modifier le démarrage automatique : " + String(err));
  }
}

/// Petit sélecteur en tête de l'onglet Favoris pour basculer entre un
/// regroupement par catégorie auto-détectée et par dossier créé par l'utilisateur.
function renderFavViewToggle(): void {
  const bar = document.createElement("li");
  bar.className = "fav-view-toggle";

  const make = (view: FavView, label: string): HTMLButtonElement => {
    const btn = document.createElement("button");
    btn.className = favView === view ? "fav-view-btn active" : "fav-view-btn";
    btn.textContent = label;
    btn.addEventListener("click", () => setFavView(view));
    return btn;
  };

  const caption = document.createElement("span");
  caption.className = "fav-view-caption";
  caption.textContent = "Grouper par";
  bar.append(caption, make("groups", "Dossiers"), make("kinds", "Catégories"));
  listEl.appendChild(bar);
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

  // Tag de la dimension complémentaire à la vue active : la catégorie quand on
  // regroupe par dossier, le dossier quand on regroupe par catégorie.
  const tag = favTag(item);
  if (tag) head.appendChild(tag);

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
    actions.appendChild(favAction("Copier", () => copyWithFlash(index)));
    actions.appendChild(favAction("Renommer", () => void renameFavorite(item)));
    actions.appendChild(favAction("Modifier", () => void editFavoriteContent(item)));
    actions.appendChild(favAction("Catégorie", () => void changeFavoriteKind(item)));
    actions.appendChild(favAction("Dossier", () => void changeFavoriteGroup(item)));
    actions.appendChild(favAction("Retirer", () => void togglePinFlow(item)));
    detail.appendChild(actions);

    li.appendChild(detail);
  }

  li.addEventListener("click", () => copyWithFlash(index));
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

/// Flash de confirmation sur l'élément, puis copie dans le presse-papiers.
function copyWithFlash(index: number): void {
  const item = items[index];
  if (!item) return;
  const node = listEl.querySelectorAll<HTMLLIElement>("li.item")[index];
  node?.classList.add("copying");
  window.setTimeout(() => void copyItem(item.id), 100);
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
      copyWithFlash(selectedIndex);
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

// ── Menu principal (bouton ☰) ───────────────────────────
// Un seul menu déroulant compact, ancré au bouton de la barre de recherche.

interface MenuEntry {
  label: string;
  onClick: () => void;
  danger?: boolean;
  /** Entrée cochable : `true`/`false` affiche une coche, `undefined` = pas de coche. */
  checked?: boolean;
}

/// Un nœud du menu : une entrée cliquable, un intitulé de section, ou un trait.
type MenuNode = MenuEntry | { caption: string } | "separator";

/// Ouvre le menu ☰ : un panneau flottant ancré sous le bouton. Se ferme au
/// clic dehors, sur Échap, ou après le choix d'une entrée.
function openMainMenu(): void {
  // Un menu déjà ouvert : on bascule (referme).
  if (document.querySelector(".menu-overlay")) {
    closeMainMenu();
    return;
  }

  const nodes: MenuNode[] = [
    { caption: "Affichage des favoris" },
    { label: "Par dossier", checked: favView === "groups", onClick: () => setFavView("groups") },
    { label: "Par catégorie", checked: favView === "kinds", onClick: () => setFavView("kinds") },
    "separator",
    { caption: "Dossiers" },
    { label: "Nouveau dossier…", onClick: () => void createGroupFlow() },
  ];
  if (groups.length > 0) {
    nodes.push({ label: "Gérer les dossiers…", onClick: () => void manageGroups() });
  }
  nodes.push(
    "separator",
    { caption: "Sauvegarde des favoris" },
    { label: "Exporter les favoris…", onClick: () => void exportFavoritesFlow() },
    { label: "Importer des favoris…", onClick: () => void importFavoritesFlow() },
    "separator",
    { caption: "Historique" },
    { label: "Vider l'historique…", onClick: () => void clearHistoryFlow(), danger: true },
    "separator",
    { caption: "Démarrage" },
    {
      label: "Lancer copicol au démarrage de Windows",
      checked: autostartEnabled,
      onClick: () => void toggleAutostart(),
    },
    "separator",
    { caption: "copicol" },
    { label: "Vérifier les mises à jour", onClick: () => void checkUpdatesManually() },
    { label: "À propos", onClick: () => void showAbout() },
    { label: "Quitter", onClick: () => void quitFlow(), danger: true },
  );

  const overlay = document.createElement("div");
  overlay.className = "menu-overlay";
  overlay.addEventListener("mousedown", (e) => {
    if (e.target === overlay) closeMainMenu();
  });
  overlay.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      closeMainMenu();
    }
  });

  const menu = document.createElement("div");
  menu.className = "menu";
  for (const node of nodes) {
    if (node === "separator") {
      const sep = document.createElement("div");
      sep.className = "menu-separator";
      menu.appendChild(sep);
    } else if ("caption" in node) {
      const cap = document.createElement("div");
      cap.className = "menu-caption";
      cap.textContent = node.caption;
      menu.appendChild(cap);
    } else {
      const btn = document.createElement("button");
      btn.className = node.danger ? "menu-item danger" : "menu-item";
      // Colonne de coche fixe : garde tous les libellés alignés, cochés ou non.
      const check = document.createElement("span");
      check.className = "menu-check";
      check.textContent = node.checked ? "✓" : "";
      const label = document.createElement("span");
      label.textContent = node.label;
      btn.append(check, label);
      btn.addEventListener("click", () => {
        closeMainMenu();
        node.onClick();
      });
      menu.appendChild(btn);
    }
  }

  overlay.appendChild(menu);
  document.body.appendChild(overlay);
  modalOpen = true;
  menu.querySelector<HTMLButtonElement>(".menu-item")?.focus();
}

function closeMainMenu(): void {
  document.querySelector(".menu-overlay")?.remove();
  modalOpen = false;
}

/// Crée un dossier depuis le menu (sans l'affecter à un favori en particulier).
async function createGroupFlow(): Promise<void> {
  const name = await promptName("", "Nouveau dossier");
  if (!name) return;
  await createGroup(name);
  await refresh();
}

/// Gère les dossiers existants : renommer ou supprimer. La suppression conserve
/// les favoris (ils repassent « sans dossier »).
async function manageGroups(): Promise<void> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    const box = document.createElement("div");
    box.className = "modal";

    const title = document.createElement("div");
    title.className = "modal-title";
    title.textContent = "Gérer les dossiers";

    const listWrap = document.createElement("div");
    listWrap.className = "group-manage";
    for (const g of groups) {
      const row = document.createElement("div");
      row.className = "group-manage-row";
      const name = document.createElement("span");
      name.className = "group-manage-name";
      name.textContent = `📁 ${g.name}`;
      const rename = document.createElement("button");
      rename.className = "fav-action";
      rename.textContent = "Renommer";
      rename.addEventListener("click", async () => {
        const next = await promptName(g.name, "Renommer le dossier");
        if (next) {
          await renameGroup(g.id, next);
          close();
          await refresh();
          void manageGroups();
        }
      });
      const del = document.createElement("button");
      del.className = "fav-action danger";
      del.textContent = "Supprimer";
      del.addEventListener("click", async () => {
        if (await confirmAction(`Supprimer le dossier « ${g.name} » ?`, "Supprimer")) {
          await deleteGroup(g.id);
          close();
          await refresh();
        }
      });
      row.append(name, rename, del);
      listWrap.appendChild(row);
    }

    const actions = document.createElement("div");
    actions.className = "modal-actions";
    const done = document.createElement("button");
    done.className = "modal-btn primary";
    done.textContent = "Fermer";
    done.addEventListener("click", () => close());
    actions.appendChild(done);

    const close = (): void => {
      modalOpen = false;
      overlay.remove();
      resolve();
    };
    overlay.addEventListener("mousedown", (e) => {
      if (e.target === overlay) close();
    });
    overlay.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape") {
        e.preventDefault();
        close();
      }
    });

    box.append(title, listWrap, actions);
    overlay.append(box);
    document.body.appendChild(overlay);
    modalOpen = true;
  });
}

/// Exporte tous les favoris (et leurs dossiers) vers un fichier JSON choisi.
async function exportFavoritesFlow(): Promise<void> {
  if (favCount === 0) {
    alert("Aucun favori à exporter.");
    return;
  }
  const date = new Date().toISOString().slice(0, 10);
  const path = await pickSavePath(`copicol-favoris-${date}.json`);
  if (!path) return;
  try {
    const count = await exportFavorites(path);
    alert(`${count} favori(s) exporté(s).`);
  } catch (err) {
    console.error(err);
    alert("Export impossible : " + String(err));
  }
}

/// Importe des favoris depuis un fichier JSON et fusionne (doublons ignorés).
async function importFavoritesFlow(): Promise<void> {
  const path = await pickOpenPath();
  if (!path) return;
  try {
    const { imported, skipped } = await importFavorites(path);
    await refresh();
    // Le fichier importé a pu changer des préférences (ex. démarrage auto)
    autostartEnabled = await getAutostartEnabled().catch(() => autostartEnabled);
    const parts = [`${imported} favori(s) importé(s)`];
    if (skipped > 0) parts.push(`${skipped} ignoré(s) (déjà présents)`);
    alert(parts.join(", ") + ".");
  } catch (err) {
    console.error(err);
    alert("Import impossible : " + String(err));
  }
}

/// Vide l'historique après confirmation (les favoris sont conservés).
async function clearHistoryFlow(): Promise<void> {
  if (await confirmAction("Vider tout l'historique ? Les favoris sont conservés.", "Vider")) {
    await clearHistory();
    selectedIndex = 0;
    await refresh();
  }
}

/// Quitte l'application après confirmation (évite un clic accidentel).
async function quitFlow(): Promise<void> {
  if (await confirmAction("Quitter copicol ?", "Quitter")) {
    await quitApp();
  }
}

/// Vérification de mise à jour déclenchée manuellement : informe l'utilisateur
/// du résultat, y compris quand l'application est déjà à jour.
async function checkUpdatesManually(): Promise<void> {
  const result = await checkUpdate();
  if (!result.available) {
    alert(result.offline ? "Impossible de vérifier les mises à jour (hors ligne ?)." : "copicol est à jour.");
    return;
  }
  if (await confirmAction(`Mise à jour ${result.version} disponible. L'installer et redémarrer ?`, "Mettre à jour")) {
    await installPendingUpdate();
  }
}

/// Fenêtre « À propos » : nom, version, auteur et lien du dépôt.
async function showAbout(): Promise<void> {
  const version = await appVersion().catch(() => "");
  const overlay = document.createElement("div");
  overlay.className = "modal-overlay";
  const box = document.createElement("div");
  box.className = "modal about-modal";

  const logo = document.createElement("div");
  logo.className = "about-logo";
  logo.textContent = "📋";

  const name = document.createElement("div");
  name.className = "about-name";
  name.textContent = "copicol";

  const ver = document.createElement("div");
  ver.className = "about-version";
  ver.textContent = version ? `Version ${version}` : "";

  const desc = document.createElement("div");
  desc.className = "about-desc";
  desc.textContent = "Un gestionnaire de presse-papiers simple et rapide.";

  const author = document.createElement("div");
  author.className = "about-author";
  author.textContent = "© Clément Toledano";

  const link = document.createElement("a");
  link.className = "about-link";
  link.href = REPO_URL;
  link.textContent = REPO_URL;
  link.title = "Ouvrir le dépôt dans le navigateur";
  link.addEventListener("click", (e) => {
    e.preventDefault();
    void (async () => {
      try {
        await openRepoUrl();
        // La fenêtre reste toujours au premier plan (alwaysOnTop) : on la
        // cache pour que le navigateur nouvellement ouvert soit visible.
        await hideWindow();
      } catch (err) {
        console.error(err);
        alert("Impossible d'ouvrir le navigateur : " + String(err));
      }
    })();
  });

  const actions = document.createElement("div");
  actions.className = "modal-actions";
  const done = document.createElement("button");
  done.className = "modal-btn primary";
  done.textContent = "Fermer";

  const close = (): void => {
    modalOpen = false;
    overlay.remove();
  };
  done.addEventListener("click", close);
  overlay.addEventListener("mousedown", (e) => {
    if (e.target === overlay) close();
  });
  overlay.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Escape" || e.key === "Enter") {
      e.preventDefault();
      close();
    }
  });
  actions.appendChild(done);

  box.append(logo, name, ver, desc, author, link, actions);
  overlay.append(box);
  document.body.appendChild(overlay);
  modalOpen = true;
  done.focus();
}

async function init(): Promise<void> {
  setupResizeHandles();
  await refresh();
  autostartEnabled = await getAutostartEnabled().catch(() => true);

  menuBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    openMainMenu();
  });
  await onShowAbout(() => {
    closeMainMenu();
    void showAbout();
  });

  await onClipboardChanged(() => void refresh());
  await onWindowShown(() => {
    closeModals(); // au cas où la fenêtre a été cachée pendant une saisie
    expandedFavIds.clear();
    searchInput.value = "";
    selectedIndex = 0;
    searchInput.focus();
    void refresh();
    void promptPendingUpdate();
  });

  searchInput.focus();

  // Vérification silencieuse au démarrage : la fenêtre est encore cachée à ce
  // stade (comportement lanceur), donc une modale ici resterait invisible.
  // On signale plutôt via notification système, et l'installation sera
  // proposée à la prochaine ouverture de la fenêtre (voir onWindowShown).
  void checkUpdateAtStartup();
}

/// Vérifie une mise à jour au démarrage et la signale par notification système
/// (visible même fenêtre cachée/en tray) si elle n'a pas déjà pu être annoncée.
async function checkUpdateAtStartup(): Promise<void> {
  const result = await checkUpdate();
  if (!result.available) return;
  pendingStartupUpdate = { version: result.version, notes: result.notes };
  if (await ensureNotificationPermission().catch(() => false)) {
    await notifyUpdateAvailable(result.version).catch((err) => console.error(err));
  }
}

/// Propose l'installation d'une mise à jour détectée au démarrage, la première
/// fois que la fenêtre est rouverte après coup.
async function promptPendingUpdate(): Promise<void> {
  if (!pendingStartupUpdate) return;
  const update = pendingStartupUpdate;
  pendingStartupUpdate = null;
  if (await confirmAction(`Mise à jour ${update.version} disponible. L'installer et redémarrer ?`, "Mettre à jour")) {
    await installPendingUpdate();
  }
}

void init();
