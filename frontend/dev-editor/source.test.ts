// @vitest-environment node
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { EditError, describeElement, editColor, removeColor, editCssVar, editDesignMd, editText, safeSrcPath, tagJsx } from "./source";

const names = new Set(["primary", "ink", "ink-muted", "card", "surface-panel", "success"]);

const SRC = `import { Badge } from "./ui/Badge";

export function Panel({ name, items }: { name: string; items: string[] }) {
  return (
    <section className="rounded bg-card p-4 hover:bg-primary">
      <h2 className="text-lg font-semibold text-ink">Connection setup</h2>
      <p className="text-sm text-ink-muted">{name}</p>
      <p className="text-sm">
        Hello, {name} and welcome
      </p>
      <button type="button" className={\`px-3 \${items.length ? "bg-primary" : "bg-card"} text-ink\`}>
        Save
      </button>
      <span>{"Literal"}</span>
      <Badge tone="x" />
      <ul>{items.map((i) => <li key={i} className="border border-primary">{i}</li>)}</ul>
    </section>
  );
}
`;

const at = (needle: string): [number, number] => {
  const idx = SRC.indexOf(needle);
  const before = SRC.slice(0, idx).split("\n");
  return [before.length, before[before.length - 1]!.length + 1];
};

describe("tagJsx", () => {
  it("tags every element with file:line:col and leaves everything else untouched", () => {
    const out = tagJsx(SRC, "components/Panel.tsx");
    expect(out).toContain('<h2 data-src="components/Panel.tsx:6:7" className');
    expect(out).toContain('<Badge data-src="components/Panel.tsx:15:7" tone');
    expect(out.match(/data-src=/g)?.length).toBe(9);
    expect(out.replace(/ data-src="[^"]+"/g, "")).toBe(SRC);
  });
  it("skips fragments", () => {
    expect(tagJsx("const a = <><b>x</b></>;", "a.tsx")).toBe('const a = <><b data-src="a.tsx:1:13">x</b></>;');
  });
});

describe("text edits", () => {
  it("replaces a static string child, touching nothing else", () => {
    const [l, c] = at("<h2");
    const out = editText(SRC, l, c, 0, "Link setup");
    expect(out).toBe(SRC.replace("Connection setup", "Link setup"));
  });
  it("edits a string-literal expression child and escapes quotes", () => {
    const [l, c] = at("<span");
    expect(editText(SRC, l, c, 0, 'say "hi"')).toBe(SRC.replace('{"Literal"}', '{"say \\"hi\\""}'));
  });
  it("refuses dynamic text", () => {
    const [l, c] = at('<p className="text-sm text-ink-muted"');
    expect(() => editText(SRC, l, c, 0, "x")).toThrow(/Dynamic text/);
    expect(describeElement(SRC, l, c, names).segments).toEqual([{ index: 0, kind: "dynamic", text: "{name}" }]);
  });
  it("mixed content: static pieces editable, dynamic locked", () => {
    const [l, c] = at('<p className="text-sm">');
    const info = describeElement(SRC, l, c, names);
    expect(info.segments.map((s) => [s.kind, s.text])).toEqual([["static", "Hello,"], ["dynamic", "{name}"], ["static", "and welcome"]]);
    const out = editText(SRC, l, c, 2, "and goodbye");
    expect(out).toBe(SRC.replace("and welcome", "and goodbye"));
    expect(() => editText(SRC, l, c, 1, "x")).toThrow(EditError);
  });
  it("rejects braces and newlines in new text", () => {
    const [l, c] = at("<h2");
    expect(() => editText(SRC, l, c, 0, "a {b}")).toThrow();
    expect(() => editText(SRC, l, c, 0, "a\nb")).toThrow();
  });
  it("is idempotent", () => {
    const [l, c] = at("<h2");
    const once = editText(SRC, l, c, 0, "Link setup");
    expect(editText(once, l, c, 0, "Link setup")).toBe(once);
  });
  it("rejects a stale element position", () => {
    expect(() => editText(SRC, 1, 1, 0, "x")).toThrow(/No JSX element/);
  });
});

describe("colour edits", () => {
  it("swaps a theme class in a static className", () => {
    const [l, c] = at("<h2");
    expect(editColor(SRC, l, c, "text", "primary", names)).toBe(SRC.replace("font-semibold text-ink", "font-semibold text-primary"));
  });
  it("leaves hover: variants alone", () => {
    const [l, c] = at("<section");
    expect(editColor(SRC, l, c, "bg", "surface-panel", names)).toBe(SRC.replace("rounded bg-card p-4", "rounded bg-surface-panel p-4"));
  });
  it("accepts arbitrary values", () => {
    const [l, c] = at("<h2");
    expect(editColor(SRC, l, c, "text", "[#123456]", names)).toContain("font-semibold text-[#123456]");
  });
  it("rewrites inside template/conditional classNames", () => {
    const [l, c] = at("<button");
    const out = editColor(SRC, l, c, "bg", "success", names, "primary");
    expect(out).toContain('? "bg-success" : "bg-card"');
  });
  it("adds a class when none exists, and a className when absent", () => {
    const [l, c] = at("<h2");
    expect(editColor(SRC, l, c, "bg", "card", names)).toContain('text-ink bg-card">Connection');
    const [l2, c2] = at("<span");
    expect(editColor(SRC, l2, c2, "text", "ink", names)).toContain('<span className="text-ink">');
  });
  it("removeColor undoes an added class and an added className byte-for-byte", () => {
    const [l, c] = at("<h2");
    const added = editColor(SRC, l, c, "bg", "card", names);
    expect(removeColor(added, l, c, "bg", "card", names)).toBe(SRC);
    const [l2, c2] = at("<span");
    const addedAttr = editColor(SRC, l2, c2, "text", "ink", names);
    expect(removeColor(addedAttr, l2, c2, "text", "ink", names)).toBe(SRC);
    expect(removeColor(SRC, l2, c2, "text", "ink", names)).toBe(SRC);
  });
  it("does not treat text-sm / text-lg as colours", () => {
    const [l, c] = at("<h2");
    expect(describeElement(SRC, l, c, names).colors).toEqual({ text: "ink" });
  });
  it("is idempotent, including a repeated save with the original `from`", () => {
    const [l, c] = at("<h2");
    const once = editColor(SRC, l, c, "text", "primary", names, "ink");
    expect(editColor(once, l, c, "text", "primary", names, "ink")).toBe(once);
    expect(editColor(once, l, c, "text", "primary", names)).toBe(once);
  });
  it("rejects values that are not colours or contain injection", () => {
    const [l, c] = at("<h2");
    expect(() => editColor(SRC, l, c, "text", "lg", names)).toThrow();
    expect(() => editColor(SRC, l, c, "text", 'primary" onClick="x', names)).toThrow();
  });
});

describe("token edits", () => {
  const md = `---\ncolors:\n  primary: "#c7231d"   # deep red\n  text: "#f8f4ec"\ntypography:\n  primary: "#000000"\n---\n`;
  it("changes only the colors block value and keeps the comment", () => {
    expect(editDesignMd(md, "primary", "#112233")).toBe(md.replace('"#c7231d"', '"#112233"'));
  });
  it("maps tailwind names back to DESIGN.md keys", () => {
    expect(editDesignMd(md, "ink", "#ffffff")).toContain('text: "#ffffff"');
  });
  it("rejects bad input", () => {
    expect(() => editDesignMd(md, "nope", "#112233")).toThrow(/not a plain/);
    expect(() => editDesignMd(md, "primary", "red")).toThrow();
  });
  it("edits a CSS variable", () => {
    expect(editCssVar(":root { --x-bg: #000;\n}", "--x-bg", "#fff")).toBe(":root { --x-bg: #fff;\n}");
    expect(() => editCssVar(":root{}", "--x-bg", "#fff")).toThrow();
    expect(() => editCssVar(":root{--a:1;}", "--a", "red;} body{")).toThrow();
  });
});

describe("safeSrcPath", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "dev-editor-"));
  const src = path.join(dir, "src");
  fs.mkdirSync(path.join(src, "components"), { recursive: true });
  fs.writeFileSync(path.join(src, "components", "A.tsx"), "x");
  fs.writeFileSync(path.join(dir, "secret.tsx"), "x");
  fs.symlinkSync(path.join(dir, "secret.tsx"), path.join(src, "link.tsx"));
  fs.symlinkSync(dir, path.join(src, "linkdir"));
  it("accepts a normal file", () => expect(safeSrcPath(src, "components/A.tsx")).toBe(fs.realpathSync(path.join(src, "components", "A.tsx"))));
  it.each(["../secret.tsx", "components/../../secret.tsx", "/etc/passwd", "link.tsx", "linkdir/secret.tsx", "components/A.ts", "package.json", "", "a\0.tsx"])(
    "rejects %j",
    (p) => expect(() => safeSrcPath(src, p)).toThrow(EditError),
  );
});
