// Dev-only overlay: Edit-mode toggle, hover/lock outline and the right-hand panel.
// Served by the Vite dev server only (see plugin.ts); never part of a production build.

const BASE = "/__dev-editor";
const token = document.querySelector<HTMLMetaElement>('meta[name="dev-editor-token"]')?.content ?? "";

interface Segment { index: number; kind: "static" | "dynamic"; text: string }
interface Info {
  tag: string;
  segments: Segment[];
  hasElementChildren: boolean;
  classNameEditable: boolean;
  colors: Partial<Record<Prop, string>>;
  theme: Record<string, string>;
}
type Prop = "text" | "bg" | "border";
type Source = { file: string; line: number; col: number; label: string };

async function api<T>(action: string, body?: unknown): Promise<T> {
  const res = await fetch(`${BASE}/${action}`, {
    method: body === undefined ? "GET" : "POST",
    headers: { "x-dev-editor-token": token, "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = (await res.json()) as T & { error?: string };
  if (!res.ok) throw new Error(json.error ?? `HTTP ${res.status}`);
  return json;
}

function parseSrc(value: string): Source | null {
  const m = /^(.*):(\d+):(\d+)$/.exec(value);
  return m ? { file: m[1]!, line: Number(m[2]), col: Number(m[3]), label: value } : null;
}

/** Sources for a DOM node: its own data-src plus call sites of components that render it. */
function sourcesOf(el: Element): Source[] {
  const out: Source[] = [];
  const own = el.getAttribute("data-src");
  if (own) out.push(parseSrc(own)!);
  const key = Object.keys(el).find((k) => k.startsWith("__reactFiber$"));
  let fiber = key ? (el as unknown as Record<string, any>)[key] : null; // eslint-disable-line @typescript-eslint/no-explicit-any
  fiber = fiber?.return;
  while (fiber && fiber.tag !== 5 && fiber.tag !== 3) {
    const src = fiber.memoizedProps?.["data-src"];
    if (typeof src === "string") {
      const p = parseSrc(src);
      if (p && !out.some((o) => o.label === src)) out.push(p);
    }
    fiber = fiber.return;
  }
  return out;
}

const OUTLINE = "2px solid #38bdf8";

class Editor {
  on = false;
  hovered: HTMLElement | null = null;
  locked: HTMLElement | null = null;
  host = document.createElement("div");
  root = this.host.attachShadow({ mode: "open" });
  panel = document.createElement("div");
  toggle = document.createElement("button");
  src: Source | null = null;
  info: Info | null = null;
  // pending (unsaved) edits
  textEdits = new Map<number, string>();
  colorEdits = new Map<Prop, string>();
  applied: Array<() => Promise<unknown>> = []; // inverse operations for saved edits
  previewUndo: Array<() => void> = [];
  status = "";
  tokenWide = false;

  constructor() {
    this.host.setAttribute("data-dev-editor", "");
    this.host.style.cssText = "position:fixed;z-index:2147483647;inset:0;pointer-events:none";
    const style = document.createElement("style");
    style.textContent = `
      *{box-sizing:border-box;font:12px/1.4 system-ui,sans-serif}
      button.t{pointer-events:auto;position:fixed;right:16px;bottom:16px;padding:8px 12px;border-radius:999px;border:1px solid #38bdf8;background:#0f172a;color:#e2e8f0;cursor:pointer}
      button.t.on{background:#0369a1}
      .p{pointer-events:auto;position:fixed;top:0;right:0;bottom:0;width:320px;overflow:auto;background:#0b1220;color:#e2e8f0;border-left:1px solid #334155;padding:12px;display:none}
      .p.show{display:block} h3{margin:0 0 8px;font-size:13px} label{display:block;margin:10px 0 4px;color:#94a3b8}
      input[type=text]{width:100%;padding:6px;background:#020617;color:#e2e8f0;border:1px solid #334155;border-radius:4px}
      .sw{display:flex;flex-wrap:wrap;gap:3px}.sw i{width:16px;height:16px;border:1px solid #475569;border-radius:3px;cursor:pointer}
      .sw i.sel{outline:2px solid #38bdf8}
      .bc button{background:none;border:0;color:#7dd3fc;cursor:pointer;padding:0}
      .warn{color:#fbbf24;margin:6px 0}.err{color:#fb7185}.ok{color:#86efac}
      .row{display:flex;gap:6px;margin-top:12px}.row button{flex:1;padding:6px;cursor:pointer}
      .row button:disabled{opacity:.4;cursor:not-allowed}
      code{color:#a5b4fc}`;
    this.toggle.className = "t";
    this.toggle.textContent = "Edit mode: off";
    this.toggle.addEventListener("click", () => this.setOn(!this.on));
    this.panel.className = "p";
    this.root.append(style, this.toggle, this.panel);
    document.body.append(this.host);
    for (const type of ["click", "dblclick", "mousedown", "mouseup", "pointerdown", "pointerup", "submit", "contextmenu", "auxclick"]) {
      window.addEventListener(type, (e) => this.block(e), true);
    }
    window.addEventListener("keydown", (e) => this.key(e), true);
    window.addEventListener("keyup", (e) => this.block(e, true), true);
    window.addEventListener("keypress", (e) => this.block(e, true), true);
    window.addEventListener("mouseover", (e) => this.hover(e), true);
    // HMR applied the saved change: the real DOM now carries it, drop previews.
    (import.meta as unknown as { hot?: { on(e: string, cb: () => void): void } }).hot?.on("vite:afterUpdate", () => this.dropPreview());
  }

  inUi(e: Event) {
    return e.composedPath().includes(this.host);
  }

  block(e: Event, keysOnly = false) {
    if (!this.on || this.inUi(e)) return;
    if (keysOnly && !(e as KeyboardEvent).key) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (e.type === "click") {
      const target = this.pick(e);
      if (target) void this.select(target);
    }
  }

  key(e: KeyboardEvent) {
    if (!this.on || this.inUi(e)) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (e.key === "Escape") this.setOn(false);
  }

  pick(e: Event): HTMLElement | null {
    const t = (e.target as Element | null)?.closest?.("[data-src]") ?? null;
    return t as HTMLElement | null;
  }

  hover(e: MouseEvent) {
    if (!this.on || this.inUi(e)) return;
    const t = this.pick(e);
    if (t === this.hovered) return;
    if (this.hovered && this.hovered !== this.locked) this.hovered.style.removeProperty("outline");
    this.hovered = t;
    if (t && t !== this.locked) t.style.outline = "2px dashed #38bdf8";
  }

  setOn(v: boolean) {
    this.on = v;
    this.toggle.textContent = `Edit mode: ${v ? "on" : "off"}`;
    this.toggle.classList.toggle("on", v);
    if (!v) {
      this.dropPreview();
      this.hovered?.style.removeProperty("outline");
      this.locked?.style.removeProperty("outline");
      this.locked = this.hovered = null;
      this.panel.classList.remove("show");
    }
  }

  dropPreview() {
    for (const undo of this.previewUndo.splice(0)) undo();
    this.textEdits.clear();
    this.colorEdits.clear();
    if (this.locked) this.render();
  }

  async select(el: HTMLElement, srcIndex = 0) {
    this.dropPreview();
    this.locked?.style.removeProperty("outline");
    this.locked = el;
    el.style.outline = OUTLINE;
    this.status = "";
    const sources = sourcesOf(el);
    this.src = sources[srcIndex] ?? sources[0] ?? null;
    this.panel.classList.add("show");
    if (!this.src) return this.render();
    try {
      this.info = await api<Info>("describe", this.src);
    } catch (e) {
      this.info = null;
      this.status = `err:${(e as Error).message}`;
    }
    this.render();
  }

  textNodeFor(seg: Segment): Text | null {
    const el = this.locked!;
    const norm = (s: string) => s.trim().replace(/\s+/g, " ");
    for (const n of Array.from(el.childNodes)) if (n.nodeType === 3 && norm(n.nodeValue ?? "") === seg.text) return n as Text;
    return null;
  }

  previewText(seg: Segment, value: string) {
    this.textEdits.set(seg.index, value);
    const node = this.textNodeFor(seg) ?? (this.previewTexts.get(seg.index) ?? null);
    if (!node) return;
    if (!this.previewTexts.has(seg.index)) {
      const orig = node.nodeValue ?? "";
      this.previewTexts.set(seg.index, node);
      this.previewUndo.push(() => {
        node.nodeValue = orig;
        this.previewTexts.delete(seg.index);
      });
    }
    // Only the text node changes (not textContent) so React keeps owning the DOM.
    const orig = node.nodeValue ?? "";
    node.nodeValue = orig.replace(orig.trim(), value);
  }
  previewTexts = new Map<number, Text>();

  previewColor(prop: Prop, value: string, hex: string) {
    this.colorEdits.set(prop, value);
    const css = prop === "text" ? "color" : prop === "bg" ? "background-color" : "border-color";
    const el = this.locked!;
    const had = el.style.getPropertyValue(css);
    if (!this.previewUndo.length || !el.dataset.devEditorPrev?.includes(css)) {
      el.dataset.devEditorPrev = (el.dataset.devEditorPrev ?? "") + css + ",";
      this.previewUndo.push(() => {
        if (had) el.style.setProperty(css, had);
        else el.style.removeProperty(css);
        delete el.dataset.devEditorPrev;
      });
    }
    el.style.setProperty(css, hex, "important");
  }

  breadcrumb(): Array<{ el: HTMLElement; label: string }> {
    const out: Array<{ el: HTMLElement; label: string }> = [];
    for (let e: Element | null = this.locked; e; e = e.parentElement) {
      if (e.hasAttribute("data-src")) out.unshift({ el: e as HTMLElement, label: e.tagName.toLowerCase() });
    }
    return out;
  }

  get dirty() {
    return this.textEdits.size > 0 || this.colorEdits.size > 0;
  }

  async save() {
    if (!this.src || !this.info) return;
    const { file, line, col } = this.src;
    const info = this.info;
    try {
      for (const [index, text] of this.textEdits) {
        const seg = info.segments.find((s) => s.index === index)!;
        await api("save", { file, line, col, kind: "text", segment: index, text, old: seg.text });
        const prevText = seg.text;
        this.applied.push(() => api("save", { file, line, col, kind: "text", segment: index, text: prevText }));
        seg.text = text;
      }
      for (const [prop, value] of this.colorEdits) {
        const from = info.colors[prop];
        const hexOf = (v: string) => info.theme[v] ?? (/^\[(#[0-9a-fA-F]{6})\]$/.exec(v)?.[1] ?? "");
        if (this.tokenWide && from && info.theme[from]) {
          const prev = info.theme[from]!;
          await api("save", { kind: "token", name: from, hex: hexOf(value) });
          this.applied.push(() => api("save", { kind: "token", name: from, hex: prev }));
        } else {
          await api("save", { file, line, col, kind: "color", prop, value, from });
          this.applied.push(() => api("save", { file, line, col, kind: "color", prop, value: from ?? "", from: value }));
        }
        info.colors[prop] = value;
      }
      this.status = "ok:Saved. Waiting for hot reload…";
      this.textEdits.clear();
      this.colorEdits.clear();
      this.previewTexts.clear();
      // Previews are dropped by vite:afterUpdate; fall back in case nothing re-rendered.
      setTimeout(() => this.dropPreview(), 2500);
    } catch (e) {
      this.status = `err:${(e as Error).message}`;
    }
    this.render();
  }

  async revert() {
    this.dropPreview();
    try {
      for (const undo of this.applied.splice(0).reverse()) await undo();
      this.status = "ok:Reverted.";
      if (this.src) this.info = await api<Info>("describe", this.src);
    } catch (e) {
      this.status = `err:${(e as Error).message}`;
    }
    this.render();
  }

  async commit() {
    try {
      const r = await api<{ branch: string; commit: string }>("commit", { message: "chore(frontend): visual editor changes" });
      this.status = `ok:Committed ${r.commit} on ${r.branch}`;
    } catch (e) {
      this.status = `err:${(e as Error).message}`;
    }
    this.render();
  }

  el(tag: string, props: Record<string, unknown> = {}, ...kids: Array<Node | string>): any { // eslint-disable-line @typescript-eslint/no-explicit-any
    const e = Object.assign(document.createElement(tag), props);
    e.append(...kids);
    return e;
  }

  render() {
    const p = this.panel;
    p.replaceChildren();
    if (!this.locked) return;
    const sources = sourcesOf(this.locked);
    const h = this.el("h3", {}, this.src ? `${this.src.file}:${this.src.line}` : "No source tag on this element");
    p.append(h);
    if (sources.length > 1) {
      const sel = this.el("select");
      sources.forEach((s, i) => sel.append(new Option(`${s.label}${i === 0 ? "  (element)" : "  (component call site)"}`, String(i), false, s === this.src)));
      sel.onchange = () => void this.select(this.locked!, Number(sel.value));
      p.append(this.el("label", {}, "Edit target"), sel);
    }
    const bc = this.el("div", { className: "bc" });
    this.breadcrumb().forEach((b, i, all) => {
      const btn = this.el("button", { textContent: b.label });
      btn.onclick = () => void this.select(b.el);
      bc.append(btn, i < all.length - 1 ? " > " : "");
    });
    p.append(this.el("label", {}, "Path"), bc);
    const info = this.info;
    if (this.status.startsWith("err:") && !info) p.append(this.el("div", { className: "err", textContent: this.status.slice(4) }));
    if (info) {
      p.append(this.el("label", {}, "Text"));
      if (!info.segments.length) p.append(this.el("div", { className: "warn", textContent: "No text of its own. Select a child or ancestor." }));
      for (const seg of info.segments) {
        if (seg.kind === "dynamic") {
          p.append(this.el("div", { className: "warn", textContent: `Dynamic text ${seg.text} is not editable.` }));
          continue;
        }
        const inp = this.el("input", { type: "text", value: this.textEdits.get(seg.index) ?? seg.text });
        inp.oninput = () => {
          this.previewText(seg, inp.value);
          this.refreshButtons();
        };
        p.append(inp);
      }
      const n = document.querySelectorAll(`[data-src="${this.src!.label}"]`).length;
      if (n > 1) p.append(this.el("div", { className: "warn", textContent: `Shared: edits the template for ${n} rendered instances.` }));
      if (!info.classNameEditable) p.append(this.el("div", { className: "warn", textContent: "className is computed here; colors cannot be changed." }));
      else {
        for (const [prop, name] of [["text", "Text color"], ["bg", "Background"], ["border", "Border color"]] as const) {
          const cur = this.colorEdits.get(prop) ?? info.colors[prop] ?? "(none)";
          p.append(this.el("label", {}, `${name}: `, this.el("code", { textContent: cur })));
          const sw = this.el("div", { className: "sw" });
          for (const [tok, hex] of Object.entries(info.theme)) {
            if (!/^#[0-9a-f]{6}$/i.test(hex)) continue;
            const i = this.el("i", { title: tok });
            i.style.background = hex;
            if (cur === tok) i.className = "sel";
            i.onclick = () => {
              this.previewColor(prop, tok, hex);
              this.render();
            };
            sw.append(i);
          }
          const pick = this.el("input", { type: "color" });
          pick.oninput = () => {
            this.previewColor(prop, `[${pick.value}]`, pick.value);
            this.refreshButtons();
          };
          pick.onchange = () => this.render();
          p.append(sw, pick);
        }
        const cb = this.el("input", { type: "checkbox", checked: this.tokenWide });
        cb.onchange = () => (this.tokenWide = cb.checked);
        p.append(this.el("label", {}, cb, " Change the theme token everywhere (DESIGN.md)"));
      }
    }
    if (this.status) p.append(this.el("div", { className: this.status.startsWith("err:") ? "err" : "ok", textContent: this.status.slice(this.status.indexOf(":") + 1) }));
    const save = this.el("button", { textContent: "Save", onclick: () => void this.save() });
    const rev = this.el("button", { textContent: "Revert", onclick: () => void this.revert() });
    const com = this.el("button", { textContent: "Commit", title: "Commit saved edits to the current dev branch (never pushes)", onclick: () => void this.commit() });
    save.className = "save";
    rev.className = "revert";
    p.append(this.el("div", { className: "row" }, save, rev, com));
    this.refreshButtons();
  }

  refreshButtons() {
    const save = this.panel.querySelector<HTMLButtonElement>("button.save");
    const rev = this.panel.querySelector<HTMLButtonElement>("button.revert");
    if (save) save.disabled = !this.dirty;
    if (rev) rev.disabled = !this.dirty && this.applied.length === 0;
  }
}

if (!(window as unknown as { __devEditor?: Editor }).__devEditor) {
  (window as unknown as { __devEditor?: Editor }).__devEditor = new Editor();
}
