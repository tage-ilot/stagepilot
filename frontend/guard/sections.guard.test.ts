// @vitest-environment node
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

import { ESLint } from "eslint";
import { describe, expect, it } from "vitest";

/**
 * The guard (DESIGN.md §6.6, `eslint.sections.js`): a configuration page cannot draw its own copy of a section template, and the message
 * says which template to use. This runs the REAL lint configuration over small made-up page files, so a rule that is weakened or dropped fails here.
 */
const root = fileURLToPath(new URL("../", import.meta.url));
const eslint = new ESLint({ cwd: root });
const PAGE = join(root, "src/components/FooSetupPanel.tsx");
const lint = async (code: string, filePath = PAGE) => (await eslint.lintText(code, { filePath }))[0]!.messages.filter((message) => message.ruleId !== null);

const WRONG: { what: string; code: string; says: RegExp }[] = [
  { what: "a dl summary", code: "export const A = () => <dl><dt>API</dt><dd>Connected</dd></dl>;", says: /StatusSummary/ },
  { what: "a Field grid", code: 'export const A = () => <div className="grid gap-2 md:grid-cols-2"><input /></div>;', says: /SettingsSection/ },
  { what: "a Field grid in a template literal", code: "export const A = ({ x }: { x: string }) => <div className={`grid ${x} sm:grid-cols-3`}><input /></div>;", says: /SettingsSection/ },
  { what: "a Field grid built with cx()", code: 'declare const cx: (...a: string[]) => string;\nexport const A = () => <div className={cx("grid", "lg:grid-cols-4")} />;', says: /SettingsSection/ },
  { what: "a Live Link shell", code: 'export const A = () => <section className="live-link mt-6 rounded-ds-xl border"><h3>Foo Live Link</h3></section>;', says: /ConnectionSection/ },
  { what: "a Live Link shell by its fill", code: 'export const A = () => <div className="rounded-ds-xl border bg-surface-panel-strong" />;', says: /ConnectionSection/ },
  { what: "a connection row", code: 'export const A = () => <div className="mt-2 flex flex-wrap items-end gap-x-2.5 gap-y-2"><input /></div>;', says: /ConnectionRow/ },
  { what: "a Details button", code: "export const A = ({ open }: { open: boolean }) => <button aria-expanded={open} type=\"button\">Details</button>;", says: /ConnectionSection|OptionalSection/ },
  { what: "a Refresh button", code: 'declare const Button: (p: { children: unknown }) => null;\nexport const A = () => <Button>Refresh timers</Button>;', says: /ActionRow/ },
  { what: "a Test button with busy words", code: 'declare const Button: (p: { children: unknown }) => null;\nexport const A = ({ busy }: { busy: boolean }) => <Button>{busy ? "Testing…" : "Test connection"}</Button>;', says: /ActionButton/ },
  { what: "its own host box", code: 'declare const Input: (p: { "aria-label": string }) => null;\nexport const A = () => <Input aria-label="Host" />;', says: /AddressBox/ },
  { what: "its own computer box", code: 'declare const Input: (p: { "aria-label": string }) => null;\nexport const A = () => <Input aria-label="Playback computer" />;', says: /AddressBox/ },
  { what: "its own Panel", code: 'import { Panel } from "./ui/Panel";\nexport const A = () => <Panel tone="midi" />;', says: /PanelLayout/ },
  { what: "its own Disclosure", code: 'import { Disclosure } from "./ui/Disclosure";\nexport const A = () => <Disclosure title="x" />;', says: /OptionalSection/ },
  { what: "its own InlineAlert", code: 'import { InlineAlert } from "./ui/InlineAlert";\nexport const A = () => <InlineAlert tone="info">x</InlineAlert>;', says: /SectionNotice/ },
  { what: "a template taken from one of its files", code: 'import { ActionRow } from "./sections/ActionRow";\nexport const A = () => <ActionRow>x</ActionRow>;', says: /components\/sections/ },
];

describe("the section-template guard", () => {
  it.each(WRONG)("stops a page from rebuilding $what, and names the template", async ({ code, says }) => {
    const found = await lint(code);
    expect(found.length, "the page must be flagged").toBeGreaterThan(0);
    expect(found.every((message) => message.severity === 2), "as an error, not a warning").toBe(true);
    expect(found.map((message) => message.message).join("\n")).toMatch(says);
  });

  it("lets a page built from the templates through", async () => {
    const code = `import { ActionButton, ActionRow, PanelLayout, SettingsSection, StatusSummary } from "./sections";
export const A = () => (
  <PanelLayout header={null} id="foo" tone="midi"
    settings={<SettingsSection columns="pair"><input /></SettingsSection>}
    actions={<ActionRow><ActionButton label="Test connection" busyLabel="Testing…" onClick={() => undefined} /></ActionRow>}
    summary={<StatusSummary items={[{ label: "API", value: "Connected" }]} />} />
);`;
    expect(await lint(code)).toEqual([]);
  });

  it("lets a page put one field's button in that Field's action, with no actions slot", async () => {
    const code = `import { ActionButton, ActionRow, PanelLayout, SettingsSection } from "./sections";
declare const Field: (p: { action?: unknown; label: string; children: unknown }) => null;
export const A = () => (
  <PanelLayout header={null} id="foo" tone="midi"
    settings={<SettingsSection columns="pair"><Field action={<ActionRow><ActionButton label="Refresh timers and Looks" onClick={() => undefined} /></ActionRow>} label="Timer"><input /></Field></SettingsSection>} />
);`;
    expect(await lint(code)).toEqual([]);
  });

  it("allows an exception only on its own line, with the reason beside it", async () => {
    const code = '// eslint-disable-next-line no-restricted-syntax -- page-specific: a list of choices, not fields\nexport const A = () => <ol className="grid sm:grid-cols-2" />;';
    expect(await lint(code)).toEqual([]);
  });

  it("does not touch files that are not pages (the templates themselves, the dashboard)", async () => {
    const code = 'export const A = () => <div className="grid md:grid-cols-2"><dl /></div>;';
    expect(await lint(code, join(root, "src/components/sections/Anything.tsx"))).toEqual([]);
    expect(await lint(code, join(root, "src/components/StatusCard.tsx"))).toEqual([]);
  });

  /** A new page must not slip past the guard by being named differently: any file that is built from PanelLayout, ConnectionSection or SettingsSection is guarded. */
  it("guards every file that uses PanelLayout, ConnectionSection or SettingsSection", async () => {
    const files: string[] = [];
    const walk = (dir: string) => {
      for (const name of readdirSync(dir)) {
        const path = join(dir, name);
        if (statSync(path).isDirectory()) { if (!/(^|\/)(sections|gallery|test|node_modules)$/.test(path.replaceAll("\\", "/"))) walk(path); continue; }
        if (/\.tsx$/.test(name) && !/\.test\.tsx$/.test(name)) files.push(path);
      }
    };
    walk(join(root, "src"));
    const users = files.filter((file) => /\b(PanelLayout|ConnectionSection|SettingsSection)\b/.test(readFileSync(file, "utf8")));
    expect(users.length).toBeGreaterThanOrEqual(10);
    const unguarded: string[] = [];
    for (const file of users) {
      const config = (await eslint.calculateConfigForFile(file)) as { rules?: Record<string, unknown[]> };
      const rule = config.rules?.["no-restricted-syntax"];
      if (!Array.isArray(rule) || rule[0] !== 2 && rule[0] !== "error") unguarded.push(relative(root, file));
    }
    expect(unguarded, "name a new page like the others (…SetupPanel.tsx) or add it to PAGE_FILES in eslint.sections.js").toEqual([]);
  });
});
