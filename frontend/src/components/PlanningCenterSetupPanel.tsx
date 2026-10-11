import { useEffect, useMemo, useRef, useState } from "react";

import { useDelayedHover } from "../hooks/useDelayedHover";
import { openExternalUrl } from "../desktop";
import type {
  ApplicationState,
  PlanningCenterServiceType,
  PlanningCenterSettingsInput,
  PlanningCenterStatusResponse,
  PlanningCenterTestInput,
  SettingsResponse,
} from "../types";
import { SetupPanelHeader } from "./SetupPanelHeader";
import { Button } from "./ui/Button";
import { Disclosure } from "./ui/Disclosure";
import { Field } from "./ui/Field";
import { InlineAlert } from "./ui/InlineAlert";
import { Input } from "./ui/Input";
import { Panel } from "./ui/Panel";
import { SaveBar, type SaveBarState } from "./ui/SaveBar";
import { Select } from "./ui/Select";
import { connectionWord } from "./ui/statusWord";

const ALL_SERVICE_TYPES_ID = "stagepilot:all-service-types";

type TimezoneOption = {
  city: string;
  offsetMinutes: number;
  timezone: string;
};

// One representative city for every populated civil standard-time offset.
// Labels stay fixed year-round; values are real IANA zones with their normal DST rules.
export const TIMEZONE_OPTIONS: readonly TimezoneOption[] = [
  { city: "Baker Island", offsetMinutes: -720, timezone: "Etc/GMT+12" },
  { city: "Pago Pago", offsetMinutes: -660, timezone: "Pacific/Pago_Pago" },
  { city: "Honolulu", offsetMinutes: -600, timezone: "Pacific/Honolulu" },
  { city: "Taiohae", offsetMinutes: -570, timezone: "Pacific/Marquesas" },
  { city: "Anchorage", offsetMinutes: -540, timezone: "America/Anchorage" },
  { city: "Los Angeles", offsetMinutes: -480, timezone: "America/Los_Angeles" },
  { city: "Denver", offsetMinutes: -420, timezone: "America/Denver" },
  { city: "Phoenix", offsetMinutes: -420, timezone: "America/Phoenix" },
  { city: "Mexico City", offsetMinutes: -360, timezone: "America/Mexico_City" },
  { city: "New York", offsetMinutes: -300, timezone: "America/New_York" },
  { city: "Santiago", offsetMinutes: -240, timezone: "America/Santiago" },
  { city: "St. John’s", offsetMinutes: -210, timezone: "America/St_Johns" },
  { city: "São Paulo", offsetMinutes: -180, timezone: "America/Sao_Paulo" },
  { city: "Vila dos Remédios", offsetMinutes: -120, timezone: "America/Noronha" },
  { city: "Praia", offsetMinutes: -60, timezone: "Atlantic/Cape_Verde" },
  { city: "London", offsetMinutes: 0, timezone: "Europe/London" },
  { city: "Lagos", offsetMinutes: 60, timezone: "Africa/Lagos" },
  { city: "Cairo", offsetMinutes: 120, timezone: "Africa/Cairo" },
  { city: "Istanbul", offsetMinutes: 180, timezone: "Europe/Istanbul" },
  { city: "Tehran", offsetMinutes: 210, timezone: "Asia/Tehran" },
  { city: "Dubai", offsetMinutes: 240, timezone: "Asia/Dubai" },
  { city: "Kabul", offsetMinutes: 270, timezone: "Asia/Kabul" },
  { city: "Karachi", offsetMinutes: 300, timezone: "Asia/Karachi" },
  { city: "Delhi", offsetMinutes: 330, timezone: "Asia/Kolkata" },
  { city: "Kathmandu", offsetMinutes: 345, timezone: "Asia/Kathmandu" },
  { city: "Dhaka", offsetMinutes: 360, timezone: "Asia/Dhaka" },
  { city: "Yangon", offsetMinutes: 390, timezone: "Asia/Yangon" },
  { city: "Jakarta", offsetMinutes: 420, timezone: "Asia/Jakarta" },
  { city: "Shanghai", offsetMinutes: 480, timezone: "Asia/Shanghai" },
  { city: "Eucla", offsetMinutes: 525, timezone: "Australia/Eucla" },
  { city: "Tokyo", offsetMinutes: 540, timezone: "Asia/Tokyo" },
  { city: "Adelaide", offsetMinutes: 570, timezone: "Australia/Adelaide" },
  { city: "Sydney", offsetMinutes: 600, timezone: "Australia/Sydney" },
  { city: "Lord Howe Island", offsetMinutes: 630, timezone: "Australia/Lord_Howe" },
  { city: "Nouméa", offsetMinutes: 660, timezone: "Pacific/Noumea" },
  { city: "Auckland", offsetMinutes: 720, timezone: "Pacific/Auckland" },
  { city: "Waitangi", offsetMinutes: 765, timezone: "Pacific/Chatham" },
  { city: "Apia", offsetMinutes: 780, timezone: "Pacific/Apia" },
  { city: "London, Kiribati", offsetMinutes: 840, timezone: "Pacific/Kiritimati" },
];

const formatOffset = (offsetMinutes: number) => {
  const sign = offsetMinutes < 0 ? "-" : "+";
  const absoluteMinutes = Math.abs(offsetMinutes);
  const hours = Math.floor(absoluteMinutes / 60);
  const minutes = absoluteMinutes % 60;
  return `UTC${sign}${hours}${minutes ? `:${minutes.toString().padStart(2, "0")}` : ""}`;
};

const offsetAt = (timezone: string, date: Date) => {
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: timezone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hourCycle: "h23",
  }).formatToParts(date);
  const values = Object.fromEntries(parts.map((part) => [part.type, part.value]));
  const representedAsUtc = Date.UTC(
    Number(values.year),
    Number(values.month) - 1,
    Number(values.day),
    Number(values.hour),
    Number(values.minute),
    Number(values.second),
  );
  return Math.round((representedAsUtc - date.getTime()) / 60_000);
};

const standardOffsetForTimezone = (timezone: string) => {
  try {
    const january = offsetAt(timezone, new Date("2024-01-15T12:00:00Z"));
    const july = offsetAt(timezone, new Date("2024-07-15T12:00:00Z"));
    return Math.min(january, july);
  } catch {
    return null;
  }
};

const systemTimezone = () => {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch {
    return "UTC";
  }
};

const formatTimestamp = (value: string | null) =>
  value ? new Date(value).toLocaleString() : "Not yet";

export function PlanningCenterSetupPanel({
  state,
  settings,
  status,
  serviceTypes,
  error,
  message,
  pendingOperation,
  pendingAction,
  pendingPlanId,
  onClose,
  onTest,
  onLoadServiceTypes,
  onSave,
  onReload,
  onSelectPlan,
  onSignInOAuth,
  onDisconnectOAuth,
}: {
  state: ApplicationState;
  settings: SettingsResponse | null;
  status: PlanningCenterStatusResponse | null;
  serviceTypes: PlanningCenterServiceType[];
  error: string | null;
  message: string | null;
  pendingOperation: "test" | "load-types" | "save" | "oauth-sign-in" | "oauth-disconnect" | null;
  pendingAction: string | null;
  pendingPlanId: string | null;
  onClose: () => void;
  onTest: (input: PlanningCenterTestInput) => void;
  onLoadServiceTypes: () => void;
  onSave: (
    input: PlanningCenterSettingsInput,
    timezone: string,
  ) => void;
  onReload: () => void;
  onSelectPlan: (planId: string) => void;
  onSignInOAuth?: () => void;
  onDisconnectOAuth?: () => void;
}) {
  const serviceLoad = state.service_load;
  const publicSettings = settings?.settings.planning_center;
  const [appId, setAppId] = useState("");
  const [secret, setSecret] = useState("");
  const [serviceTypeId, setServiceTypeId] = useState("");
  const [timezone, setTimezone] = useState(systemTimezone);
  const [titlePreference, setTitlePreference] = useState("");
  const [preferredTime, setPreferredTime] = useState("");
  const [removeSecret, setRemoveSecret] = useState(false);
  const applicationIdHelp = useDelayedHover();

  const connectionMethod = status?.connection_method ?? "manual";
  const oauthConnected = status?.oauth_connected ?? false;
  const oauthNeedsReconnect = status?.oauth_needs_reconnect ?? false;
  const [manualConnectionOpen, setManualConnectionOpen] = useState(false);

  useEffect(() => {
    if (!settings) return;
    setAppId(settings.settings.planning_center.app_id ?? "");
    setServiceTypeId(settings.settings.planning_center.service_type_id ?? "");
    const savedTimezone = settings.settings.timezone || systemTimezone();
    setTimezone(savedTimezone);
    setTitlePreference(settings.settings.planning_center.plan_title_preference ?? "");
    setPreferredTime(settings.settings.planning_center.preferred_service_time ?? "");
  }, [settings]);

  const selectedServiceTypeKnown = serviceTypeId === ALL_SERVICE_TYPES_ID
    || serviceTypes.some((value) => value.id === serviceTypeId);
  const valid = useMemo(
    () => Boolean(
      (connectionMethod === "oauth" || appId.trim())
      && serviceTypeId
      && timezone.trim(),
    ),
    [appId, connectionMethod, serviceTypeId, timezone],
  );
  const busy = pendingOperation !== null;

  // Held changes: fields that differ from the saved values. Nothing here saves on change.
  const saved = settings?.settings;
  const savedPc = saved?.planning_center;
  const savedTimezone = saved ? saved.timezone || systemTimezone() : null;
  const held = {
    appId: Boolean(savedPc) && appId.trim() !== (savedPc?.app_id ?? "").trim(),
    secret: secret !== "",
    removeSecret,
    serviceType: Boolean(savedPc) && serviceTypeId !== (savedPc?.service_type_id ?? ""),
    timezone: savedTimezone !== null && timezone !== savedTimezone,
    title: Boolean(savedPc) && titlePreference.trim() !== (savedPc?.plan_title_preference ?? "").trim(),
    time: Boolean(savedPc) && preferredTime !== (savedPc?.preferred_service_time ?? ""),
  };
  const changeCount = Object.values(held).filter(Boolean).length;
  const [saveFailed, setSaveFailed] = useState(false);
  const wasSaving = useRef(false);
  useEffect(() => {
    const saving = pendingOperation === "save";
    if (wasSaving.current && !saving) setSaveFailed(Boolean(error));
    wasSaving.current = saving;
  }, [pendingOperation, error]);
  const saveState: SaveBarState = pendingOperation === "save"
    ? { kind: "saving" }
    : saveFailed && changeCount > 0
      ? { kind: "error", message: "Couldn't save" }
      : changeCount > 0
        ? { kind: "held", count: changeCount }
        : { kind: "clean" };
  const blockedReason = changeCount > 0 && !valid
    ? "Add an Application ID (or sign in), choose a service type and a timezone before saving."
    : null;

  const discard = () => {
    if (!settings) return;
    setAppId(settings.settings.planning_center.app_id ?? "");
    setServiceTypeId(settings.settings.planning_center.service_type_id ?? "");
    setTimezone(settings.settings.timezone || systemTimezone());
    setTitlePreference(settings.settings.planning_center.plan_title_preference ?? "");
    setPreferredTime(settings.settings.planning_center.preferred_service_time ?? "");
    setSecret("");
    setRemoveSecret(false);
    setSaveFailed(false);
  };
  const manualSummary = `${connectionMethod === "oauth" ? "Sign-in" : "App ID and Secret"} · ${settings?.settings.planning_center.app_id?.trim() ? `App ID ${settings.settings.planning_center.app_id.trim()}` : "App ID not set"}`;

  const testInput = (): PlanningCenterTestInput => ({
    ...(appId.trim() ? { app_id: appId.trim() } : {}),
    ...(secret ? { secret } : {}),
  });

  const save = () => {
    if (!publicSettings || !valid || busy || (changeCount === 0 && state.plugins.planning_center)) return;
    setSaveFailed(false);
    onSave(
      {
        app_id: appId.trim(),
        service_type_id: serviceTypeId,
        plan_title_preference: titlePreference.trim() || null,
        preferred_service_time: preferredTime || null,
        upcoming_lookahead_days: publicSettings.upcoming_lookahead_days,
        request_timeout_seconds: publicSettings.request_timeout_seconds,
        ...(secret ? { secret } : {}),
        ...(removeSecret ? { remove_secret: true } : {}),
      },
      timezone.trim(),
    );
    setSecret("");
  };

  return (
    <Panel
      aria-busy={busy}
      aria-labelledby="planning-center-setup-heading"
      className="mt-5"
      id="planning-center-configuration"
      variant="setup"
    >
      <SetupPanelHeader
        closeLabel="Close Services configuration"
        description="Sign in with Planning Center, or manage a manual connection, to keep the Service plan updated."
        headingId="planning-center-setup-heading"
        onClose={onClose}
        status={status?.connection_status ?? state.planning_center_status}
        title="Services"
      />

      <h3 className="mt-5 font-semibold text-ink">Basic</h3>

      {oauthNeedsReconnect && (
        <InlineAlert className="mt-4 px-4 py-3" tone="warning">
          <p className="font-semibold">Your Services connection has expired</p>
          <p className="mt-1">
            Sign in again to keep loading your Service plan from Planning Center.
          </p>
          <Button
            className="mt-3"
            disabled={busy || !onSignInOAuth}
            onClick={onSignInOAuth}
            tone="warning"
            variant="tinted"
          >
            {pendingOperation === "oauth-sign-in" ? "Reconnecting…" : "Reconnect to Planning Center"}
          </Button>
        </InlineAlert>
      )}

      {connectionMethod === "oauth" && oauthConnected && !oauthNeedsReconnect && (
        <InlineAlert className="mt-4 px-4 py-3" tone="success">
          <p className="font-semibold">Connected to Planning Center</p>
          <p className="mt-1">
            Signed in with Planning Center. Your credentials never leave Planning Center&apos;s servers.
          </p>
          <Button
            className="mt-3"
            disabled={busy || !onDisconnectOAuth}
            onClick={onDisconnectOAuth}
            variant="secondary"
          >
            {pendingOperation === "oauth-disconnect" ? "Disconnecting…" : "Disconnect"}
          </Button>
        </InlineAlert>
      )}

      {!(connectionMethod === "oauth" && oauthConnected) && !oauthNeedsReconnect && (
        <InlineAlert className="mt-4 px-4 py-3" tone="info">
          <p className="font-semibold">Sign in with Planning Center</p>
          <p className="mt-1">
            The fastest way to connect: sign in with your Planning Center account, no App ID or
            Secret required.
          </p>
          <Button
            className="mt-3"
            disabled={busy || !onSignInOAuth}
            onClick={onSignInOAuth}
            variant="primary"
          >
            {pendingOperation === "oauth-sign-in" ? "Signing in…" : "Sign in with Planning Center"}
          </Button>
        </InlineAlert>
      )}

      <Disclosure
        className="mt-5"
        onOpenChange={setManualConnectionOpen}
        open={manualConnectionOpen}
        summary={manualSummary}
        title="Manual API Connection"
        tone="services"
        variant="advanced"
      >
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          <div className="text-sm text-ink-body">
            <div className="mb-1.5 flex items-center gap-1.5">
              <label
                className="font-type-text-strong text-type-text-strong text-ink"
                htmlFor="planning-center-application-id"
              >
                Application ID
              </label>
              <div
                className="relative"
                ref={applicationIdHelp.containerRef}
                {...applicationIdHelp.hoverProps}
              >
                <button
                  aria-describedby="planning-center-pat-help"
                  aria-label="How to get a Planning Center Personal Access Token"
                  className="relative grid h-4 w-4 touch-manipulation place-items-center rounded-ds-full border border-ink-muted pb-px text-type-micro font-black leading-none normal-case tracking-normal text-ink-muted transition-colors duration-[140ms] after:absolute after:left-1/2 after:top-1/2 after:hidden after:size-ds-touch-target after:-translate-x-1/2 after:-translate-y-1/2 after:content-[''] coarse:after:block can-hover:border-integration-services can-hover:text-integration-services-text motion-reduce:transition-none"
                  type="button"
                >
                  ?
                </button>
                <div
                  className={`absolute left-0 top-full z-50 w-[min(22rem,calc(100vw-3rem))] pt-2 text-left text-sm font-normal normal-case tracking-normal transition-[opacity,transform,visibility] motion-reduce:transition-none ${applicationIdHelp.open ? "visible translate-y-0 opacity-100" : "pointer-events-none invisible translate-y-1 opacity-0"}`}
                  id="planning-center-pat-help"
                  role="tooltip"
                >
                  <div className="rounded-ds-xl border border-edge-medium bg-surface-950 p-4 text-ink-body shadow-elevation-overlay">
                    <p className="font-semibold text-ink">Connect your Planning Center account</p>
                    <p className="mt-2 leading-relaxed">
                      Open Planning Center&apos;s{" "}
                      <a
                        className="font-semibold text-integration-services-text underline decoration-integration-services/50 underline-offset-2 can-hover:text-ink"
                        href="https://api.planningcenteronline.com/personal_access_tokens"
                        onClick={(event) => {
                          event.preventDefault();
                          void openExternalUrl(event.currentTarget.href);
                        }}
                        rel="noreferrer"
                        target="_blank"
                      >
                        Personal Access Tokens page
                      </a>
                      , create a new token, and give it a recognizable name. Copy
                      its Client ID into Application ID and its Secret into the
                      Secret field. Save the settings, then load your service types
                      to keep StagePilot&apos;s Service plan updated.
                    </p>
                  </div>
                </div>
              </div>
            </div>
            <Input
              autoComplete="username"
              disabled={busy}
              id="planning-center-application-id"
              onChange={(event) => setAppId(event.target.value)}
              value={appId}
            />
          </div>
          <Field heldChange={held.secret} label="Secret">
            <Input
              autoComplete="current-password"
              disabled={busy || removeSecret}
              onChange={(event) => setSecret(event.target.value)}
              placeholder={status?.planning_center_secret_saved ? "Saved securely — leave blank to keep" : "Enter PAT secret"}
              type="password"
              value={secret}
            />
          </Field>
          <label className="flex min-h-ds-touch-target touch-manipulation items-center gap-2 self-end rounded-ds-lg bg-surface-900/60 px-3 py-2.5 text-sm text-ink-body">
            <input
              checked={removeSecret}
              className="accent-primary"
              disabled={busy || !status?.planning_center_secret_saved}
              onChange={(event) => {
                setRemoveSecret(event.target.checked);
                if (event.target.checked) setSecret("");
              }}
              type="checkbox"
            />
            Remove saved secret
          </label>
        </div>

        {!(connectionMethod === "oauth" && oauthConnected) && (
          <div className="mt-5 flex flex-wrap gap-2">
            <Button
              disabled={busy || (!secret && !status?.planning_center_secret_saved)}
              onClick={() => onTest(testInput())}
              tone="services"
              variant="tinted"
            >
              {pendingOperation === "test" ? "Testing…" : "Test connection"}
            </Button>
          </div>
        )}
      </Disclosure>

      <div className="mt-5 border-t border-edge-subtle pt-4">
        <h3 className="text-sm font-bold uppercase tracking-wider text-ink-muted">
          Service plan configuration
        </h3>
        <div className="mt-4 grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          <Field heldChange={held.serviceType} label="Service type">
            <Select
              disabled={busy || serviceTypes.length === 0}
              onChange={(event) => setServiceTypeId(event.target.value)}
              value={serviceTypeId}
            >
              <option value="">Load and choose a service type</option>
              <option value={ALL_SERVICE_TYPES_ID}>All service types (nearest upcoming service plan)</option>
              {serviceTypeId && !selectedServiceTypeKnown && (
                <option value={serviceTypeId}>Saved service type ({serviceTypeId})</option>
              )}
              {serviceTypes.map((serviceType) => (
                <option key={serviceType.id} value={serviceType.id}>{serviceType.name}</option>
              ))}
            </Select>
          </Field>
          <Field heldChange={held.timezone} label="Timezone">
            <Select
              disabled={busy}
              onChange={(event) => {
                setTimezone(event.target.value);
              }}
              value={timezone}
            >
              {timezone && !TIMEZONE_OPTIONS.some((option) => option.timezone === timezone) && (
                <option value={timezone}>{timezone} ({formatOffset(standardOffsetForTimezone(timezone) ?? 0)})</option>
              )}
              {TIMEZONE_OPTIONS.map((option) => (
                <option key={option.timezone} value={option.timezone}>
                  {option.city} ({formatOffset(option.offsetMinutes)})
                </option>
              ))}
            </Select>
          </Field>
          <Field heldChange={held.title} label="Service plan title preference">
            <Input
              disabled={busy}
              onChange={(event) => setTitlePreference(event.target.value)}
              placeholder="Optional, for example Sunday Morning"
              value={titlePreference}
            />
          </Field>
          <Field heldChange={held.time} label="Preferred service time">
            <Input
              className="tabular-nums"
              disabled={busy}
              onChange={(event) => setPreferredTime(event.target.value)}
              type="time"
              value={preferredTime}
            />
          </Field>
        </div>

        <div className="mt-5 flex flex-wrap gap-2">
          <Button
            disabled={busy || !(status?.oauth_connected || status?.planning_center_secret_saved)}
            onClick={onLoadServiceTypes}
            tone="services"
            variant="tinted"
          >
            {pendingOperation === "load-types" ? "Loading…" : "Load service types"}
          </Button>
          <Button
            disabled={pendingAction !== null || !state.plugins.planning_center}
            onClick={onReload}
            tone="services"
            variant="tinted"
          >
            {pendingAction === "reload_plan" ? "Loading…" : "Load today’s service plan"}
          </Button>
        </div>
        {blockedReason && <p className="mt-4 text-sm text-ink-body" id="planning-center-save-blocked">{blockedReason}</p>}
        <SaveBar className="mt-5" onDiscard={discard} onRetry={save} onSave={save} state={saveState} />
      </div>

      {(error || message) && (
        <InlineAlert className="mt-4" tone={error ? "danger" : "success"}>
          {error ?? message}
        </InlineAlert>
      )}
      {!state.plugins.planning_center && (
        <p className="mt-3 text-xs text-warning-text">
          Saving this configuration enables Services. Restart StagePilot to start the connection.
        </p>
      )}

      <dl className="mt-5 grid gap-x-4 gap-y-3 border-t border-edge-subtle pt-4 text-sm sm:grid-cols-2 xl:grid-cols-4">
        <div>
          <dt className="text-xs uppercase tracking-wider text-ink-muted">Connection</dt>
          <dd className="mt-1 text-ink-body">{connectionWord[status?.connection_status ?? state.planning_center_status]}</dd>
        </div>
        <div>
          <dt className="text-xs uppercase tracking-wider text-ink-muted">Credential</dt>
          <dd className="mt-1 text-ink-body">{status?.planning_center_secret_saved ? "Saved securely" : "Not saved"}</dd>
        </div>
        <div>
          <dt className="text-xs uppercase tracking-wider text-ink-muted">Target date</dt>
          <dd className="mt-1 text-ink-body tabular-nums">{serviceLoad.target_date ?? "Not selected"}</dd>
        </div>
        <div>
          <dt className="text-xs uppercase tracking-wider text-ink-muted">Last successful sync</dt>
          <dd className="mt-1 text-ink-body tabular-nums">{formatTimestamp(state.last_successful_plan_reload_at)}</dd>
        </div>
      </dl>

      {serviceLoad.candidates.length > 0 && (
        <div className="mt-5 border-t border-edge-subtle pt-4">
          <p className="text-xs font-bold uppercase tracking-wider text-warning-text">Matching service plans</p>
          <div className="mt-2 grid gap-2 md:grid-cols-2">
            {serviceLoad.candidates.map((candidate) => (
              <div className="flex items-center justify-between gap-3 rounded-ds-lg bg-surface-900/60 px-3 py-2" key={candidate.id}>
                <div className="min-w-0">
                  <p className="truncate font-semibold text-ink">{candidate.title}</p>
                  <p className="text-xs text-ink-muted">{candidate.service_type_name} · {candidate.service_times.join(", ")}</p>
                </div>
                <Button
                  className="shrink-0"
                  disabled={pendingPlanId !== null}
                  onClick={() => onSelectPlan(candidate.id)}
                  variant="primary"
                >
                  {pendingPlanId === candidate.id ? "Loading…" : "Use service plan"}
                </Button>
              </div>
            ))}
          </div>
        </div>
      )}
    </Panel>
  );
}
