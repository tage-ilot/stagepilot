type ClassPart = string | false | null | undefined | 0;

/** Join authored classes in order; caller overrides stay last. */
export function cx(...parts: ClassPart[]): string {
  return parts.filter(Boolean).join(" ");
}
