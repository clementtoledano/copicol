/// Éditeur d'annotation d'image : un <canvas> sur l'image, des formes
/// (ligne, flèche, rectangle, ellipse, étape numérotée), un crayon, du texte et
/// un outil de sélection. Les annotations restent des objets modifiables
/// jusqu'à l'enregistrement, qui les aplatit à la résolution d'origine.
/// Toutes les coordonnées du modèle sont en pixels de l'image d'origine.

import { icon } from "./icons";

type Pt = [number, number];
type SegmentKind = "line" | "arrow" | "rect" | "ellipse";
type Shape =
  | { kind: "pen"; points: Pt[]; color: string }
  | { kind: SegmentKind; x1: number; y1: number; x2: number; y2: number; color: string }
  | { kind: "step"; x: number; y: number; n: number; color: string }
  | { kind: "text"; x: number; y: number; text: string; color: string };
type Tool = "select" | "pen" | SegmentKind | "step" | "text";

/// Épaisseur de trait et taille de texte, proportionnelles à l'image.
interface Style {
  lw: number;
  font: number;
}

const COLORS = ["#e5484d", "#f5c451", "#30a46c", "#5865f2", "#111111"];
const COLOR_NAMES = ["Rouge", "Jaune", "Vert", "Bleu", "Noir"];

const SHAPE_TOOLS: Array<{ tool: SegmentKind | "step"; label: string }> = [
  { tool: "line", label: "Ligne" },
  { tool: "arrow", label: "Flèche" },
  { tool: "rect", label: "Rectangle" },
  { tool: "ellipse", label: "Ellipse" },
  { tool: "step", label: "Étape" },
];

/// Tracés Lucide (licence ISC) des outils, dans le même style que icons.ts.
const GLYPHS = {
  select:
    '<path d="M4.037 4.688a.495.495 0 0 1 .651-.651l16 6.5a.5.5 0 0 1-.063.947l-6.124 1.58a2 2 0 0 0-1.438 1.435l-1.579 6.126a.5.5 0 0 1-.947.063z"/>',
  pen: '<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/><path d="m15 5 4 4"/>',
  line: '<path d="M5 19 19 5"/>',
  arrow: '<path d="M7 7h10v10"/><path d="M7 17 17 7"/>',
  rect: '<rect width="18" height="18" x="3" y="3" rx="2"/>',
  ellipse: '<circle cx="12" cy="12" r="10"/>',
  step: '<circle cx="12" cy="12" r="10"/><path d="m10 9 2-1v8"/>',
  text: '<polyline points="4 7 4 4 20 4 20 7"/><line x1="9" x2="15" y1="20" y2="20"/><line x1="12" x2="12" y1="4" y2="20"/>',
  undo: '<path d="M9 14 4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11"/>',
  redo: '<path d="m15 14 5-5-5-5"/><path d="M20 9H9.5A5.5 5.5 0 0 0 4 14.5A5.5 5.5 0 0 0 9.5 20H13"/>',
} as const;
type GlyphName = keyof typeof GLYPHS;

function glyph(name: GlyphName, size = 16): SVGSVGElement {
  const wrap = document.createElement("div");
  wrap.innerHTML =
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="${size}" height="${size}" ` +
    `fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" ` +
    `aria-hidden="true">${GLYPHS[name]}</svg>`;
  return wrap.firstElementChild as SVGSVGElement;
}

function styleFor(w: number, h: number): Style {
  const lw = Math.max(3, Math.round(Math.min(w, h) / 250));
  return { lw, font: Math.max(16, lw * 6) };
}

// ── Dessin ───────────────────────────────────────────────────────────

function drawShape(ctx: CanvasRenderingContext2D, s: Shape, st: Style): void {
  ctx.strokeStyle = s.color;
  ctx.fillStyle = s.color;
  ctx.lineWidth = st.lw;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  switch (s.kind) {
    case "pen": {
      ctx.beginPath();
      s.points.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      if (s.points.length === 1) ctx.lineTo(s.points[0][0], s.points[0][1]);
      ctx.stroke();
      break;
    }
    case "line":
    case "arrow": {
      ctx.beginPath();
      ctx.moveTo(s.x1, s.y1);
      ctx.lineTo(s.x2, s.y2);
      ctx.stroke();
      if (s.kind === "arrow") {
        const a = Math.atan2(s.y2 - s.y1, s.x2 - s.x1);
        const head = st.lw * 4.5;
        ctx.beginPath();
        ctx.moveTo(s.x2, s.y2);
        ctx.lineTo(s.x2 - head * Math.cos(a - Math.PI / 6), s.y2 - head * Math.sin(a - Math.PI / 6));
        ctx.lineTo(s.x2 - head * Math.cos(a + Math.PI / 6), s.y2 - head * Math.sin(a + Math.PI / 6));
        ctx.closePath();
        ctx.fill();
      }
      break;
    }
    case "rect":
      ctx.strokeRect(Math.min(s.x1, s.x2), Math.min(s.y1, s.y2), Math.abs(s.x2 - s.x1), Math.abs(s.y2 - s.y1));
      break;
    case "ellipse":
      ctx.beginPath();
      ctx.ellipse(
        (s.x1 + s.x2) / 2,
        (s.y1 + s.y2) / 2,
        Math.max(Math.abs(s.x2 - s.x1) / 2, 0.5),
        Math.max(Math.abs(s.y2 - s.y1) / 2, 0.5),
        0,
        0,
        Math.PI * 2,
      );
      ctx.stroke();
      break;
    case "step": {
      const r = st.lw * 4.5;
      ctx.beginPath();
      ctx.arc(s.x, s.y, r, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = s.color === "#f5c451" ? "#111111" : "#ffffff";
      ctx.font = `700 ${r * 1.25}px "Segoe UI", system-ui, sans-serif`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(String(s.n), s.x, s.y + r * 0.05);
      ctx.textAlign = "start";
      break;
    }
    case "text":
      ctx.font = `600 ${st.font}px "Segoe UI", system-ui, sans-serif`;
      ctx.textAlign = "start";
      ctx.textBaseline = "top";
      ctx.fillText(s.text, s.x, s.y);
      break;
  }
}

/// Boîte englobante d'une forme (pour la sélection et son contour).
function bounds(ctx: CanvasRenderingContext2D, s: Shape, st: Style): [number, number, number, number] {
  switch (s.kind) {
    case "pen": {
      const xs = s.points.map((p) => p[0]);
      const ys = s.points.map((p) => p[1]);
      return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
    }
    case "step": {
      const r = st.lw * 4.5;
      return [s.x - r, s.y - r, s.x + r, s.y + r];
    }
    case "text": {
      ctx.font = `600 ${st.font}px "Segoe UI", system-ui, sans-serif`;
      return [s.x, s.y, s.x + ctx.measureText(s.text).width, s.y + st.font * 1.2];
    }
    default:
      return [Math.min(s.x1, s.x2), Math.min(s.y1, s.y2), Math.max(s.x1, s.x2), Math.max(s.y1, s.y2)];
  }
}

function distToSegment(p: Pt, a: Pt, b: Pt): number {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2));
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy));
}

function hit(ctx: CanvasRenderingContext2D, s: Shape, p: Pt, st: Style, tol: number): boolean {
  switch (s.kind) {
    case "pen": {
      if (s.points.length === 1) return Math.hypot(p[0] - s.points[0][0], p[1] - s.points[0][1]) <= tol + st.lw;
      for (let i = 1; i < s.points.length; i++) {
        if (distToSegment(p, s.points[i - 1], s.points[i]) <= tol + st.lw) return true;
      }
      return false;
    }
    case "line":
    case "arrow":
      return distToSegment(p, [s.x1, s.y1], [s.x2, s.y2]) <= tol + st.lw;
    default: {
      const [x1, y1, x2, y2] = bounds(ctx, s, st);
      return p[0] >= x1 - tol && p[0] <= x2 + tol && p[1] >= y1 - tol && p[1] <= y2 + tol;
    }
  }
}

function moved(s: Shape, dx: number, dy: number): Shape {
  switch (s.kind) {
    case "pen":
      return { ...s, points: s.points.map(([x, y]): Pt => [x + dx, y + dy]) };
    case "step":
    case "text":
      return { ...s, x: s.x + dx, y: s.y + dy };
    default:
      return { ...s, x1: s.x1 + dx, y1: s.y1 + dy, x2: s.x2 + dx, y2: s.y2 + dy };
  }
}

// ── Éditeur ──────────────────────────────────────────────────────────

/// Ouvre l'éditeur sur un PNG. Résout avec le PNG aplati (résolution
/// d'origine) à l'enregistrement, ou `null` si l'utilisateur annule.
export async function openAnnotator(png: ArrayBuffer): Promise<Uint8Array | null> {
  const bmp = await createImageBitmap(new Blob([png], { type: "image/png" }));
  const W = bmp.width;
  const H = bmp.height;
  const st = styleFor(W, H);

  return new Promise((resolve) => {
    let shapes: Shape[] = [];
    const undoStack: Shape[][] = [];
    const redoStack: Shape[][] = [];
    let tool: Tool = "arrow";
    let lastShapeTool: SegmentKind | "step" = "arrow";
    let color = COLORS[0];
    let draft: Shape | null = null;
    let selected = -1;
    let scale = 1;
    let textInput: { el: HTMLInputElement; x: number; y: number } | null = null;

    // ── DOM
    const overlay = document.createElement("div");
    overlay.className = "annot-overlay";
    overlay.tabIndex = -1;

    const head = document.createElement("div");
    head.className = "annot-head";
    const title = document.createElement("span");
    title.className = "annot-title";
    title.textContent = "Annoter l'image";
    const undoBtn = document.createElement("button");
    undoBtn.className = "annot-btn";
    undoBtn.title = "Annuler (Ctrl+Z)";
    undoBtn.appendChild(glyph("undo"));
    const redoBtn = document.createElement("button");
    redoBtn.className = "annot-btn";
    redoBtn.title = "Rétablir (Ctrl+Y)";
    redoBtn.appendChild(glyph("redo"));
    const cancelBtn = document.createElement("button");
    cancelBtn.className = "modal-btn";
    cancelBtn.textContent = "Annuler";
    const saveBtn = document.createElement("button");
    saveBtn.className = "modal-btn primary";
    saveBtn.textContent = "Enregistrer";
    saveBtn.title = "Enregistrer et copier (Ctrl+Entrée)";
    head.append(title, undoBtn, redoBtn, cancelBtn, saveBtn);

    const stage = document.createElement("div");
    stage.className = "annot-stage";
    const canvas = document.createElement("canvas");
    canvas.className = "annot-canvas";
    stage.appendChild(canvas);
    const ctx = canvas.getContext("2d") as CanvasRenderingContext2D;

    // Barre d'outils flottante
    const bar = document.createElement("div");
    bar.className = "annot-toolbar";
    const toolBtns = new Map<Tool, HTMLButtonElement>();
    const mkTool = (t: Tool, g: GlyphName, label: string): HTMLButtonElement => {
      const b = document.createElement("button");
      b.className = "annot-tool";
      b.title = label;
      b.appendChild(glyph(g));
      b.addEventListener("click", () => setTool(t));
      toolBtns.set(t, b);
      return b;
    };
    const shapeBtn = document.createElement("button");
    shapeBtn.className = "annot-tool annot-shape";
    const shapeMenu = document.createElement("div");
    shapeMenu.className = "annot-menu";
    shapeMenu.hidden = true;
    for (const { tool: t, label } of SHAPE_TOOLS) {
      const item = document.createElement("button");
      item.className = "annot-menu-item";
      item.dataset.tool = t;
      item.append(glyph(t), document.createTextNode(label));
      item.addEventListener("click", () => {
        lastShapeTool = t;
        shapeMenu.hidden = true;
        setTool(t);
      });
      shapeMenu.appendChild(item);
    }
    shapeBtn.addEventListener("click", () => {
      const wasShape = SHAPE_TOOLS.some((s) => s.tool === tool);
      setTool(lastShapeTool);
      shapeMenu.hidden = wasShape ? !shapeMenu.hidden : false;
    });

    const sep = (): HTMLSpanElement => {
      const s = document.createElement("span");
      s.className = "annot-sep";
      return s;
    };
    const swatches: HTMLButtonElement[] = COLORS.map((c, i) => {
      const b = document.createElement("button");
      b.className = "annot-swatch";
      b.style.background = c;
      b.title = COLOR_NAMES[i];
      b.addEventListener("click", () => {
        color = c;
        // Recolorer la forme sélectionnée
        if (selected >= 0) {
          commit(shapes.map((s, idx) => (idx === selected ? { ...s, color: c } : s)));
        }
        refresh();
      });
      return b;
    });
    const closeBtn = document.createElement("button");
    closeBtn.className = "annot-tool";
    closeBtn.title = "Fermer sans enregistrer (Échap)";
    closeBtn.appendChild(icon("x", 16));
    closeBtn.addEventListener("click", () => finish(null));

    bar.append(
      mkTool("select", "select", "Sélection"),
      mkTool("pen", "pen", "Crayon"),
      shapeBtn,
      mkTool("text", "text", "Texte"),
      sep(),
      ...swatches,
      sep(),
      closeBtn,
    );
    stage.append(shapeMenu, bar);
    overlay.append(head, stage);

    // ── État
    const clone = <T,>(v: T): T => structuredClone(v);

    function commit(next: Shape[]): void {
      undoStack.push(clone(shapes));
      redoStack.length = 0;
      shapes = next;
      refresh();
    }

    function undo(): void {
      const prev = undoStack.pop();
      if (!prev) return;
      redoStack.push(clone(shapes));
      shapes = prev;
      selected = -1;
      refresh();
    }

    function redo(): void {
      const next = redoStack.pop();
      if (!next) return;
      undoStack.push(clone(shapes));
      shapes = next;
      selected = -1;
      refresh();
    }

    function setTool(t: Tool): void {
      commitText();
      tool = t;
      if (t !== "select") selected = -1;
      if (SHAPE_TOOLS.some((s) => s.tool === t)) lastShapeTool = t as SegmentKind | "step";
      if (t === "select" || t === "pen" || t === "text") shapeMenu.hidden = true;
      refresh();
    }

    function refresh(): void {
      for (const [t, b] of toolBtns) b.classList.toggle("on", t === tool);
      const isShape = SHAPE_TOOLS.some((s) => s.tool === tool);
      shapeBtn.classList.toggle("on", isShape);
      shapeBtn.replaceChildren(glyph(lastShapeTool), icon("chevron-down", 10));
      shapeBtn.title = "Formes";
      shapeMenu.querySelectorAll<HTMLButtonElement>(".annot-menu-item").forEach((b) => {
        b.classList.toggle("on", b.dataset.tool === tool);
      });
      swatches.forEach((b, i) => b.classList.toggle("on", COLORS[i] === color));
      undoBtn.disabled = undoStack.length === 0;
      redoBtn.disabled = redoStack.length === 0;
      canvas.style.cursor = tool === "select" ? "default" : tool === "text" ? "text" : "crosshair";
      paint();
    }

    function layout(): void {
      const availW = Math.max(stage.clientWidth - 24, 50);
      const availH = Math.max(stage.clientHeight - 70, 50);
      scale = Math.min(availW / W, availH / H, 1);
      const dpr = window.devicePixelRatio || 1;
      canvas.style.width = `${W * scale}px`;
      canvas.style.height = `${H * scale}px`;
      canvas.width = Math.max(1, Math.round(W * scale * dpr));
      canvas.height = Math.max(1, Math.round(H * scale * dpr));
      paint();
    }

    function paint(): void {
      const dpr = window.devicePixelRatio || 1;
      ctx.setTransform(dpr * scale, 0, 0, dpr * scale, 0, 0);
      ctx.clearRect(0, 0, W, H);
      ctx.drawImage(bmp, 0, 0);
      for (const s of shapes) drawShape(ctx, s, st);
      if (draft) drawShape(ctx, draft, st);
      if (selected >= 0 && shapes[selected]) {
        const [x1, y1, x2, y2] = bounds(ctx, shapes[selected], st);
        const pad = 5 / scale;
        ctx.save();
        ctx.strokeStyle = "#5865f2";
        ctx.lineWidth = 1.5 / scale;
        ctx.setLineDash([5 / scale, 4 / scale]);
        ctx.strokeRect(x1 - pad, y1 - pad, x2 - x1 + pad * 2, y2 - y1 + pad * 2);
        ctx.restore();
      }
    }

    function toImage(e: PointerEvent): Pt {
      const r = canvas.getBoundingClientRect();
      return [(e.clientX - r.left) / scale, (e.clientY - r.top) / scale];
    }

    function nextStepNumber(): number {
      return shapes.reduce((m, s) => (s.kind === "step" ? Math.max(m, s.n) : m), 0) + 1;
    }

    // ── Texte
    function startText(x: number, y: number): void {
      commitText();
      const el = document.createElement("input");
      el.className = "annot-text-input";
      el.type = "text";
      el.style.left = `${canvas.offsetLeft + x * scale}px`;
      el.style.top = `${canvas.offsetTop + y * scale}px`;
      el.style.color = color;
      el.style.fontSize = `${st.font * scale}px`;
      el.addEventListener("keydown", (e) => {
        e.stopPropagation();
        if (e.key === "Enter") {
          e.preventDefault();
          commitText();
        } else if (e.key === "Escape") {
          e.preventDefault();
          textInput = null;
          el.remove();
          overlay.focus();
        }
      });
      el.addEventListener("blur", () => commitText());
      stage.appendChild(el);
      textInput = { el, x, y };
      el.focus();
    }

    function commitText(): void {
      if (!textInput) return;
      const { el, x, y } = textInput;
      textInput = null;
      const value = el.value.trim();
      el.remove();
      if (value) commit([...shapes, { kind: "text", x, y, text: value, color }]);
    }

    // ── Souris
    let dragFrom: Pt | null = null;
    let dragOrig: Shape | null = null;
    let dragSnapshot: Shape[] | null = null;
    let dragMoved = false;

    canvas.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      overlay.focus();
      shapeMenu.hidden = true;
      const p = toImage(e);
      canvas.setPointerCapture(e.pointerId);
      if (tool === "select") {
        selected = -1;
        for (let i = shapes.length - 1; i >= 0; i--) {
          if (hit(ctx, shapes[i], p, st, 6 / scale)) {
            selected = i;
            break;
          }
        }
        if (selected >= 0) {
          dragFrom = p;
          dragOrig = clone(shapes[selected]);
          dragSnapshot = clone(shapes);
          dragMoved = false;
        }
        paint();
      } else if (tool === "pen") {
        draft = { kind: "pen", points: [p], color };
      } else if (tool === "step") {
        commit([...shapes, { kind: "step", x: p[0], y: p[1], n: nextStepNumber(), color }]);
      } else if (tool === "text") {
        // Le focus est pris au relâchement : sinon le clic de pose le ferait perdre aussitôt.
        dragFrom = p;
      } else {
        draft = { kind: tool, x1: p[0], y1: p[1], x2: p[0], y2: p[1], color };
      }
    });

    canvas.addEventListener("pointermove", (e) => {
      const p = toImage(e);
      if (tool === "select" && dragFrom && dragOrig && selected >= 0) {
        dragMoved = true;
        shapes[selected] = moved(dragOrig, p[0] - dragFrom[0], p[1] - dragFrom[1]);
        paint();
      } else if (draft?.kind === "pen") {
        draft.points.push(p);
        paint();
      } else if (draft && "x2" in draft) {
        draft.x2 = p[0];
        draft.y2 = p[1];
        paint();
      }
    });

    canvas.addEventListener("pointerup", (e) => {
      const p = toImage(e);
      if (tool === "select" && dragFrom) {
        if (dragMoved && dragSnapshot) {
          undoStack.push(dragSnapshot);
          redoStack.length = 0;
        }
        dragFrom = null;
        dragOrig = null;
        dragSnapshot = null;
        refresh();
      } else if (tool === "text" && dragFrom) {
        dragFrom = null;
        startText(p[0], p[1]);
      } else if (draft) {
        const d = draft;
        draft = null;
        const real = d.kind === "pen" || ("x2" in d && Math.hypot(d.x2 - d.x1, d.y2 - d.y1) > 3 / scale);
        if (real) commit([...shapes, d]);
        else paint();
      }
    });

    // ── Clavier
    overlay.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.ctrlKey && e.key.toLowerCase() === "z" && !e.shiftKey) {
        e.preventDefault();
        undo();
      } else if (e.ctrlKey && (e.key.toLowerCase() === "y" || (e.key.toLowerCase() === "z" && e.shiftKey))) {
        e.preventDefault();
        redo();
      } else if (e.ctrlKey && e.key === "Enter") {
        e.preventDefault();
        void save();
      } else if ((e.key === "Delete" || e.key === "Backspace") && selected >= 0) {
        e.preventDefault();
        const idx = selected;
        selected = -1;
        commit(shapes.filter((_, i) => i !== idx));
      } else if (e.key === "Escape") {
        e.preventDefault();
        if (!shapeMenu.hidden) shapeMenu.hidden = true;
        else finish(null);
      }
    });

    undoBtn.addEventListener("click", undo);
    redoBtn.addEventListener("click", redo);
    cancelBtn.addEventListener("click", () => finish(null));
    saveBtn.addEventListener("click", () => void save());

    // ── Sortie
    const resizeObserver = new ResizeObserver(() => layout());

    function finish(result: Uint8Array | null): void {
      resizeObserver.disconnect();
      textInput = null;
      overlay.remove();
      bmp.close();
      resolve(result);
    }

    async function save(): Promise<void> {
      commitText();
      const out = document.createElement("canvas");
      out.width = W;
      out.height = H;
      const octx = out.getContext("2d") as CanvasRenderingContext2D;
      octx.drawImage(bmp, 0, 0);
      for (const s of shapes) drawShape(octx, s, st);
      const blob = await new Promise<Blob | null>((r) => out.toBlob(r, "image/png"));
      if (!blob) {
        finish(null);
        return;
      }
      finish(new Uint8Array(await blob.arrayBuffer()));
    }

    document.body.appendChild(overlay);
    resizeObserver.observe(stage);
    refresh();
    layout();
    overlay.focus();
  });
}
