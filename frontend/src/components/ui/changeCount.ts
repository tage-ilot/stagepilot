const nbsp = "\u00a0";

/** "1 change" / "2 changes", with a non-breaking space between number and unit (DESIGN.md Build rules). */
export function changeCountText(count: number): string {
  return `${count}${nbsp}${count === 1 ? "change" : "changes"}`;
}
