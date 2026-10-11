export const states = ["default", "hover", "focus", "active", "disabled", "loading", "error"] as const;
export type GalleryState = typeof states[number];
