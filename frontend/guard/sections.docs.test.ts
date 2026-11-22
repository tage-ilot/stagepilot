// @vitest-environment node
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { ESLint } from "eslint";
import { describe, expect, it } from "vitest";

/** The documentation of the section templates (DESIGN.md §6.6, CONTRIBUTING.md) cannot drift from the code it describes. */
const frontend = fileURLToPath(new URL("../", import.meta.url));
// Windows checks files out with CRLF; the quoted example is compared line for line, so read every file with LF.
const read = (path: string) => readFileSync(fileURLToPath(new URL(path, import.meta.url)), "utf8").replace(/\r\n/g, "\n");
const design = read("../../DESIGN.md");
const section = design.slice(design.indexOf("### 6.6 Section templates"), design.indexOf("## 7. The dashboard"));

describe("DESIGN.md §6.6 and CONTRIBUTING.md", () => {
  it("names every template the sections folder exports, and a props line for each", () => {
    const exported = Array.from(read("../src/components/sections/index.ts").matchAll(/export \{([^}]*)\} from/g))
      .flatMap((match) => (match[1] ?? "").split(",").map((name) => name.trim()))
      .filter((name) => /^[A-Z]/.test(name) && !name.startsWith("type "));
    expect(exported.length).toBeGreaterThanOrEqual(14);
    const unnamed = exported.filter((name) => !section.includes(`\`${name}\``));
    expect(unnamed, "every export of components/sections/index.ts is in DESIGN.md §6.6").toEqual([]);
    for (const template of ["ConnectionSection", "ConnectionRow", "HelperSetupBox", "SettingsSection", "ActionRow", "ActionButton", "StatusSummary", "SectionNotice", "OptionalSection", "OptionalBand", "PanelLayout", "PanelBody"]) {
      expect(section, `a props line for ${template}`).toMatch(new RegExp(`\\| \`${template}\` \\| .*\`[a-zA-Z-]+\``));
    }
  });

  it("documents the Field action slot (a button under one field) beside SettingsSection, and says the actions slot then draws nothing", () => {
    expect(read("../src/components/ui/Field.tsx")).toMatch(/action\?: ReactNode/);
    expect(section).toMatch(/\| `SettingsSection` \| [^\n]*`Field` may carry an `action`/);
    expect(section).toContain("`Field`'s `action`");
    expect(section).toContain("draws nothing");
  });

  it("says the parts the task asked for: the order of a panel, a worked example, a design change, the gallery, the guard", () => {
    for (const heading of ["**The order of a configuration page (`PanelLayout`)", "**How to add a connection**", "**How to make a design change to one section type.**", "**The guard.**", "**The section gallery**", "Last built from"]) {
      expect(section, heading).toContain(heading);
    }
  });

  it("the worked example in DESIGN.md is the real file, word for word", () => {
    const real = read("../src/gallery/AcmeSetupPanel.tsx").trim();
    const marker = "<!-- example: frontend/src/gallery/AcmeSetupPanel.tsx -->\n```tsx\n";
    const start = section.indexOf(marker);
    expect(start).toBeGreaterThan(-1);
    const quoted = section.slice(start + marker.length, section.indexOf("\n```", start + marker.length)).trim();
    expect(quoted).toBe(real);
    expect(real.split("\n").length, "a new connection page in about 40 lines").toBeLessThanOrEqual(40);
  });

  it("the worked example passes the guard as a page file", async () => {
    const eslint = new ESLint({ cwd: frontend });
    const code = read("../src/gallery/AcmeSetupPanel.tsx");
    const [result] = await eslint.lintText(code, { filePath: `${frontend}src/components/AcmeSetupPanel.tsx` });
    expect(result!.messages).toEqual([]);
  });

  it("CONTRIBUTING.md points to it", () => {
    const contributing = read("../../CONTRIBUTING.md");
    expect(contributing).toMatch(/DESIGN\.md.*6\.6.*Section templates/);
    expect(contributing).toContain("components/sections/");
  });
});
