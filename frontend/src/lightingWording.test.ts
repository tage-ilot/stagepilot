import { describe, expect, it } from "vitest";

// The word "Lights" must not reach a user: the feature is called "Lighting".
// Source scan of string literals, JSX text and attribute values. Allow-list = contract identifiers only.
const sources = import.meta.glob(["./**/*.ts", "./**/*.tsx"], { query: "?raw", import: "default", eager: true }) as Record<string, string>;

// Backend contract and test fixtures mirroring it keep `lights` (URLs, JSON keys, mirrored type names).
const contract = [
  /\/api\/v1\/lights[\w/-]*/g,
  /\b(?:get|update)Lights\w*/g,
  /\bLights(?:Status|Settings|Operation)\w*/g,
  /lights-icon-\w+\.png/g,
];

const strip = (text: string) => contract.reduce((out, re) => out.replace(re, ""), text);

describe("Lighting wording guard", () => {
  it("no user-facing string, label or title contains the word Lights", () => {
    const hits: string[] = [];
    for (const [file, text] of Object.entries(sources)) {
      if (file.endsWith("lightingWording.test.ts")) continue;
      strip(text).split("\n").forEach((line, index) => {
        // Capitalised word only: identifiers (lights, lights_status) are lower-case contract names.
        if (/\bLights\b/.test(line)) hits.push(`${file}:${index + 1}: ${line.trim().slice(0, 100)}`);
      });
    }
    expect(hits).toEqual([]);
  });
});
