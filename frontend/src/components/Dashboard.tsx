
import { useEffect, useLayoutEffect, useRef, useState, type MouseEventHandler, type Ref, type TouchEventHandler } from "react";

import { LOCAL_CAPABILITIES } from "../access/accessState";
import lightingIcon from "../assets/lights-icon-purple.png";
import multitracksIcon from "../assets/multitracks-icon.png";
import planningCenterIcon from "../assets/planning-center-icon.png";
import propresenterIcon from "../assets/propresenter-icon.png";
import stagepilotIcon from "../assets/stagepilot-icon.png";
import { useDelayedHover } from "../hooks/useDelayedHover";
import type {
  AccessCapabilities,
  ActionName,
  ApplicationState,
  ConnectionStatus,
  GeneralSettingsInput,
  HealthResponse,
  LightingCue,
  LightsSettingsInput,
  LightsStatusResponse,
  MidiCueName,
  MidiInputsResponse,
  MidiMonitorMessage,
  MidiSettingsInput,
  PlanningCenterServiceType,
  PlanningCenterSettingsInput,
  PlanningCenterStatusResponse,
  PlanningCenterTestInput,
  ProPresenterSettingsInput,
  ProPresenterStatusResponse,
  SettingsResponse,
  SkippedServiceItem,
  Song,
} from "../types";
import type { UpdaterController } from "../hooks/useUpdater";
import { BackendSetupPanel } from "./BackendSetupPanel";
import { DashboardGrid } from "./dashboard/DashboardGrid";
import { latestActiveError } from "./dashboard/dashboardActiveError";
import {
  buildConnectionCardViews,
  buildReadinessChecks,
  readinessErrorCauses,
  readinessHasError,
  readinessPassed,
} from "./dashboard/dashboardReadiness";
import { LightingSetupPanel } from "./LightingSetupPanel";
import { MidiSetupPanel } from "./MidiSetupPanel";
import type { PlaybackController } from "../hooks/usePlaybackInput";
import { PlanningCenterSetupPanel } from "./PlanningCenterSetupPanel";
import { ProPresenterSetupPanel } from "./ProPresenterSetupPanel";
import { SetupChecklist } from "./SetupChecklist";
import { StatusCard } from "./StatusCard";
import { Button, Dialog, Panel, type LiveTone } from "./ui";
import { InlineAlert } from "./ui/InlineAlert";
import { UpdateAvailableButton } from "./UpdateAvailableButton";
import { UpdateDialog } from "./UpdateDialog";

type ConnectionPanel = "planning-center" | "midi" | "propresenter" | "lights" | "backend";
type HeaderNotification = {
  id: number;
  message: string;
  tone: "error" | "info";
};

const NOTIFICATION_DURATION_MS = 6_000;
const MAX_NOTIFICATION_QUEUE = 2;
// Retain the first-launch implementation for possible future use while keeping
// it disabled in every production configuration state.
const FIRST_LAUNCH_SETUP_ENABLED = false;

const formatDuration = (seconds: number | null | undefined) => {
  if (seconds == null) return "—:——";
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remainder = seconds % 60;
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(remainder).padStart(2, "0")}`
    : `${String(minutes).padStart(2, "0")}:${String(remainder).padStart(2, "0")}`;
};

const formatTime = (value: string | null | undefined) =>
  value
    ? new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit", second: "2-digit" }).format(new Date(value))
    : "No activity yet";

const formatPlanCurrentAsOf = (value: string | null | undefined) => {
  if (!value) return "Load time unavailable";
  const loadedAt = new Date(value);
  if (Number.isNaN(loadedAt.getTime())) return "Load time unavailable";
  const time = `${String(loadedAt.getHours()).padStart(2, "0")}:${String(loadedAt.getMinutes()).padStart(2, "0")}`;
  const date = `${String(loadedAt.getMonth() + 1).padStart(2, "0")}-${String(loadedAt.getDate()).padStart(2, "0")}-${loadedAt.getFullYear()}`;
  return `Current as of ${time} ${date}`;
};

const connectionIcon = (source: string, name: string, compact = false) => (
  <img
    alt=""
    className={`status-card-icon-image h-9 w-9 max-w-none object-cover ${compact ? "scale-110" : "scale-[1.35]"}`}
    data-status-icon={name}
    src={source}
  />
);

const OFFLINE_CONTROLS_SENTENCE = "Can't reach StagePilot, so these controls are off until it is back.";

function ActionButton({
  action,
  label,
  tone = "neutral",
  disabled,
  onAction,
}: {
  action: ActionName;
  label: string;
  tone?: "green" | "orange" | "red" | "blue" | "neutral";
  disabled: boolean;
  onAction: (action: ActionName) => void;
}) {
  const liveTone = { green: "go", orange: "back", red: "stop", blue: "reload", neutral: undefined }[tone] as LiveTone | undefined;
  return (
    <Button
      disabled={disabled}
      liveTone={liveTone}
      onClick={() => onAction(action)}
      variant={liveTone ? "live" : "secondary"}
    >
      {label}
    </Button>
  );
}

const CURSOR_POPOVER_OFFSET = 14;
const CURSOR_POPOVER_WIDTH = 288;
const TOUCH_TAP_MAX_DRIFT = 10;
const HOVER_PREVIEW_WARMUP_DELAY_MS = 500;

// Shared, widget-level "hover-intent" state: the popover only appears after
// the pointer has sat still over an item for HOVER_PREVIEW_WARMUP_DELAY_MS.
// Once warmed up during a continuous hover session over the widget, moving
// between adjacent items shows their popovers immediately (no re-delay).
// Leaving the widget entirely (hoverCount drops to 0 and stays there) resets
// the warmup so the delay applies again next time.
type HoverPreviewWarmupState = {
  hoverCount: number;
  warm: boolean;
  leaveTimer: ReturnType<typeof setTimeout> | null;
  delayMs: number;
};

function createHoverPreviewWarmupState(delayMs = HOVER_PREVIEW_WARMUP_DELAY_MS): HoverPreviewWarmupState {
  return { hoverCount: 0, warm: false, leaveTimer: null, delayMs };
}

function useCursorPopover(text: string | null | undefined, warmup?: HoverPreviewWarmupState) {
  const [visible, setVisible] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [position, setPosition] = useState({ x: 0, y: 0 });
  const hasText = Boolean(text && text.trim());
  const rowRef = useRef<HTMLElement | null>(null);
  const popoverRef = useRef<HTMLDivElement | null>(null);
  const sizeRef = useRef({ width: CURSOR_POPOVER_WIDTH, height: 0 });
  const touchStartRef = useRef<{ x: number; y: number } | null>(null);
  const lastPointerRef = useRef({ x: 0, y: 0 });
  const warmupTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const clearWarmupTimer = () => {
    if (warmupTimerRef.current === null) return;
    clearTimeout(warmupTimerRef.current);
    warmupTimerRef.current = null;
  };

  useEffect(() => clearWarmupTimer, []);

  const clampPosition = (clientX: number, clientY: number) => {
    if (typeof window === "undefined") return { x: clientX, y: clientY };
    const { width, height } = sizeRef.current;
    const maxX = window.innerWidth - width - 8;
    const maxY = window.innerHeight - height - 8;
    let y = clientY + CURSOR_POPOVER_OFFSET;
    // Flip above the cursor when there isn't room below the viewport.
    if (height && y > maxY) {
      const above = clientY - CURSOR_POPOVER_OFFSET - height;
      y = above >= 8 ? above : maxY;
    }
    return {
      x: Math.max(8, Math.min(clientX + CURSOR_POPOVER_OFFSET, maxX)),
      y: Math.max(8, Math.min(y, maxY)),
    };
  };

  // Re-measure the popover's real size once it renders, then re-clamp so
  // long notes never extend below (or above) the viewport.
  useLayoutEffect(() => {
    if (!visible || !popoverRef.current) return;
    const rect = popoverRef.current.getBoundingClientRect();
    sizeRef.current = { width: rect.width, height: rect.height };
    setPosition(clampPosition(lastPointerRef.current.x, lastPointerRef.current.y));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visible, text]);

  const openAt = (clientX: number, clientY: number) => {
    lastPointerRef.current = { x: clientX, y: clientY };
    setPosition(clampPosition(clientX, clientY));
    setVisible(true);
  };

  // Enters the widget's shared hover-intent session: if the widget hasn't
  // "warmed up" yet in this continuous session, wait for the warmup delay
  // before opening; if it's already warm (e.g. moving between adjacent
  // items), open immediately with no re-delay.
  const onMouseEnter: MouseEventHandler<HTMLElement> = (event) => {
    if (!hasText) return;
    const { clientX, clientY } = event;
    if (!warmup) {
      openAt(clientX, clientY);
      return;
    }
    if (warmup.leaveTimer !== null) {
      clearTimeout(warmup.leaveTimer);
      warmup.leaveTimer = null;
    }
    warmup.hoverCount += 1;
    if (warmup.warm) {
      openAt(clientX, clientY);
      return;
    }
    lastPointerRef.current = { x: clientX, y: clientY };
    clearWarmupTimer();
    warmupTimerRef.current = setTimeout(() => {
      warmupTimerRef.current = null;
      warmup.warm = true;
      openAt(lastPointerRef.current.x, lastPointerRef.current.y);
    }, warmup.delayMs);
  };

  const onMouseMove: MouseEventHandler<HTMLElement> = (event) => {
    if (!hasText) return;
    lastPointerRef.current = { x: event.clientX, y: event.clientY };
    setPosition(clampPosition(event.clientX, event.clientY));
  };

  // Leaving this item (but possibly still within the widget, e.g. moving to
  // an adjacent item) closes this item's popover. The widget-level warmup
  // only resets once ALL items have been left (hoverCount reaches 0), after
  // a short grace period so moving directly between adjacent items doesn't
  // momentarily drop to zero and reset warmth.
  const onMouseLeave = () => {
    clearWarmupTimer();
    setVisible(false);
    if (!warmup) return;
    warmup.hoverCount = Math.max(0, warmup.hoverCount - 1);
    if (warmup.hoverCount > 0) return;
    if (warmup.leaveTimer !== null) clearTimeout(warmup.leaveTimer);
    warmup.leaveTimer = setTimeout(() => {
      warmup.leaveTimer = null;
      if (warmup.hoverCount === 0) warmup.warm = false;
    }, 0);
  };

  // Touch has no hover state: track tap start, and on release treat it as a
  // tap (not a scroll/drag) if the finger barely moved, toggling the popover.
  const onTouchStart: TouchEventHandler<HTMLElement> = (event) => {
    if (!hasText) return;
    const touch = event.touches[0];
    if (!touch) return;
    touchStartRef.current = { x: touch.clientX, y: touch.clientY };
  };

  const onTouchEnd: TouchEventHandler<HTMLElement> = (event) => {
    const start = touchStartRef.current;
    touchStartRef.current = null;
    if (!hasText || !start) return;
    const touch = event.changedTouches[0];
    if (!touch) return;
    const drift = Math.hypot(touch.clientX - start.x, touch.clientY - start.y);
    if (drift > TOUCH_TAP_MAX_DRIFT) return; // scroll/drag, not a tap
    event.preventDefault();
    setVisible((prevVisible) => {
      const next = !prevVisible;
      if (next) openAt(touch.clientX, touch.clientY);
      setPinned(next);
      return next;
    });
  };

  // Tapping anywhere outside a pinned (touch-opened) popover dismisses it,
  // without interfering with normal scrolling/tapping elsewhere.
  useEffect(() => {
    if (!pinned) return;
    const handleOutside = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (rowRef.current && target && rowRef.current.contains(target)) return;
      if (popoverRef.current && target && popoverRef.current.contains(target)) return;
      setVisible(false);
      setPinned(false);
    };
    document.addEventListener("pointerdown", handleOutside, true);
    return () => document.removeEventListener("pointerdown", handleOutside, true);
  }, [pinned]);

  return {
    open: hasText && visible,
    position,
    popoverRef,
    hoverProps: {
      ref: rowRef,
      onMouseEnter,
      onMouseMove,
      onMouseLeave,
      onTouchStart,
      onTouchEnd,
    },
  };
}

function CursorPopover({
  open,
  position,
  title,
  text,
  popoverRef,
}: {
  open: boolean;
  position: { x: number; y: number };
  title?: string | null;
  text: string | null | undefined;
  popoverRef?: Ref<HTMLDivElement>;
}) {
  if (!open || !text) return null;
  return (
    <div
      ref={popoverRef}
      className="pointer-events-none fixed z-50 w-[min(18rem,calc(100vw-1rem))] rounded-ds-lg border border-edge bg-surface-950/95 p-3 text-sm text-ink-body shadow-elevation-overlay backdrop-blur-xl"
      role="tooltip"
      style={{ left: position.x, top: position.y }}
    >
      {title && title.trim() && (
        <p className="mb-1 truncate text-xs font-bold uppercase tracking-wider text-ink">{title}</p>
      )}
      {text}
    </div>
  );
}

function SongRow({ song, current, next, hoverWarmup }: { song: Song; current: boolean; next: boolean; hoverWarmup?: HoverPreviewWarmupState }) {
  const popover = useCursorPopover(song.description, hoverWarmup);
  const { ref: popoverHoverRef, ...popoverHoverProps } = popover.hoverProps;
  return (
    <li
      ref={popoverHoverRef as unknown as Ref<HTMLLIElement>}
      className={`group grid grid-cols-[2rem_minmax(0,1fr)_auto] items-center gap-3 border-t border-edge-subtle px-4 py-3 transition-colors duration-150 ease-out motion-reduce:transition-none can-hover:bg-ink/5 ${current ? "current-song-row" : ""}`}
      {...popoverHoverProps}
    >
      <span className={`grid h-7 w-7 place-items-center rounded-ds-lg text-xs font-bold tabular-nums transition-colors duration-150 motion-reduce:transition-none ${current ? "bg-primary text-on-primary" : next ? "bg-info/15 text-info-text group-hover:bg-info/25" : "bg-ink/5 text-ink-muted group-hover:bg-ink/10"}`}>
        {song.order}
      </span>
      <div className="min-w-0">
        <p className="break-words font-medium text-ink [overflow-wrap:anywhere]">{song.title}</p>
        <div className="mt-0.5 flex gap-2 font-type-micro text-type-micro uppercase">
          {current && <span className="text-primary-text">Current · live</span>}
          {next && <span className="text-info-text">Up next</span>}
          {song.is_generic && <span className="text-warning-text">Generic item</span>}
          {!song.duration_seconds && <span className="text-danger-text">Missing duration</span>}
        </div>
      </div>
      <span className={`font-mono text-sm font-semibold tabular-nums ${song.duration_seconds ? "text-ink-body" : "text-danger-text"}`}>
        {formatDuration(song.duration_seconds)}
      </span>
      <CursorPopover open={popover.open} position={popover.position} title={song.title} text={song.description} popoverRef={popover.popoverRef} />
    </li>
  );
}

function ReferenceItemRow({ item, hoverWarmup }: { item: SkippedServiceItem; hoverWarmup?: HoverPreviewWarmupState }) {
  const popover = useCursorPopover(item.description, hoverWarmup);
  const { ref: refHoverRef, ...refHoverProps } = popover.hoverProps;
  if (item.reason === "header") {
    return (
      <li className="border-t border-edge-subtle bg-surface-950/40 px-4 py-2 transition-colors duration-150 ease-out motion-reduce:transition-none can-hover:bg-surface-950/50">
        <p className="break-words text-xs font-extrabold uppercase tracking-[0.14em] text-ink-muted [overflow-wrap:anywhere]">
          {item.title}
        </p>
      </li>
    );
  }

  return (
    <li
      ref={refHoverRef as unknown as Ref<HTMLLIElement>}
      className="grid grid-cols-[2rem_minmax(0,1fr)_auto] items-center gap-3 border-t border-edge-subtle bg-surface-950/30 px-4 py-3 transition-colors duration-150 ease-out motion-reduce:transition-none can-hover:bg-surface-950/40"
      {...refHoverProps}
    >
      <span className="grid h-7 w-7 place-items-center rounded-ds-lg bg-surface-950/40 text-xs font-bold text-idle">•</span>
      <div className="min-w-0">
        <p className="break-words font-medium text-ink-muted [overflow-wrap:anywhere]">{item.title}</p>
        {item.description && (
          <p className="mt-0.5 truncate text-xs text-idle">{item.description}</p>
        )}
      </div>
      <span className="font-mono text-sm font-semibold tabular-nums text-idle">
        {formatDuration(item.duration_seconds)}
      </span>
      <CursorPopover open={popover.open} position={popover.position} title={item.title} text={item.description} popoverRef={popover.popoverRef} />
    </li>
  );
}

export function Dashboard({
  capabilities = LOCAL_CAPABILITIES,
  state,
  health,
  live,
  error,
  actionMessage,
  pendingAction,
  pendingPlanId,
  settings = null,
  settingsError = null,
  settingsMessage = null,
  pendingSettingsOperation = false,
  planningCenterStatus = null,
  planningCenterServiceTypes = [],
  planningCenterError = null,
  planningCenterMessage = null,
  pendingPlanningCenterOperation = null,
  midi,
  midiMessages,
  playback,
  midiError,
  midiMessage,
  pendingMidiOperation,
  pendingMidiCue,
  propresenter = null,
  propresenterError = null,
  propresenterMessage = null,
  pendingProPresenterOperation = null,
  lights = null,
  lightingError = null,
  lightingMessage = null,
  pendingLightingOperation = null,
  dispatch,
  retryLoads = () => undefined,
  selectPlan,
  saveGeneralSettings = () => undefined,
  saveMidiSettings = () => undefined,
  testPlanningCenterConnection = () => undefined,
  loadPlanningCenterServiceTypes = () => undefined,
  savePlanningCenter = () => undefined,
  signInPlanningCenterOAuth = () => undefined,
  disconnectPlanningCenter = () => undefined,
  refreshMidi,
  selectMidi,
  simulateMidi,
  saveProPresenter = () => undefined,
  runProPresenterTest = () => undefined,
  refreshProPresenter = () => undefined,
  saveLighting = () => undefined,
  refreshLighting = () => undefined,
  sendLightingTest = () => undefined,
  saveLightingCues = () => undefined,
  clearAllLightingCues = () => undefined,
  updater,
}: {
  capabilities?: AccessCapabilities;
  state: ApplicationState;
  health: HealthResponse | null;
  live: boolean;
  error: string | null;
  actionMessage: string | null;
  pendingAction: ActionName | null;
  pendingPlanId: string | null;
  settings?: SettingsResponse | null;
  settingsError?: string | null;
  settingsMessage?: string | null;
  pendingSettingsOperation?: boolean;
  planningCenterStatus?: PlanningCenterStatusResponse | null;
  planningCenterServiceTypes?: PlanningCenterServiceType[];
  planningCenterError?: string | null;
  planningCenterMessage?: string | null;
  pendingPlanningCenterOperation?: "test" | "load-types" | "save" | "oauth-sign-in" | "oauth-disconnect" | null;
  midi: MidiInputsResponse | null;
  midiMessages: MidiMonitorMessage[];
  playback?: PlaybackController;
  midiError: string | null;
  midiMessage: string | null;
  pendingMidiOperation: "refresh" | "connect" | "disconnect" | null;
  pendingMidiCue: MidiCueName | null;
  propresenter?: ProPresenterStatusResponse | null;
  propresenterError?: string | null;
  propresenterMessage?: string | null;
  pendingProPresenterOperation?: "save" | "test" | "refresh" | null;
  lights?: LightsStatusResponse | null;
  lightingError?: string | null;
  lightingMessage?: string | null;
  pendingLightingOperation?: "save" | "refresh" | "test" | "save-cues" | null;
  dispatch: (action: ActionName) => void;
  retryLoads?: () => void;
  selectPlan: (planId: string) => void;
  saveGeneralSettings?: (settings: GeneralSettingsInput) => void;
  saveMidiSettings?: (settings: MidiSettingsInput) => void | boolean | Promise<boolean | void>;
  testPlanningCenterConnection?: (input: PlanningCenterTestInput) => void;
  loadPlanningCenterServiceTypes?: () => void;
  savePlanningCenter?: (
    input: PlanningCenterSettingsInput,
    timezone: string,
  ) => void;
  signInPlanningCenterOAuth?: () => void;
  disconnectPlanningCenter?: () => void;
  refreshMidi: () => void;
  selectMidi: (inputId: string | null) => void;
  simulateMidi: (cue: MidiCueName) => void;
  saveProPresenter?: (settings: ProPresenterSettingsInput) => void;
  runProPresenterTest?: () => void;
  refreshProPresenter?: () => void;
  saveLighting?: (settings: LightsSettingsInput) => void;
  refreshLighting?: () => void;
  sendLightingTest?: (note: number, velocity: number) => void;
  saveLightingCues?: (song: Song, cues: LightingCue[]) => void;
  clearAllLightingCues?: (songs: Song[]) => void;
  updater?: UpdaterController;
}) {
  const { canOperate, canConfigure } = capabilities;
  const [activeConnection, setActiveConnection] = useState<ConnectionPanel | null>(null);
  const [clockNow, setClockNow] = useState(Date.now());
  const [notificationQueue, setNotificationQueue] = useState<HeaderNotification[]>([]);
  const [statusCompact, setStatusCompact] = useState(() => window.innerWidth <= 1_000);
  const [statusMotionPhase, setStatusMotionPhase] = useState<"idle" | "preparing" | "moving">("idle");
  const readinessHover = useDelayedHover();
  const hoverPreviewWarmupRef = useRef<HoverPreviewWarmupState>(createHoverPreviewWarmupState());
  const notificationId = useRef(0);
  const updateButton = useRef<HTMLButtonElement>(null);
  const connectionsRow = useRef<HTMLElement>(null);
  const statusCompactRef = useRef(statusCompact);
  const statusMotionPhaseRef = useRef(statusMotionPhase);
  const statusMotionRects = useRef(new Map<string, DOMRect>());
  const statusMotionTimers = useRef<number[]>([]);
  const statusPreparationAnimations = useRef<Animation[]>([]);
  const previousNotificationSources = useRef({
    action: null as string | null,
    error: null as string | null,
    service: null as string | null,
    update: null as string | null,
  });
  useEffect(() => {
    setClockNow(Date.now());
    if (state.timer.status !== "running" || !state.timer.started_at) return;
    const timer = window.setInterval(() => setClockNow(Date.now()), 250);
    return () => window.clearInterval(timer);
  }, [state.timer.started_at, state.timer.status]);
  useEffect(() => {
    const timer = window.setInterval(() => setClockNow(Date.now()), 5_000);
    return () => window.clearInterval(timer);
  }, []);
  useEffect(() => {
    let secondFrame = 0;
    const firstFrame = window.requestAnimationFrame(() => {
      secondFrame = window.requestAnimationFrame(() => {
        connectionsRow.current?.classList.add("status-motion-ready");
      });
    });
    return () => {
      window.cancelAnimationFrame(firstFrame);
      if (secondFrame) window.cancelAnimationFrame(secondFrame);
    };
  }, []);
  useEffect(() => {
    statusMotionPhaseRef.current = statusMotionPhase;
  }, [statusMotionPhase]);
  useEffect(() => {
    const clearMotionTimers = () => {
      for (const timer of statusMotionTimers.current) window.clearTimeout(timer);
      statusMotionTimers.current = [];
      for (const animation of statusPreparationAnimations.current) animation.cancel();
      statusPreparationAnimations.current = [];
    };
    const captureMotionRects = () => {
      const next = new Map<string, DOMRect>();
      connectionsRow.current?.querySelectorAll<HTMLElement>("[data-status-motion-part]").forEach((element, index) => {
        next.set(`${index}:${element.dataset.statusMotionPart}`, element.getBoundingClientRect());
      });
      statusMotionRects.current = next;
    };
    const finishMotion = () => {
      statusMotionPhaseRef.current = "idle";
      setStatusMotionPhase("idle");
    };
    const applyMode = (compact: boolean) => {
      captureMotionRects();
      statusCompactRef.current = compact;
      statusMotionPhaseRef.current = "moving";
      setStatusMotionPhase("moving");
      setStatusCompact(compact);
      statusMotionTimers.current.push(window.setTimeout(finishMotion, 295));
    };
    const handleResize = () => {
      const compact = window.innerWidth <= 1_000;
      if (compact && statusMotionPhaseRef.current === "preparing") return;
      if (compact === statusCompactRef.current) {
        if (statusMotionPhaseRef.current === "preparing") {
          clearMotionTimers();
          finishMotion();
        }
        return;
      }
      clearMotionTimers();
      if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false) {
        statusCompactRef.current = compact;
        setStatusCompact(compact);
        finishMotion();
        return;
      }
      if (compact) {
        statusMotionPhaseRef.current = "preparing";
        setStatusMotionPhase("preparing");
        const labels = connectionsRow.current?.querySelectorAll<HTMLElement>(
          ".status-card-title, .status-card-detail",
        ) ?? [];
        if (![...labels].every((label) => typeof label.animate === "function")) {
          statusMotionTimers.current.push(window.setTimeout(() => applyMode(true), 75));
          return;
        }
        const animations = [...labels].map((label) => label.animate(
          [{ opacity: 1 }, { opacity: 0 }],
          { duration: 75, easing: "linear", fill: "forwards" },
        ));
        statusPreparationAnimations.current = animations;
        void Promise.all(animations.map((animation) => animation.finished)).then(() => {
          for (const animation of animations) animation.cancel();
          statusPreparationAnimations.current = [];
          if (statusMotionPhaseRef.current === "preparing" && window.innerWidth <= 1_000) {
            applyMode(true);
          }
        }).catch(() => undefined);
      } else {
        applyMode(false);
      }
    };
    window.addEventListener("resize", handleResize);
    return () => {
      window.removeEventListener("resize", handleResize);
      clearMotionTimers();
    };
  }, []);
  useLayoutEffect(() => {
    const previous = statusMotionRects.current;
    if (!previous.size) return;
    connectionsRow.current?.querySelectorAll<HTMLElement>("[data-status-motion-part]").forEach((element, index) => {
      const before = previous.get(`${index}:${element.dataset.statusMotionPart}`);
      if (!before) return;
      const after = element.getBoundingClientRect();
      const scaleX = after.width ? before.width / after.width : 1;
      const scaleY = after.height ? before.height / after.height : 1;
      const isSurface = element.dataset.statusMotionPart === "surface";
      if (typeof element.animate !== "function") return;
      if (typeof element.getAnimations === "function") {
        element.getAnimations().forEach((animation) => animation.cancel());
      }
      element.animate([
        {
          opacity: isSurface ? 1 : 0.55,
          transform: `translate(${before.left - after.left}px, ${before.top - after.top}px) scale(${scaleX}, ${scaleY})`,
          transformOrigin: "top left",
        },
        { opacity: 1, transform: "none", transformOrigin: "top left" },
      ], { duration: 275, easing: "linear" });
    });
    statusMotionRects.current = new Map();
  }, [statusCompact]);
  const backendStatus: ConnectionStatus = state.application_status === "running" && live
    ? "connected"
    : state.application_status === "error" ? "error" : live ? "connecting" : "disconnected";
  const plan = state.plan;
  const serviceLoad = state.service_load;
  const serviceNotification = serviceLoad.status !== "idle"
    && serviceLoad.status !== "loaded"
    && serviceLoad.status !== "ambiguous"
    ? `${serviceLoad.message ?? "Service plan status changed."}${serviceLoad.is_stale ? " The last successful service plan is still displayed as stale." : ""}`
    : null;
  useEffect(() => {
    const previous = previousNotificationSources.current;
    const pending: HeaderNotification[] = [];
    const add = (message: string, tone: HeaderNotification["tone"]) => {
      notificationId.current += 1;
      pending.push({ id: notificationId.current, message, tone });
    };

    if (error && error !== previous.error) add(error, "error");
    if (actionMessage && actionMessage !== error && actionMessage !== previous.action) {
      add(actionMessage, "info");
    }
    if (serviceNotification && serviceNotification !== previous.service) {
      add(serviceNotification, serviceLoad.status === "loading" ? "info" : "error");
    }
    if (updater?.successMessage && updater.successMessage !== previous.update) {
      add(updater.successMessage, "info");
    }

    previousNotificationSources.current = {
      action: actionMessage,
      error,
      service: serviceNotification,
      update: updater?.successMessage ?? null,
    };
    if (pending.length) {
      setNotificationQueue((current) => [...current, ...pending].slice(-MAX_NOTIFICATION_QUEUE));
    }
  }, [actionMessage, error, serviceLoad.status, serviceNotification, updater?.successMessage]);

  const notification = notificationQueue[0] ?? null;
  useEffect(() => {
    if (!notification) return;
    const timeout = window.setTimeout(() => {
      setNotificationQueue((current) => current[0]?.id === notification.id ? current.slice(1) : current);
    }, NOTIFICATION_DURATION_MS);
    return () => window.clearTimeout(timeout);
  }, [notification]);
  const connectionViews = buildConnectionCardViews({
    playback: playback?.status,
    state,
    stateOnly: !canConfigure,
    settings,
    planningCenterStatus,
    midi,
    propresenter,
    lights,
  });
  const checks = buildReadinessChecks({
    state,
    stateOnly: !canConfigure,
    settings,
    propresenter,
    live,
    views: connectionViews,
  });
  const ready = readinessPassed(checks);
  const systemError = readinessHasError(checks);
  const pinnedError = latestActiveError(state);
  const activeErrorEventId = pinnedError?.event_id;
  const EVENT_DISPLAY_WINDOW_MS = 300_000;
  const activity = [...state.recent_events].reverse()
    .filter((event) => (
      event.id === activeErrorEventId
      || clockNow - Date.parse(event.timestamp) <= EVENT_DISPLAY_WINDOW_MS
    ))
    .slice(0, 10);
  // The active-error summary can reference an event that has already been
  // evicted from the capped recent_events list (after 100 newer events).
  // Keep the pinned error inspectable in the stream by synthesizing its row
  // from the ErrorSummary itself when the raw event is no longer present.
  const activeErrorEventMissing = Boolean(
    activeErrorEventId && !activity.some((event) => event.id === activeErrorEventId),
  );
  if (activeErrorEventMissing && pinnedError) {
    activity.unshift({
      id: pinnedError.event_id ?? `evicted-error-${pinnedError.timestamp}`,
      type: `${pinnedError.component} error`,
      timestamp: pinnedError.timestamp,
      source: "Evicted from event stream — shown from the pinned error",
    });
    activity.splice(10);
  }
  const midiDetail = connectionViews.midi.detail;
  const timerDuration = state.timer.duration_seconds ?? state.playing_song?.duration_seconds ?? 0;
  const elapsedMilliseconds = state.timer.status === "running" && state.timer.started_at
    ? Math.max(0, clockNow - Date.parse(state.timer.started_at))
    : 0;
  // Stop keeps the remaining time: a stopped countdown shows the frozen value instead of blanking.
  const frozenRemaining = state.timer.status === "stopped" ? state.timer.remaining_seconds ?? null : null;
  const timerElapsed = frozenRemaining !== null
    ? Math.max(0, (state.playing_song?.duration_seconds ?? timerDuration) - frozenRemaining)
    : Math.min(timerDuration, Math.floor(elapsedMilliseconds / 1_000));
  const timerRemaining = frozenRemaining !== null
    ? frozenRemaining
    : Math.max(0, timerDuration - Math.ceil(elapsedMilliseconds / 1_000));
  const [panelDirty, setPanelDirty] = useState(false);
  const [leaveTarget, setLeaveTarget] = useState<ConnectionPanel | null | undefined>(undefined);
  const discardButton = useRef<HTMLButtonElement>(null);
  const discardOpen = leaveTarget !== undefined;
  // Focus Discard once per opening (after Dialog's own first-control focus). Not a callback ref: that re-ran on every render and pulled focus back.
  useEffect(() => {
    if (!discardOpen) return;
    const timer = window.setTimeout(() => discardButton.current?.focus(), 0);
    return () => window.clearTimeout(timer);
  }, [discardOpen]);
  // Leaving any setup panel with held (unsaved) changes asks first; everything else is immediate.
  const goTo = (target: ConnectionPanel | null) => {
    if (activeConnection !== null && target !== activeConnection && panelDirty) { setLeaveTarget(target); return; }
    setPanelDirty(false);
    setActiveConnection(target);
  };
  const toggleConnection = (connection: ConnectionPanel) => {
    if (!canConfigure) return;
    goTo(activeConnection === connection ? null : connection);
  };
  const closeConnection = () => goTo(null);
  const servicePlanEntries = [
    ...(plan?.songs.map((song) => ({
      kind: "song" as const,
      sequence: song.service_sequence ?? song.order * 1_000,
      song,
    })) ?? []),
    ...serviceLoad.skipped_items.map((item) => ({
      kind: "reference" as const,
      sequence: item.sequence,
      item,
    })),
  ].sort((left, right) => left.sequence - right.sequence);

  return (
    <main className="stagepilot-dashboard mx-auto min-h-screen max-w-[1680px] px-4 py-5 sm:px-6 lg:px-8">
      <header className="mb-5 grid grid-cols-[minmax(0,1fr)_minmax(0,auto)] items-center gap-x-3 gap-y-2 sm:grid-cols-[auto_minmax(12rem,1fr)_minmax(0,auto)] sm:gap-4">
        <div className="col-span-2 flex min-w-0 flex-wrap items-center gap-y-1 sm:col-span-1">
          <h1 className="relative z-10 -translate-y-4 shrink-0 select-none font-brand text-[4.05rem] leading-[0.56] text-ink">StagePilot</h1>
          {canConfigure && updater?.status === "available" && updater.availableVersion && (
            <UpdateAvailableButton
              onClick={updater.openConfirmation}
              ref={updateButton}
              version={updater.availableVersion}
            />
          )}
        </div>
        <div
          aria-atomic="true"
          aria-live="polite"
          className={`col-span-1 row-start-2 min-w-0 justify-self-start sm:col-span-1 sm:col-start-2 sm:row-start-1 sm:justify-self-end ${notification ? "" : "hidden sm:block"}`}
        >
          <div
            className={`ml-auto h-9 w-fit max-w-full truncate rounded-ds-lg border px-4 py-2 text-center text-sm transition-opacity ${notification ? `opacity-100 ${notification.tone === "error" ? "border-danger/25 bg-danger/10 text-danger-text" : "border-info/20 bg-info/10 text-info-text"}` : "invisible border-transparent opacity-0"}`}
            role="status"
            title={notification?.message}
          >
            {notification?.message ?? "\u00A0"}
          </div>
        </div>
        <div
          className="relative z-40 col-start-2 row-start-2 min-w-0 max-w-full justify-self-end sm:col-start-3 sm:row-start-1"
          ref={readinessHover.containerRef}
          {...readinessHover.hoverProps}
        >
          <button
            aria-describedby="system-readiness-popover"
            title={systemError ? `Error: ${readinessErrorCauses(checks).map((check) => check.detail ?? check.label).join("; ")}` : undefined}
            aria-expanded={readinessHover.open}
            className={`flex max-w-full min-w-0 cursor-help touch-manipulation items-center gap-2 break-words rounded-ds-full border px-3 py-1.5 font-type-badge text-type-badge font-bold uppercase tracking-wider text-shadow-label transition-colors duration-[140ms] motion-reduce:transition-none coarse:min-h-ds-touch-target ${ready ? "border-success-soft bg-success text-ink can-hover:bg-success/85" : systemError ? "border-danger/40 bg-surface-950 text-danger-text can-hover:bg-surface-900" : "border-warning/40 bg-surface-950 text-warning-text can-hover:bg-surface-900"}`}
            type="button"
          >
            <span className="min-w-0 text-left">{!canConfigure ? "Live status" : ready ? "Ready" : systemError ? `Error: ${readinessErrorCauses(checks)[0]?.label}` : "Check system"}</span>
          </button>
          <div
            className={`absolute right-0 top-full w-[min(22rem,calc(100vw-2rem))] pt-2 transition-[opacity,transform,visibility] motion-reduce:transition-none ${readinessHover.open ? "visible translate-y-0 opacity-100" : "pointer-events-none invisible translate-y-1 opacity-0"}`}
            id="system-readiness-popover"
            role="tooltip"
          >
            <div className="rounded-ds-xl border border-edge bg-surface-950/85 p-3 shadow-elevation-overlay backdrop-blur-xl">
              <ul className="grid gap-1.5">
                {checks.map((check) => (
                  <li
                    className="flex items-center gap-2 rounded-ds-lg bg-edge-subtle px-3 py-2 text-sm"
                    key={check.id}
                    title={check.detail}
                  >
                    <span className={`grid h-5 w-5 shrink-0 place-items-center rounded-full text-type-micro font-black ${check.status === "connected" ? "bg-success-soft/20 text-success-text" : check.status === "error" ? "bg-danger/15 text-danger-text" : "bg-edge text-ink-muted"}`}>
                      {check.status === "connected" ? "✓" : check.status === "error" ? "!" : "—"}
                    </span>
                    <span className={check.status === "connected" ? "text-success-text" : check.status === "error" ? "text-danger-text" : "text-ink-muted"}>
                      {check.label}
                    </span>
                    {!check.required && (
                      <span className="ml-auto text-type-micro font-bold uppercase tracking-wider text-ink-muted">
                        Optional
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          </div>
        </div>
      </header>


      {canConfigure && updater && (
        <UpdateDialog
          onCancel={updater.cancelConfirmation}
          onCloseError={updater.closeError}
          onConfirm={() => void updater.install()}
          onRetry={() => void updater.retry()}
          returnFocus={updateButton}
          updater={updater}
        />
      )}

      {canConfigure && FIRST_LAUNCH_SETUP_ENABLED && (
        <SetupChecklist
          playback={playback?.status}
          live={live}
          midi={midi}
          planningCenterStatus={planningCenterStatus}
          onOpen={(connection) => goTo(connection)}
          propresenter={propresenter}
          settings={settings}
          state={state}
        />
      )}

      {serviceLoad.status === "ambiguous" && (
        <section className="mb-5 rounded-xl border border-warning/30 bg-warning/10 p-4" aria-live="polite">
          <p className="font-type-kicker text-type-kicker uppercase text-warning-text">Service plan selection required</p>
          <h2 className="mt-1 text-lg font-bold text-ink">Multiple plans match {serviceLoad.target_date ?? "the service date"}</h2>
          {serviceLoad.message && <p className="mt-1 text-sm font-medium text-warning-text">{serviceLoad.message}</p>}
          <p className="mt-1 text-sm text-warning-text/70">
            {serviceLoad.is_stale ? "The previous service plan remains available but is marked stale. " : ""}
            {canOperate ? "Choose the service plan StagePilot should load." : "Waiting for an Operator to select the service plan."}
          </p>
          <div className="mt-3 grid gap-2 md:grid-cols-2">
            {serviceLoad.candidates.map((candidate) => (
              <div key={candidate.id} className="flex items-center justify-between gap-3 rounded-ds-lg border border-warning/15 bg-surface-950/30 px-3 py-3">
                <div className="min-w-0">
                  <p className="truncate font-semibold text-ink">{candidate.title}</p>
                  <p className="mt-0.5 text-xs text-ink-muted">{candidate.service_type_name} · {candidate.service_times.join(", ")}</p>
                </div>
                {canOperate && <button
                  aria-label={pendingPlanId === candidate.id ? `Loading ${candidate.title}` : `Use ${candidate.title}`}
                  className="shrink-0 touch-manipulation rounded-ds-lg bg-warning px-3 py-2 text-xs font-bold text-on-light transition-colors duration-[140ms] motion-reduce:transition-none can-hover:bg-warning-text disabled:cursor-wait disabled:opacity-50 coarse:min-h-ds-touch-target"
                  disabled={pendingPlanId !== null}
                  onClick={() => selectPlan(candidate.id)}
                  type="button"
                >
                  {pendingPlanId === candidate.id ? "Loading…" : "Use this service plan"}
                </button>}
              </div>
            ))}
          </div>
        </section>
      )}

      <section
        aria-label="Connections"
        className={`connections-status grid grid-cols-5 gap-1.5 sm:gap-3 ${statusCompact ? "status-compact" : ""} ${statusMotionPhase === "preparing" ? "status-preparing" : statusMotionPhase === "moving" ? "status-moving" : ""}`}
        ref={connectionsRow}
      >
        <StatusCard
          interactive={canConfigure}
          accessibleTitle="Services"
          active={activeConnection === "planning-center"}
          controls="planning-center-configuration"
          detail={connectionViews.planningCenter.detail}
          icon={connectionIcon(planningCenterIcon, "planning-center")}
          onClick={() => toggleConnection("planning-center")}
          status={connectionViews.planningCenter.status}
          title="Services"
          tone="services"
        />
        <StatusCard
          interactive={canConfigure}
          accessibleTitle="MIDI / Playback"
          active={activeConnection === "midi"}
          controls="midi-configuration"
          detail={midiDetail}
          icon={connectionIcon(multitracksIcon, "multitracks")}
          onClick={() => toggleConnection("midi")}
          status={connectionViews.midi.status}
          title="Playback"
          tone="playback"
        />
        <StatusCard
          interactive={canConfigure}
          accessibleTitle="Presentation"
          active={activeConnection === "propresenter"}
          controls="propresenter-configuration"
          detail={connectionViews.propresenter.detail}
          icon={connectionIcon(propresenterIcon, "propresenter", true)}
          onClick={() => toggleConnection("propresenter")}
          status={connectionViews.propresenter.status}
          title="Presentation"
          tone="presentation"
        />
        <StatusCard
          interactive={canConfigure}
          active={activeConnection === "lights"}
          controls="lighting-configuration"
          detail={connectionViews.lights.detail}
          icon={connectionIcon(lightingIcon, "lights", true)}
          onClick={() => toggleConnection("lights")}
          status={connectionViews.lights.status}
          title="Lighting"
          tone="lighting"
        />
        <StatusCard
          interactive={canConfigure}
          accessibleTitle="StagePilot"
          active={activeConnection === "backend"}
          controls="backend-configuration"
          detail={health ? `v${health.version} · state revision ${state.revision}` : "Connecting to local API"}
          icon={connectionIcon(stagepilotIcon, "stagepilot", true)}
          onClick={() => toggleConnection("backend")}
          status={backendStatus}
          title="Backend"
          tone="backend"
        />
      </section>

      {canConfigure && activeConnection === "planning-center" && (
        <PlanningCenterSetupPanel
          onDirtyChange={setPanelDirty}
          error={planningCenterError}
          onRetryLoad={retryLoads}
          message={planningCenterMessage}
          onClose={closeConnection}
          onDisconnectOAuth={disconnectPlanningCenter}
          onLoadServiceTypes={loadPlanningCenterServiceTypes}
          onReload={() => dispatch("reload_plan")}
          onSave={savePlanningCenter}
          onSelectPlan={selectPlan}
          onSignInOAuth={signInPlanningCenterOAuth}
          onTest={testPlanningCenterConnection}
          pendingAction={pendingAction}
          pendingOperation={pendingPlanningCenterOperation}
          pendingPlanId={pendingPlanId}
          serviceTypes={planningCenterServiceTypes}
          settings={settings}
          state={state}
          status={planningCenterStatus}
        />
      )}

      {canConfigure && activeConnection === "midi" && (
        <MidiSetupPanel
          playback={playback}
          error={midiError}
          onRetryLoad={retryLoads}
          message={midiMessage}
          midi={midi}
          messages={midiMessages}
          onClose={closeConnection}
          onDirtyChange={setPanelDirty}
          onRefresh={refreshMidi}
          onSelect={selectMidi}
          onSimulate={simulateMidi}
          onSaveSettings={saveMidiSettings}
          pendingCue={pendingMidiCue}
          pendingOperation={pendingMidiOperation}
          pendingSettingsSave={pendingSettingsOperation}
          settings={settings}
          settingsError={settingsError}
          settingsMessage={settingsMessage}
          songs={state?.plan?.songs ?? []}
        />
      )}

      <Dialog
        open={leaveTarget !== undefined}
        dismissible={false}
        tone="danger"
        title="Discard unsaved changes?"
        actions={
          <>
            <Button onClick={() => setLeaveTarget(undefined)} size="touch" variant="secondary">Keep editing</Button>
            {/* Dialog focuses the first control on open; the Discard button takes focus after that, as before. */}
            <Button ref={discardButton} onClick={() => { setPanelDirty(false); if (leaveTarget !== undefined) setActiveConnection(leaveTarget); setLeaveTarget(undefined); }} size="touch" variant="destructive">Discard</Button>
          </>
        }
      />

      {canConfigure && activeConnection === "propresenter" && (
        <ProPresenterSetupPanel
          onDirtyChange={setPanelDirty}
          error={propresenterError}
          onRetryLoad={retryLoads}
          message={propresenterMessage}
          onClose={closeConnection}
          onRefreshTimers={refreshProPresenter}
          onSave={saveProPresenter}
          onTest={runProPresenterTest}
          pendingOperation={pendingProPresenterOperation}
          propresenter={propresenter}
        />
      )}

      {canConfigure && activeConnection === "lights" && (
        <LightingSetupPanel
          onDirtyChange={setPanelDirty}
          connectionStatus={connectionViews.lights.status}
          error={lightingError}
          onRetryLoad={retryLoads}
          lights={lights}
          message={lightingMessage}
          onClearAllCues={clearAllLightingCues}
          onClose={closeConnection}
          onRefresh={refreshLighting}
          onSaveCues={saveLightingCues}
          onSaveSettings={saveLighting}
          onTest={sendLightingTest}
          pendingOperation={pendingLightingOperation}
          settings={settings}
          state={state}
        />
      )}

      {canConfigure && activeConnection === "backend" && (
        <BackendSetupPanel
          onDirtyChange={setPanelDirty}
          error={settingsError}
          onRetryLoad={retryLoads}
          health={health}
          live={live}
          message={settingsMessage}
          onClose={closeConnection}
          onSave={saveGeneralSettings}
          pending={pendingSettingsOperation}
          settings={settings}
          state={state}
        />
      )}

      <DashboardGrid
        canEditLayout={canConfigure}
        widgets={{
          "service-plan": (
        <Panel className="widget-autosize-target flex h-full min-h-0 flex-col overflow-hidden !p-0">
          <div className="flex flex-wrap items-center justify-between gap-3 p-4">
            <div className="min-w-0 max-w-full">
              <p className="text-type-kicker uppercase text-ink-muted">Service plan</p>
              <h2 className="mt-1 break-words text-lg font-bold text-ink [overflow-wrap:anywhere]">{plan?.title ?? "No service loaded"}</h2>
              <p className="mt-1 break-words text-xs text-ink-muted [overflow-wrap:anywhere]">
                {plan ? `${plan.service_type} · ${plan.date} · ${plan.service_times.join(", ")}` : "Waiting for a current or upcoming service plan."}
              </p>
            </div>
            <div className="text-right">
              <p className="text-sm font-semibold tabular-nums text-ink-body">{plan?.songs.length ?? 0} songs</p>
              <p className="text-xs tabular-nums text-ink-muted">
                {plan ? formatPlanCurrentAsOf(state.last_successful_plan_reload_at) : "Load time unavailable"}
              </p>
            </div>
          </div>
          {!servicePlanEntries.length && (
            <div className="border-t border-edge-subtle p-4">
              <p className="text-sm text-ink-muted">No songs in this service plan yet.</p>
              {canOperate && <Button className="mt-3" disabled={pendingAction !== null} onClick={() => dispatch("reload_plan")} variant="secondary">Reload service plan</Button>}
            </div>
          )}
          <ol aria-label="Service plan order" className="min-h-0 flex-1 overflow-hidden" data-autosize-content>
            {servicePlanEntries.map((entry) => entry.kind === "song"
              ? <SongRow key={`song-${entry.song.id}`} song={entry.song} current={entry.song.id === state.playing_song?.id} next={entry.song.id === state.up_next_song?.id} hoverWarmup={hoverPreviewWarmupRef.current} />
              : <ReferenceItemRow key={`reference-${entry.item.item_id}`} item={entry.item} hoverWarmup={hoverPreviewWarmupRef.current} />)}
          </ol>
        </Panel>
          ),
          "now-playing": (
          <Panel variant="live" className="widget-autosize-target h-full min-h-0 overflow-hidden !p-5 [container-type:inline-size] sm:!p-6">
            <div className="flex items-center justify-between gap-3">
              <p className="font-type-kicker text-type-kicker uppercase text-primary-text">Now playing · live signal</p>
              <span className={`rounded-ds-full px-2.5 py-1 font-type-micro text-type-micro uppercase ${state.timer.status === "running" ? "bg-success-soft/20 text-success-text" : state.timer.status === "error" ? "bg-danger/15 text-danger-text" : "bg-ink/5 text-ink-muted"}`}>Timer {state.timer.status}</span>
            </div>
            <h2 className="live-title-transition mt-7 min-h-9 text-[clamp(2rem,3.2vw,3.65rem)] font-black leading-[0.96] tracking-[-0.045em] text-ink" key={state.playing_song?.id ?? "idle"}>{state.playing_song?.title ?? "Waiting for first cue"}</h2>
            <div className="mt-2 flex items-end justify-between gap-5">
              <div>
                <p className="text-sm text-ink-muted">Time remaining</p>
                <p className="operational-timer mt-1 font-mono text-[clamp(3rem,5vw,5.4rem)] font-light leading-none">{formatDuration(timerRemaining)}</p>
              </div>
              <div className="elapsed-time-block pb-1 text-right">
                <p className="text-sm font-semibold text-ink-muted sm:text-base">Elapsed time</p>
                <p className="mt-1 font-mono text-[clamp(1.5rem,2.4cqw,2rem)] font-semibold leading-none tabular-nums text-primary-text">{formatDuration(timerElapsed)}</p>
                <p className="mt-1 text-xs font-medium tabular-nums text-ink-muted sm:text-sm">of {formatDuration(state.playing_song?.duration_seconds)}</p>
              </div>
            </div>
            <div className="now-playing-metadata mt-7 grid grid-cols-2 gap-4 border-t border-edge-subtle pt-4 text-sm">
              <div><p className="text-xs text-ink-muted">Position</p><p className="mt-1 font-semibold tabular-nums text-ink-body">{state.playing_song_index == null ? "Not started" : `${state.playing_song_index + 1} of ${plan?.songs.length ?? 0}`}</p></div>
              <div><p className="text-xs text-ink-muted">Up next</p><p className="mt-1 line-clamp-2 font-semibold text-ink">{state.up_next_song?.title ?? "End of service"}</p></div>
              <div><p className="text-xs text-ink-muted">Countdown started</p><p className="mt-1 font-semibold tabular-nums text-ink-body">{formatTime(state.timer.started_at)}</p></div>
              <div><p className="text-xs text-ink-muted">Last action</p><p className="mt-1 font-semibold capitalize text-ink-body">{state.last_action?.replaceAll("_", " ") ?? "None"}</p></div>
            </div>
          </Panel>
          ),
          "manual-controls": canOperate ? (
          <Panel className="widget-autosize-target manual-controls-panel h-full min-h-0 overflow-hidden">
            <p className="text-type-kicker uppercase text-ink-muted">Manual controls</p>
            {!live && (
              <InlineAlert className="mt-3" id="manual-controls-offline" tone="warning">
                {OFFLINE_CONTROLS_SENTENCE}
              </InlineAlert>
            )}
            <div className="manual-controls-grid mt-3 grid gap-2">
              <ActionButton action="start_next" label="Start next" tone="green" disabled={!live || pendingAction !== null} onAction={dispatch} />
              <ActionButton action="restart_current" label="Restart current" tone="green" disabled={!live || pendingAction !== null || !state.current_song} onAction={dispatch} />
              <ActionButton action="previous" label="Previous" tone="orange" disabled={!live || pendingAction !== null} onAction={dispatch} />
              <ActionButton action="reload_plan" label="Reload service plan" tone="blue" disabled={!live || pendingAction !== null} onAction={dispatch} />
              <ActionButton action="stop_timer" label="Stop timer" tone="red" disabled={!live || pendingAction !== null} onAction={dispatch} />
              <ActionButton action="reset_position" label="Reset position" tone="red" disabled={!live || pendingAction !== null} onAction={dispatch} />
            </div>
          </Panel>
          ) : null,
          events: (
        <Panel className="flex h-full min-h-0 flex-col overflow-hidden !p-0">
          <div className="flex items-center justify-between p-4"><p className="text-type-kicker uppercase text-ink-muted">Recent event stream</p><span className="text-xs tabular-nums text-idle">Latest {activity.length}</span></div>
          <div className="min-h-0 flex-1 overflow-auto overscroll-contain border-t border-edge-subtle">
            {pinnedError && <div className="grid grid-cols-[4.5rem_1fr] gap-3 border-b border-danger/15 bg-danger/10 px-4 py-2.5 text-xs"><time className="font-mono tabular-nums text-danger-text/70">{formatTime(pinnedError.timestamp)}</time><p className="text-danger-text"><span className="font-bold uppercase text-danger-text">{pinnedError.component}</span> · {pinnedError.message}</p></div>}
            {activity.map((event) => <div key={event.id} className="event-row grid grid-cols-[4.5rem_1fr] gap-3 border-b border-edge-subtle px-4 py-2.5 text-xs"><time className="font-mono tabular-nums text-idle">{formatTime(event.timestamp)}</time><p className="truncate text-ink-body"><span className="font-semibold text-ink-body">{event.type}</span> · {event.source}</p></div>)}
            {!activity.length && <p className="p-4 text-sm text-ink-muted">Waiting for demo events…</p>}
          </div>
        </Panel>
          ),
        }}
      />
    </main>
  );
}
