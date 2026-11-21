// @vitest-environment node
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

import ts from "typescript";
import { describe, expect, it } from "vitest";

/**
 * The guard (DESIGN.md, song cards): a `<button>` that is a flex column writes its `items-*`. WebKit's own style for `button` is
 * `align-items: flex-start`, so a column that relies on the browser default does not stretch its children on the Mac app (the song card's
 * colored bar was a quarter wide) while Chrome and newer WebKit stretch it, and a preview in Chrome never shows it.
 *
 * It reads the class names written in `src/components/**` (a string, a template, `cx(...)`, or a `const` string in the same file) and checks
 * only what is written: no layout, so it runs in a few milliseconds.
 */
const root = fileURLToPath(new URL("../", import.meta.url));
const FIX = "write items-stretch (the children span the button) or items-start / items-center / items-end / items-baseline (and why) next to flex-col";
const ITEMS = /^items-(stretch|start|center|end|baseline)$/;

/** Every string the class name of an element can be made of, following a name to its `const` in the same file. */
function classWords(file: ts.SourceFile): (node: ts.Node, seen?: Set<string>) => string[] {
  const constants = new Map<string, ts.Expression>();
  for (const statement of file.statements) {
    if (!ts.isVariableStatement(statement)) continue;
    for (const declaration of statement.declarationList.declarations) {
      if (ts.isIdentifier(declaration.name) && declaration.initializer) constants.set(declaration.name.text, declaration.initializer);
    }
  }
  const words = (node: ts.Node, seen = new Set<string>()): string[] => {
    if (ts.isStringLiteralLike(node)) return node.text.split(/\s+/);
    if (ts.isIdentifier(node)) {
      const value = constants.get(node.text);
      if (!value || seen.has(node.text)) return [];
      return words(value, new Set(seen).add(node.text));
    }
    const found: string[] = [];
    node.forEachChild((child) => found.push(...words(child, seen)));
    if (ts.isTemplateExpression(node)) found.push(...node.head.text.split(/\s+/));
    return found;
  };
  return words;
}

/** The `<button>` elements of a file whose class names say `flex-col` and no `items-*`. Returns `file:line` with the words to write. */
export function columnButtonsWithoutItems(code: string, fileName: string): string[] {
  const file = ts.createSourceFile(fileName, code, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const wordsOf = classWords(file);
  const found: string[] = [];
  const visit = (node: ts.Node) => {
    const opening = ts.isJsxElement(node) ? node.openingElement : ts.isJsxSelfClosingElement(node) ? node : null;
    if (opening && opening.tagName.getText(file) === "button") {
      const className = opening.attributes.properties.find((attribute): attribute is ts.JsxAttribute => ts.isJsxAttribute(attribute) && attribute.name.getText(file) === "className");
      const tokens = className?.initializer ? wordsOf(className.initializer) : [];
      if (tokens.includes("flex-col") && !tokens.some((token) => ITEMS.test(token))) {
        found.push(`${fileName}:${file.getLineAndCharacterOfPosition(opening.getStart(file)).line + 1} a <button> with flex-col and no items-*: ${FIX}`);
      }
    }
    node.forEachChild(visit);
  };
  visit(file);
  return found;
}

describe("the column-button guard", () => {
  it.each([
    { what: "a plain string", code: 'export const A = () => <button className="flex flex-col gap-1" type="button">x</button>;' },
    { what: "a template literal", code: "export const A = ({ on }: { on: boolean }) => <button className={`flex flex-col ${on ? \"a\" : \"b\"}`} type=\"button\">x</button>;" },
    { what: "a const written once and used", code: 'const CARD = "flex min-h-[44px] flex-col justify-between";\nexport const A = () => <button className={CARD} type="button">x</button>;' },
    { what: "a const used inside cx()", code: 'declare const cx: (...a: string[]) => string;\nconst CARD = "flex flex-col";\nexport const A = () => <button className={cx(CARD, "p-2")} type="button">x</button>;' },
    { what: "a self-closing button", code: 'export const A = () => <button className="flex flex-col" type="button" />;' },
    { what: "an items-* that only applies at one width", code: 'export const A = () => <button className="flex flex-col sm:items-center" type="button">x</button>;' },
  ])("flags $what, and says what to write", ({ code }) => {
    const found = columnButtonsWithoutItems(code, "Foo.tsx");
    expect(found).toHaveLength(1);
    expect(found[0]).toMatch(/items-stretch/);
    expect(found[0]).toMatch(/Foo\.tsx:\d+/);
  });

  it.each([
    { what: "items-stretch", code: 'export const A = () => <button className="flex flex-col items-stretch" type="button">x</button>;' },
    { what: "items-start", code: 'export const A = () => <button className="flex flex-col items-start" type="button">x</button>;' },
    { what: "items-center in a template literal", code: "export const A = ({ on }: { on: boolean }) => <button className={`flex flex-col items-center ${on ? \"a\" : \"\"}`} type=\"button\">x</button>;" },
    { what: "items-baseline in a const", code: 'const CARD = "flex flex-col items-baseline";\nexport const A = () => <button className={CARD} type="button">x</button>;' },
    { what: "a row (no flex-col)", code: 'export const A = () => <button className="flex items-center gap-2" type="button">x</button>;' },
    { what: "a div that is a column (not a button)", code: 'export const A = () => <div className="flex flex-col">x</div>;' },
    { what: "a button with no class", code: "export const A = () => <button type=\"button\">x</button>;" },
  ])("lets through $what", ({ code }) => {
    expect(columnButtonsWithoutItems(code, "Foo.tsx")).toEqual([]);
  });

  it("finds none in src/components", () => {
    const found: string[] = [];
    let files = 0;
    const walk = (dir: string) => {
      for (const name of readdirSync(dir)) {
        const path = join(dir, name);
        if (statSync(path).isDirectory()) { if (name !== "node_modules") walk(path); continue; }
        if (!/\.tsx$/.test(name) || /\.test\.tsx$/.test(name)) continue;
        files += 1;
        found.push(...columnButtonsWithoutItems(readFileSync(path, "utf8"), relative(root, path).replaceAll("\\", "/")));
      }
    };
    walk(join(root, "src/components"));
    expect(files, "the guard must be reading the components").toBeGreaterThan(50);
    expect(found).toEqual([]);
  });
});
