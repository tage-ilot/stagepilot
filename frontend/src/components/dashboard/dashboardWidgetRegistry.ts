import type {
  DashboardLayoutItem,
  DashboardWidgetId,
} from "./dashboardLayoutTypes";

export type DashboardWidgetDefinition = {
  title: string;
  desktop: Omit<DashboardLayoutItem, "id" | "kind">;
  tablet: Omit<DashboardLayoutItem, "id" | "kind">;
};
// Default desktop arrangement (owner, 2026-10-15), the same for a first-time user and for "Reset layout": Service plan on the left;
// on the right, Now playing, then Manual controls, then Event stream, all the same width as Now playing (5 columns, x = 7).
export const DASHBOARD_WIDGETS: Record<DashboardWidgetId, DashboardWidgetDefinition> = {
  "service-plan": {
    title: "Service plan",
    desktop: { x: 0, y: 0, w: 7, h: 35, minW: 5, minH: 9, maxW: 12 },
    tablet: { x: 0, y: 0, w: 6, h: 28, minW: 4, minH: 9, maxW: 6 },
  },
  "now-playing": {
    title: "Now Playing",
    desktop: { x: 7, y: 0, w: 5, h: 13, minW: 4, minH: 11, maxW: 12 },
    tablet: { x: 0, y: 14, w: 6, h: 12, minW: 4, minH: 11, maxW: 6 },
  },
  "manual-controls": {
    title: "Manual Controls",
    desktop: { x: 7, y: 13, w: 5, h: 10, minW: 3, minH: 7, maxW: 12 },
    tablet: { x: 0, y: 26, w: 3, h: 12, minW: 3, minH: 8, maxW: 6 },
  },
  events: {
    title: "Recent Event Stream",
    desktop: { x: 7, y: 23, w: 5, h: 12, minW: 4, minH: 7, maxW: 12 },
    tablet: { x: 0, y: 38, w: 6, h: 11, minW: 4, minH: 7, maxW: 6 },
  },
};
