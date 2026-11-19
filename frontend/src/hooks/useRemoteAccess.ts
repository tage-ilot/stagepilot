import { useEffect, useRef, useState } from "react";
import { useDashboardAccess } from "../access/AccessContext";
import { invalidateAccess } from "../access/accessState";
import {
  ApiError, getRemoteStatus, getRemoteUsers, setRemoteEnabled,
  type RemoteStatus, type RemoteUser,
} from "../api";
import { setRemoteAutostart } from "../desktop";

export function friendlyRemoteError(cause: unknown): string {
  if (cause instanceof ApiError) {
    if (cause.status === 401 || cause.status === 403) return "Your access has changed. Please sign in again.";
    if (cause.status === 409) return "The change could not be applied. Keep at least one enabled Operator and use a unique email. Refresh before trying again.";
    if (cause.status === 422) return "Use a valid email and a password of at least 12 characters.";
    if (cause.status === 429) return "Too many requests. Please wait before trying again.";
    // The backend already translates enable/regenerate failures (enrollment
    // limit, unreachable control plane, revoked credential, etc.) into a
    // short, specific, end-user-safe message -- surface it verbatim rather
    // than collapsing everything to the generic fallback below.
    if (cause.status === 503 && cause.message) return cause.message;
  }
  return "Remote access is unavailable. Local StagePilot is unaffected. Try again shortly.";
}

/**
 * Single source of truth for Remote Access status and the enable/disable
 * lifecycle. Shared between the BackendSetupPanel checkbox (which owns
 * enabling/disabling and expand/collapse) and RemoteAccessPanel (which
 * renders status, users, and link management using the same state so the
 * visual checkbox state and the real enabled state never diverge).
 */
// A single momentary `credential_available: false` reading (e.g. the very
// first native Keychain/Credential Manager broker read after a backend
// restart racing an in-flight system password prompt) self-resolves within
// a poll cycle or two and must never be reported to the user as a
// permanent revocation -- see t_460cf8dd. Only a run of consecutive false
// readings across this many 2s polls is treated as a genuine, persistent
// problem worth surfacing with the alarming "revoked, contact support"
// wording; anything shorter renders no credential banner at all.
const CREDENTIAL_UNAVAILABLE_STREAK_THRESHOLD = 3;

export function useRemoteAccess() {
  const access = useDashboardAccess();
  const [status, setStatus] = useState<RemoteStatus | null>(null);
  const [users, setUsers] = useState<RemoteUser[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmDisable, setConfirmDisable] = useState(false);
  const [checkboxBusy, setCheckboxBusy] = useState(false);
  const [credentialWarning, setCredentialWarning] = useState(false);
  const alive = useRef(true);
  const inFlight = useRef(false);
  const credentialUnavailableStreak = useRef(0);
  const canManage = access.authenticated && access.capabilities.canConfigure;
  const local = access.mode !== "remote";

  useEffect(() => {
    alive.current = true;
    if (!canManage) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      if (!inFlight.current) {
        try {
          const next = await getRemoteStatus();
          if (!cancelled) {
            setStatus(next);
            if (next.provisioned && !next.credential_available) {
              credentialUnavailableStreak.current += 1;
              if (credentialUnavailableStreak.current >= CREDENTIAL_UNAVAILABLE_STREAK_THRESHOLD) {
                setCredentialWarning(true);
              }
            } else {
              credentialUnavailableStreak.current = 0;
              setCredentialWarning(false);
            }
          }
        } catch (cause) {
          if (!cancelled) { setStatus(null); setError(friendlyRemoteError(cause)); }
        }
      }
      if (!cancelled) timer = setTimeout(() => void poll(), 2000);
    };
    void poll();
    void getRemoteUsers().then((next) => { if (!cancelled) setUsers(next); }).catch((cause) => {
      if (!cancelled) setError(friendlyRemoteError(cause));
    });
    return () => { cancelled = true; alive.current = false; clearTimeout(timer); };
  }, [canManage]);

  async function run(work: () => Promise<unknown>, selfChange = false) {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true); setError(null); setNotice(null);
    try {
      await work();
      if (selfChange && !local) { invalidateAccess(); return; }
      const [next, nextUsers] = await Promise.all([getRemoteStatus(), getRemoteUsers()]);
      if (alive.current) { setStatus(next); setUsers(nextUsers); setNotice("Changes saved."); }
    } catch (cause) {
      if (alive.current) setError(friendlyRemoteError(cause));
      throw cause;
    } finally {
      inFlight.current = false;
      if (alive.current) setBusy(false);
    }
  }

  /** The checkbox was checked. Always enables Remote immediately, even when
   * there is no Operator yet -- nothing on the backend treats "no Operator"
   * as open access, so starting the tunnel with zero users configured is
   * safe. The "Create first Operator" step (see `bootstrap` below) happens
   * as a required follow-up once Remote is running, not a precondition.
   * Never marks the checkbox checked prematurely -- it stays reflecting
   * status.enabled, set only once the enable call actually succeeds. */
  function requestEnable() {
    if (status?.enabled) return;
    setCheckboxBusy(true);
    void run(async () => {
      await setRemoteEnabled(true);
      await setRemoteAutostart(true);
    }).catch(() => undefined).finally(() => setCheckboxBusy(false));
  }

  /** The checkbox was unchecked. Opens the disable confirmation; the real
   * disable only happens once the user confirms. */
  function requestDisable() {
    if (!status?.enabled) return;
    setConfirmDisable(true);
  }

  function cancelDisable() {
    setConfirmDisable(false);
  }

  function confirmDisableAccept() {
    setConfirmDisable(false);
    setCheckboxBusy(true);
    void run(async () => {
      await setRemoteEnabled(false);
      await setRemoteAutostart(false);
    }, true).catch(() => undefined).finally(() => setCheckboxBusy(false));
  }

  const enabled = Boolean(status?.enabled);
  // Once Remote is enabled but there is still no Operator, the first-Operator
  // form is a required next step (local device only -- a Remote session
  // can't bootstrap itself). This replaces the old pre-enable bootstrap
  // gate: it's now derived from live status rather than a one-shot flag set
  // by requestEnable, so it stays visible until an Operator actually exists
  // and reappears automatically if that ever regresses.
  const bootstrap = local && enabled && Boolean(status?.needs_operator);
  // The setup panel should stay expanded whenever Remote Access is actually
  // enabled, while a toggle is in flight, or while the confirm/bootstrap
  // dialogs need to be visible -- collapsing those would hide them.
  const panelOpen = enabled || checkboxBusy || confirmDisable || bootstrap;

  return {
    access, status, users, setUsers, error, setError, notice, setNotice,
    busy, bootstrap, confirmDisable, checkboxBusy, credentialWarning,
    canManage, local, run,
    enabled, panelOpen,
    requestEnable, requestDisable, cancelDisable, confirmDisableAccept,
  };
}

export type RemoteAccessControl = ReturnType<typeof useRemoteAccess>;
