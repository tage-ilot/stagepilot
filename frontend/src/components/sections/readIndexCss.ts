/** Test helper: vitest blanks CSS imports, so the section tests read the stylesheet from disk (no @types/node in the app tsconfig). */
export async function readIndexCss(): Promise<string> {
  const { readFileSync } = (await import("node:fs" as string)) as { readFileSync(file: string, enc: "utf8"): string };
  return readFileSync(`${(globalThis as unknown as { process: { cwd(): string } }).process.cwd()}/src/index.css`, "utf8");
}
