// Pure source-level helpers for the dev-only visual editor: tag JSX elements with
// their source position, and rewrite exactly one element's static text or colour
// class using the TypeScript compiler API. Edits are text splices, never a
// reprint, so the diff stays minimal and formatting is untouched.
import fs from "node:fs";
import path from "node:path";
import ts from "typescript";

export class EditError extends Error {
  constructor(
    message: string,
    readonly status = 400,
  ) {
    super(message);
  }
}

export type ColorProp = "text" | "bg" | "border";

export interface TextSegment {
  index: number;
  kind: "static" | "dynamic";
  text: string;
}

export interface ElementInfo {
  tag: string;
  segments: TextSegment[];
  hasElementChildren: boolean;
  classNameEditable: boolean;
  /** Current colour value (token / arbitrary) for each prop, from the source. */
  colors: Partial<Record<ColorProp, string>>;
}

function parse(source: string, file = "x.tsx"): ts.SourceFile {
  return ts.createSourceFile(file, source, ts.ScriptTarget.ES2022, true, ts.ScriptKind.TSX);
}

type OpeningLike = ts.JsxOpeningElement | ts.JsxSelfClosingElement;

function isFragmentName(tag: string): boolean {
  return tag === "Fragment" || tag === "React.Fragment";
}

/** Add data-src="file:line:col" to every JSX element. Positions refer to the original source. */
export function tagJsx(source: string, relFile: string): string {
  const sf = parse(source, relFile);
  const inserts: Array<{ at: number; text: string }> = [];
  const visit = (node: ts.Node) => {
    if (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) {
      const tag = node.tagName.getText(sf);
      if (!isFragmentName(tag) && !node.typeArguments) {
        const { line, character } = sf.getLineAndCharacterOfPosition(node.getStart(sf));
        inserts.push({ at: node.tagName.getEnd(), text: ` data-src="${relFile}:${line + 1}:${character + 1}"` });
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
  let out = source;
  for (const { at, text } of inserts.sort((a, b) => b.at - a.at)) out = out.slice(0, at) + text + out.slice(at);
  return out;
}

function findOpening(sf: ts.SourceFile, line: number, col: number): OpeningLike {
  let found: OpeningLike | undefined;
  const visit = (node: ts.Node) => {
    if (found) return;
    if (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) {
      const p = sf.getLineAndCharacterOfPosition(node.getStart(sf));
      if (p.line + 1 === line && p.character + 1 === col) {
        found = node;
        return;
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
  if (!found) throw new EditError(`No JSX element at ${line}:${col} (source changed? reload the page)`, 409);
  return found;
}

function childrenOf(opening: OpeningLike): readonly ts.JsxChild[] {
  return ts.isJsxOpeningElement(opening) ? opening.parent.children : [];
}

function segmentsOf(sf: ts.SourceFile, opening: OpeningLike): TextSegment[] {
  const out: TextSegment[] = [];
  childrenOf(opening).forEach((child, index) => {
    if (ts.isJsxText(child)) {
      if (!child.containsOnlyTriviaWhiteSpaces) out.push({ index, kind: "static", text: child.getText(sf).trim().replace(/\s+/g, " ") });
    } else if (ts.isJsxExpression(child)) {
      const e = child.expression;
      if (!e) return;
      if (ts.isStringLiteral(e) || ts.isNoSubstitutionTemplateLiteral(e)) out.push({ index, kind: "static", text: e.text });
      else out.push({ index, kind: "dynamic", text: child.getText(sf) });
    }
  });
  return out;
}

interface Piece {
  start: number;
  end: number;
}

/** Collect the literal text ranges inside a className expression. */
function classPieces(sf: ts.SourceFile, expr: ts.Expression | undefined): Piece[] {
  const out: Piece[] = [];
  const walk = (e: ts.Node | undefined) => {
    if (!e) return;
    if (ts.isStringLiteral(e) || ts.isNoSubstitutionTemplateLiteral(e)) out.push({ start: e.getStart(sf) + 1, end: e.getEnd() - 1 });
    else if (ts.isTemplateExpression(e)) {
      out.push({ start: e.head.getStart(sf) + 1, end: e.head.getEnd() - 2 });
      for (const span of e.templateSpans) {
        const last = ts.isTemplateTail(span.literal);
        walk(span.expression);
        out.push({ start: span.literal.getStart(sf) + 1, end: span.literal.getEnd() - (last ? 1 : 2) });
      }
    } else if (ts.isParenthesizedExpression(e) || ts.isAsExpression(e) || ts.isNonNullExpression(e)) walk(e.expression);
    else if (ts.isConditionalExpression(e)) {
      walk(e.whenTrue);
      walk(e.whenFalse);
    } else if (ts.isBinaryExpression(e)) {
      walk(e.left);
      walk(e.right);
    } else if (ts.isCallExpression(e)) e.arguments.forEach(walk);
    else if (ts.isArrayLiteralExpression(e)) e.elements.forEach(walk);
  };
  walk(expr);
  return out;
}

function classNameAttr(opening: OpeningLike): ts.JsxAttribute | undefined {
  for (const a of opening.attributes.properties) {
    if (ts.isJsxAttribute(a) && a.name.getText() === "className") return a;
  }
  return undefined;
}

function classExpr(attr: ts.JsxAttribute | undefined): ts.Expression | undefined {
  const init = attr?.initializer;
  if (!init) return undefined;
  return ts.isJsxExpression(init) ? init.expression : init;
}

const STOCK = /^(white|black|transparent|current|inherit)$/;
const STOCK_SHADE = /^(?!opacity|ring|divide|gap|space)[a-z]+-\d{2,3}$/;
const ARBITRARY = /^\[(#|rgb|hsl|color:|var\(--)/;

/** True when the (variant-free) value after bg-/text-/border- is a colour. */
export function isColorValue(value: string, tokenNames: ReadonlySet<string>): boolean {
  const base = value.replace(/\/(\d{1,3}|\[[^\]]+\])$/, "");
  return tokenNames.has(base) || STOCK.test(base) || STOCK_SHADE.test(base) || ARBITRARY.test(base);
}

const PREFIX: Record<ColorProp, string> = { text: "text-", bg: "bg-", border: "border-" };

function colorTokens(sf: ts.SourceFile, pieces: Piece[], prop: ColorProp, names: ReadonlySet<string>) {
  const hits: Array<{ start: number; end: number; value: string }> = [];
  for (const p of pieces) {
    const text = sf.text.slice(p.start, p.end);
    for (const m of text.matchAll(/\S+/g)) {
      const tok = m[0];
      if (!tok.startsWith(PREFIX[prop])) continue; // variants (hover:...) start differently: left alone
      const value = tok.slice(PREFIX[prop].length);
      if (isColorValue(value, names)) hits.push({ start: p.start + m.index, end: p.start + m.index + tok.length, value });
    }
  }
  return hits;
}

export function describeElement(source: string, line: number, col: number, tokenNames: ReadonlySet<string>): ElementInfo {
  const sf = parse(source);
  const opening = findOpening(sf, line, col);
  const attr = classNameAttr(opening);
  const expr = classExpr(attr);
  const pieces = classPieces(sf, expr);
  const colors: Partial<Record<ColorProp, string>> = {};
  for (const prop of ["text", "bg", "border"] as ColorProp[]) {
    const hit = colorTokens(sf, pieces, prop, tokenNames)[0];
    if (hit) colors[prop] = hit.value;
  }
  return {
    tag: opening.tagName.getText(sf),
    segments: segmentsOf(sf, opening),
    hasElementChildren: childrenOf(opening).some((c) => ts.isJsxElement(c) || ts.isJsxSelfClosingElement(c)),
    classNameEditable: !attr || pieces.length > 0 || !attr.initializer,
    colors,
  };
}

function splice(source: string, start: number, end: number, text: string): string {
  return source.slice(0, start) + text + source.slice(end);
}

/** Replace the static text of one child segment. Dynamic parts are refused. */
export function editText(source: string, line: number, col: number, segmentIndex: number, newText: string, oldText?: string): string {
  if (/[\r\n]/.test(newText)) throw new EditError("Line breaks are not supported");
  const sf = parse(source);
  const opening = findOpening(sf, line, col);
  const child = childrenOf(opening)[segmentIndex];
  if (!child) throw new EditError("No such text segment");
  if (ts.isJsxText(child)) {
    if (child.containsOnlyTriviaWhiteSpaces) throw new EditError("No text there");
    if (/[{}<>]/.test(newText)) throw new EditError("Text may not contain { } < >");
    const raw = child.getText(sf);
    const lead = raw.length - raw.trimStart().length;
    const trail = raw.length - raw.trimEnd().length;
    const start = child.getStart(sf) + lead;
    const end = child.getEnd() - trail;
    const current = sf.text.slice(start, end).replace(/\s+/g, " ");
    if (oldText !== undefined && current !== oldText && current !== newText) throw new EditError("Source text changed; reload", 409);
    return splice(source, start, end, newText);
  }
  if (ts.isJsxExpression(child) && child.expression && (ts.isStringLiteral(child.expression) || ts.isNoSubstitutionTemplateLiteral(child.expression))) {
    const lit = child.expression;
    const quote = sf.text[lit.getStart(sf)]!;
    if (quote === "`" && /\$\{|`/.test(newText)) throw new EditError("Text may not contain ${ or `");
    const body = quote === "`" ? newText.replace(/\\/g, "\\\\") : newText.replace(/\\/g, "\\\\").split(quote).join("\\" + quote);
    if (oldText !== undefined && lit.text !== oldText && lit.text !== newText) throw new EditError("Source text changed; reload", 409);
    return splice(source, lit.getStart(sf) + 1, lit.getEnd() - 1, body);
  }
  throw new EditError("Dynamic text ({expression}, translations) is not editable");
}

/** Undo of "add class": drop the class again (and the className attribute if it ends up empty). */
export function removeColor(source: string, line: number, col: number, prop: ColorProp, value: string, tokenNames: ReadonlySet<string>): string {
  const sf = parse(source);
  const opening = findOpening(sf, line, col);
  const attr = classNameAttr(opening);
  const hit = colorTokens(sf, classPieces(sf, classExpr(attr)), prop, tokenNames).find((h) => h.value === value);
  if (!attr || !hit) return source; // already gone: idempotent
  const lit = attr.initializer;
  const only = lit && ts.isStringLiteral(lit) && lit.text.trim() === PREFIX[prop] + value;
  if (only) return splice(source, attr.getFullStart(), attr.getEnd(), "");
  let start = hit.start;
  let end = hit.end;
  if (source[start - 1] === " ") start -= 1;
  else if (source[end] === " ") end += 1;
  return splice(source, start, end, "");
}

const VALUE_OK = /^[A-Za-z0-9\-/[\]#.,%()_:]+$/;

/** Swap (or add) the Tailwind colour class for one prop. `value` is e.g. "primary" or "[#123456]". */
export function editColor(
  source: string,
  line: number,
  col: number,
  prop: ColorProp,
  value: string,
  tokenNames: ReadonlySet<string>,
  from?: string,
): string {
  if (!VALUE_OK.test(value) || !isColorValue(value, tokenNames)) throw new EditError("Not a colour value");
  const sf = parse(source);
  const opening = findOpening(sf, line, col);
  const attr = classNameAttr(opening);
  const pieces = classPieces(sf, classExpr(attr));
  const cls = PREFIX[prop] + value;
  let hits = colorTokens(sf, pieces, prop, tokenNames);
  if (from !== undefined) {
    const matching = hits.filter((h) => h.value === from);
    // `from` already gone (idempotent re-save) means the edit is applied.
    if (matching.length === 0 && hits.some((h) => h.value === value)) return source;
    hits = matching.length ? matching : hits.slice(0, 1);
  } else hits = hits.slice(0, 1);
  if (hits.length) {
    let out = source;
    for (const h of [...hits].sort((a, b) => b.start - a.start)) out = splice(out, h.start, h.end, cls);
    return out;
  }
  if (!attr) return splice(source, opening.tagName.getEnd(), opening.tagName.getEnd(), ` className="${cls}"`);
  const target = pieces[0];
  if (!target) throw new EditError("className is not a static string; cannot add a colour class");
  return splice(source, target.end, target.end, (target.end > target.start ? " " : "") + cls);
}

/** Edit a `--name: value;` custom property in a CSS file (first definition). */
export function editCssVar(css: string, name: string, value: string): string {
  if (!/^--[A-Za-z0-9-]+$/.test(name)) throw new EditError("Bad variable name");
  if (!/^(#[0-9a-fA-F]{3,8}|rgba?\([0-9 ,./%]+\)|hsla?\([0-9 ,./%deg]+\))$/.test(value)) throw new EditError("Bad colour value");
  const re = new RegExp(`(${name.replace(/[-]/g, "\\-")}\\s*:\\s*)([^;]+)(;)`);
  if (!re.test(css)) throw new EditError(`${name} is not defined`, 404);
  return css.replace(re, (_m, a: string, _b: string, c: string) => a + value + c);
}

const EDITABLE_EXT = new Set([".tsx", ".jsx", ".css"]);

/** Resolve `file` (relative to srcRoot) to a real path inside srcRoot, or throw. */
export function safeSrcPath(srcRoot: string, file: string): string {
  if (typeof file !== "string" || !file || file.includes("\0") || path.isAbsolute(file) || file.split(/[\\/]/).includes("..")) {
    throw new EditError("Invalid path", 403);
  }
  if (!EDITABLE_EXT.has(path.extname(file))) throw new EditError("File type not editable", 403);
  const root = fs.realpathSync(srcRoot);
  const target = path.resolve(root, file);
  let st: fs.Stats;
  try {
    st = fs.lstatSync(target);
  } catch {
    throw new EditError("No such file", 404);
  }
  if (st.isSymbolicLink() || !st.isFile()) throw new EditError("Symlinks and non-files are rejected", 403);
  const real = fs.realpathSync(target);
  if (real !== target || !real.startsWith(root + path.sep)) throw new EditError("Path escapes src", 403);
  return real;
}

/** Tailwind colour name -> DESIGN.md colour key (inverse of colorName() in scripts/design-tokens.mjs). */
export function designKey(tailwindName: string): string {
  return tailwindName.replace(/^ink(?=-|$)/, "text").replace(/^edge(?=-|$)/, "border").replace(/^surface-panel(?=-|$)/, "panel");
}

/** Change one hex value inside the `colors:` block of DESIGN.md, keeping the trailing comment. */
export function editDesignMd(md: string, tailwindName: string, hex: string): string {
  if (!/^#[0-9a-fA-F]{6}$/.test(hex)) throw new EditError("Token values must be #rrggbb");
  const key = designKey(tailwindName);
  if (!/^[a-z0-9-]+$/.test(key)) throw new EditError("Bad token name");
  const start = md.indexOf("\ncolors:\n");
  const end = md.indexOf("\ntypography:", start);
  if (start < 0 || end < 0) throw new EditError("DESIGN.md has no colors block", 500);
  const block = md.slice(start, end);
  const re = new RegExp(`^(  ${key}:\\s*)"#[0-9a-fA-F]{6}"`, "m");
  if (!re.test(block)) throw new EditError(`Token ${key} is not a plain #rrggbb colour`, 404);
  return md.slice(0, start) + block.replace(re, `$1"${hex.toLowerCase()}"`) + md.slice(end);
}
