import {
  DASHBOARD_COLUMNS,
  DASHBOARD_WIDGET_IDS,
  type DashboardItemId,
  type DashboardLayoutItem,
  type DashboardLayoutMode,
  type DashboardLayoutState,
  type DashboardWidgetId,
} from "./dashboardLayoutTypes";
import { DASHBOARD_WIDGETS } from "./dashboardWidgetRegistry";

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

export const isDashboardWidgetId = (value: unknown): value is DashboardWidgetId =>
  typeof value === "string" && DASHBOARD_WIDGET_IDS.includes(value as DashboardWidgetId);

// Spacers were removed (owner, 2026-10-14): widgets can sit anywhere on the grid. Older saved layouts may still carry them, so
// loading drops them silently instead of treating the whole layout as invalid.
const isLegacySpacerId = (value: unknown): boolean =>
  typeof value === "string" && /^spacer-[a-zA-Z0-9_-]+$/.test(value);
const withoutLegacySpacers = (value: unknown): unknown =>
  Array.isArray(value) ? value.filter((item) => !isLegacySpacerId(isRecord(item) ? item.id : item)) : value;

export const dashboardModeForWidth = (width: number): DashboardLayoutMode =>
  width < 1024 ? "mobile" : "desktop";

const widgetItem = (
  id: DashboardWidgetId,
  mode: "desktop" | "tablet",
): DashboardLayoutItem => ({
  id,
  kind: "widget",
  ...DASHBOARD_WIDGETS[id][mode],
});

export const createDefaultDashboardLayout = (): DashboardLayoutState => ({
  version: 2,
  desktop: DASHBOARD_WIDGET_IDS.map((id) => widgetItem(id, "desktop")),
  tablet: DASHBOARD_WIDGET_IDS.map((id) => widgetItem(id, "tablet")),
  mobileOrder: ["now-playing", "service-plan", "manual-controls", "events"],
  mobileHeights: {},
  eventsHidden: false,
});

const validInteger = (value: unknown, minimum = 0): value is number =>
  Number.isInteger(value) && Number(value) >= minimum;

const normalizeItem = (
  value: unknown,
  columns: number,
  mode: "desktop" | "tablet",
): DashboardLayoutItem | null => {
  if (!isRecord(value)) return null;
  const id = value.id;
  const kind = value.kind;
  if (!(kind === "widget" && isDashboardWidgetId(id))) return null;
  if (
    !validInteger(value.x)
    || !validInteger(value.y)
    || !validInteger(value.w, 1)
    || !validInteger(value.h, 1)
  ) return null;

  const constraints = DASHBOARD_WIDGETS[id][mode];
  const minW = Math.min(constraints?.minW ?? 1, columns);
  const minH = constraints?.minH ?? 1;
  const maxW = Math.min(constraints?.maxW ?? columns, columns);
  const maxH = constraints?.maxH;
  const w = Math.min(Math.max(value.w, minW), maxW);
  const h = Math.max(value.h, minH);
  return {
    id,
    kind,
    x: Math.min(value.x, Math.max(0, columns - w)),
    y: value.y,
    w,
    h: maxH ? Math.min(h, maxH) : h,
    minW,
    minH,
    maxW,
    ...(maxH ? { maxH } : {}),
    ...(value.locked === true ? { locked: true } : {}),
  };
};

const validItems = (
  value: unknown,
  columns: number,
  mode: "desktop" | "tablet",
): DashboardLayoutItem[] | null => {
  if (!Array.isArray(value)) return null;
  const withoutLegacyReadiness = value.filter(
    (item) => !(isRecord(item) && item.id === "readiness" && item.kind === "widget"),
  );
  const items = withoutLegacyReadiness.map((item) => normalizeItem(item, columns, mode));
  if (items.some((item) => item === null)) return null;
  const normalized = items as DashboardLayoutItem[];
  const ids = normalized.map(({ id }) => id);
  if (new Set(ids).size !== ids.length) return null;
  if (!DASHBOARD_WIDGET_IDS.every((id) => ids.filter((valueId) => valueId === id).length === 1)) {
    return null;
  }
  return normalized;
};

const validMobileHeights = (
  value: unknown,
  validIds: Set<DashboardItemId>,
): Partial<Record<DashboardItemId, number>> => {
  if (!isRecord(value)) return {};
  const result: Partial<Record<DashboardItemId, number>> = {};
  for (const [id, height] of Object.entries(value)) {
    if (!validIds.has(id as DashboardItemId) || !validInteger(height, 1)) continue;
    result[id as DashboardItemId] = height as number;
  }
  return result;
};

export const parseDashboardLayout = (value: unknown): DashboardLayoutState | null => {
  if (!isRecord(value) || value.version !== 2) return null;
  const desktop = validItems(withoutLegacySpacers(value.desktop), DASHBOARD_COLUMNS.desktop, "desktop");
  const tablet = validItems(withoutLegacySpacers(value.tablet), DASHBOARD_COLUMNS.tablet, "tablet");
  if (!desktop || !tablet || !Array.isArray(value.mobileOrder)) return null;
  const mobileOrder = (withoutLegacySpacers(value.mobileOrder) as unknown[]).filter((id) => id !== "readiness");
  if (
    mobileOrder.some((id) => !isDashboardWidgetId(id))
    || new Set(mobileOrder).size !== mobileOrder.length
    || !DASHBOARD_WIDGET_IDS.every((id) => mobileOrder.includes(id))
  ) return null;
  const mobileHeights = validMobileHeights(
    value.mobileHeights,
    new Set(mobileOrder as DashboardItemId[]),
  );
  const eventsHidden = typeof value.eventsHidden === "boolean" ? value.eventsHidden : false;
  return {
    version: 2,
    desktop,
    tablet,
    mobileOrder: mobileOrder as DashboardItemId[],
    mobileHeights,
    eventsHidden,
  };
};

export const packWidgetsInOrder = (
  order: DashboardWidgetId[],
  columns = DASHBOARD_COLUMNS.desktop,
): DashboardLayoutItem[] => {
  let x = 0;
  let y = 0;
  let rowHeight = 0;
  return order.map((id) => {
    const base = widgetItem(id, columns === DASHBOARD_COLUMNS.tablet ? "tablet" : "desktop");
    const w = Math.min(base.w, columns);
    if (x + w > columns) {
      x = 0;
      y += rowHeight;
      rowHeight = 0;
    }
    const item = { ...base, x, y, w };
    x += w;
    rowHeight = Math.max(rowHeight, item.h);
    return item;
  });
};

export const migrateDashboardOrder = (value: unknown): DashboardLayoutState | null => {
  const orderWithoutLegacyReadiness = Array.isArray(value)
    ? value.filter((id) => id !== "readiness")
    : value;
  if (
    !Array.isArray(orderWithoutLegacyReadiness)
    || orderWithoutLegacyReadiness.length !== DASHBOARD_WIDGET_IDS.length
    || orderWithoutLegacyReadiness.some((id) => !isDashboardWidgetId(id))
    || new Set(orderWithoutLegacyReadiness).size !== orderWithoutLegacyReadiness.length
    || !DASHBOARD_WIDGET_IDS.every((id) => orderWithoutLegacyReadiness.includes(id))
  ) return null;
  const order = orderWithoutLegacyReadiness as DashboardWidgetId[];
  return {
    version: 2,
    desktop: packWidgetsInOrder(order, DASHBOARD_COLUMNS.desktop),
    tablet: packWidgetsInOrder(order, DASHBOARD_COLUMNS.tablet),
    mobileOrder: [...order],
    mobileHeights: {},
    eventsHidden: false,
  };
};

export const orderedLayoutItems = (items: DashboardLayoutItem[]) =>
  [...items].sort((left, right) =>
    left.y - right.y || left.x - right.x || left.id.localeCompare(right.id));

export const moveDashboardItem = (
  items: DashboardLayoutItem[],
  id: DashboardItemId,
  offset: -1 | 1,
): DashboardLayoutItem[] => {
  const ordered = orderedLayoutItems(items);
  const index = ordered.findIndex((item) => item.id === id);
  const target = index + offset;
  if (index < 0 || target < 0 || target >= ordered.length) return items;
  const left = ordered[index]!;
  const right = ordered[target]!;
  const leftPosition = { x: left.x, y: left.y };
  ordered[index] = { ...left, x: right.x, y: right.y };
  ordered[target] = { ...right, ...leftPosition };
  return ordered;
};
