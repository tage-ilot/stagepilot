export const DASHBOARD_WIDGET_IDS = [
  "service-plan",
  "now-playing",
  "manual-controls",
  "events",
] as const;

export type DashboardWidgetId = (typeof DASHBOARD_WIDGET_IDS)[number];
export type DashboardItemId = DashboardWidgetId;
export type DashboardLayoutMode = "desktop" | "tablet" | "mobile";

export type DashboardLayoutItem = {
  id: DashboardItemId;
  kind: "widget";
  x: number;
  y: number;
  w: number;
  h: number;
  minW?: number;
  minH?: number;
  maxW?: number;
  maxH?: number;
  locked?: boolean;
};
export type DashboardLayoutState = {
  version: 2;
  desktop: DashboardLayoutItem[];
  tablet: DashboardLayoutItem[];
  mobileOrder: DashboardItemId[];
  // Real measured content heights (grid rows) for mobile-mode widgets,
  // keyed by widget/spacer id. Populated by the same content-fit
  // measurement pass desktop/tablet use, so mobile no longer relies on a
  // static clamp derived from the desktop layout's `h`. Optional/partial
  // so older persisted layouts (pre-mobileHeights) still parse cleanly.
  mobileHeights?: Partial<Record<DashboardItemId, number>>;
  // User-controlled visibility for the Recent Event Stream widget. Only
  // togglable while edit-layout mode is active; when true the widget is
  // fully omitted (no render, no space) outside of edit mode. Optional so
  // older persisted layouts (pre-eventsHidden) still parse cleanly and
  // default to visible.
  eventsHidden?: boolean;
};

export const DASHBOARD_COLUMNS: Record<DashboardLayoutMode, number> = {
  desktop: 12,
  tablet: 6,
  mobile: 1,
};
