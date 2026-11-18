import { useCallback, useEffect, useRef, useState } from "react";

import { endLightkeyHelperInstall, startLightkeyHelperInstall } from "../api";
import { describeError } from "../errorText";
import type { LightkeyHelperInstall, LightkeyStatus } from "../types";
import { countdownText, helperIsOutdated } from "./lightkeyProject";
import { CONTROL_34 } from "./compactControl";
import { Button } from "./ui/Button";

/** The box's id, so the strip's setup button can say what it controls. */
export const LIGHTKEY_INSTALL_ID = "lightkey-install-box";

type Phase = "idle" | "starting" | "ready" | "expired" | "error" | "done";

/**
 * "Set up the Lighting computer", in the Lightkey strip. Shown while there is no connected Lighting
 * computer, and as "Update the helper" when the helper that answers is older than the one StagePilot
 * carries. StagePilot makes a one-time command; pasted into Terminal on the Lighting Mac it fetches
 * the matching helper from StagePilot itself (this network only, 30 minutes). The box ends the
 * session by itself when the helper answers.
 */
export function LightkeyInstallBox({ status, autoStart = false }: { status: LightkeyStatus; autoStart?: boolean }) {
  const connected = status.link === "connected";
  const outdated = helperIsOutdated(status);
  const [phase, setPhase] = useState<Phase>("idle");
  const [session, setSession] = useState<LightkeyHelperInstall | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [left, setLeft] = useState(0);
  const [copied, setCopied] = useState(false);
  const startedConnected = useRef(false);
  // Opened on purpose (autoStart) while everything already works: the command for another Lighting computer, or again here.
  const manual = useRef(false);
  const autoRan = useRef(false);
  const alive = useRef(true);
  const current = connected && !outdated;
  const heading = current ? "Lighting computer command" : connected ? "Update the helper" : "Set up the Lighting computer";

  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);

  const start = useCallback(async () => {
    setPhase("starting");
    setError(null);
    setCopied(false);
    try {
      const made = await startLightkeyHelperInstall();
      if (!alive.current) return;
      startedConnected.current = connected;
      manual.current = connected && !outdated;
      setSession(made);
      setLeft(Date.parse(made.expires_at) - Date.now());
      setPhase("ready");
    } catch (cause) {
      if (!alive.current) return;
      setError(describeError(cause, "Could not make the setup command."));
      setPhase("error");
    }
  }, [connected, outdated]);

  // Opened on purpose: make the command at once, once per opening (Cancel leaves it cancelled).
  useEffect(() => {
    if (!autoStart) { autoRan.current = false; return; }
    if (autoRan.current || phase !== "idle") return;
    autoRan.current = true;
    void start();
  }, [autoStart, phase, start]);

  // The countdown. One tick a second; it is text only (not announced every second).
  useEffect(() => {
    if (phase !== "ready" || !session) return undefined;
    const expires = Date.parse(session.expires_at);
    const tick = () => {
      const remaining = expires - Date.now();
      setLeft(remaining);
      if (remaining <= 0) setPhase("expired");
    };
    tick();
    const timer = window.setInterval(tick, 1000);
    return () => window.clearInterval(timer);
  }, [phase, session]);

  // The helper answers: the box is done and the session ends (the backend ends it too).
  useEffect(() => {
    // A command made on purpose while it already works just runs until it expires or is cancelled.
    if (phase !== "ready" || manual.current) return;
    const finished = startedConnected.current ? connected && !outdated : connected;
    if (!finished) return;
    setPhase("done");
    void endLightkeyHelperInstall().catch(() => undefined);
  }, [phase, connected, outdated]);

  const copy = async () => {
    if (!session) return;
    try {
      await navigator.clipboard.writeText(session.command);
      setCopied(true);
    } catch {
      setCopied(false);
      setError("Copy did not work here. Select the command and copy it by hand.");
    }
  };

  // Closing the box (the strip's button) ends a command that was made on purpose.
  useEffect(() => {
    if (autoStart || !manual.current || phase === "idle") return;
    manual.current = false;
    setPhase("idle");
    setSession(null);
    void endLightkeyHelperInstall().catch(() => undefined);
  }, [autoStart, phase]);

  const cancel = () => {
    manual.current = false;
    setPhase("idle");
    setSession(null);
    void endLightkeyHelperInstall().catch(() => undefined);
  };

  // The live region stays on screen so the change to "connected" is announced even though the box closes.
  const announcement = phase === "done"
    ? "The Lighting computer is connected."
    : phase === "ready" ? "Waiting for the helper\u2026" : "";
  const live = <p aria-live="polite" className="sr-only" data-testid="lightkey-install-live" role="status">{announcement}</p>;

  if (phase === "done") {
    return <div data-testid="lightkey-install-done">{live}</div>;
  }
  if (current && phase === "idle" && !autoStart) return <div>{live}</div>;
  if (status.link === "off" && phase === "idle") return <div>{live}</div>;

  const panel = "mt-2 rounded-ds-lg border border-edge-subtle bg-surface-950 px-3 py-2 text-type-body-sm text-ink-body";
  return (
    <section aria-label={heading} className={panel} data-testid="lightkey-install" id={LIGHTKEY_INSTALL_ID}>
      {live}
      <h4 className="text-[0.8125rem] font-semibold text-ink">{heading}</h4>

      {(phase === "idle" || phase === "starting") && (
        <div className="mt-1.5 flex flex-wrap items-center gap-2">
          <p className="min-w-0 flex-[1_1_16rem] text-ink-muted">
            {connected
              ? "The helper on the Lighting computer is older than the one StagePilot carries. Run one command on that computer to update it."
              : "StagePilot gives you one command to paste into Terminal on the Lighting computer. It installs the Lightkey helper from StagePilot, so that computer needs no internet."}
          </p>
          <Button className={CONTROL_34} loading={phase === "starting"} loadingLabel={"Making the command\u2026"} onClick={() => void start()} variant="secondary" tone="lighting">
            {heading}
          </Button>
        </div>
      )}

      {phase === "ready" && session && (
        <div data-testid="lightkey-install-ready">
          <ol className="mt-1.5 list-decimal space-y-1 pl-5">
            <li>On the Lighting Mac, turn on sACN output in Lightkey and open your project.</li>
            <li>
              Open Terminal there and paste this command.
              <p className="mt-1.5 font-medium text-ink" data-testid="lightkey-install-label">
                Capture Script for Lighting Computer (beta, only for Lightkey):
              </p>
              <div className="mt-1 flex flex-wrap items-center gap-2">
                <code className="min-w-0 flex-[1_1_20rem] select-all break-all rounded-ds-lg border border-edge bg-surface-panel-strong px-2.5 py-1.5 font-mono text-[0.8125rem] text-ink" data-testid="lightkey-install-command" tabIndex={0}>
                  {session.command}
                </code>
                <Button className={CONTROL_34} onClick={() => void copy()} variant="secondary" tone="lighting">
                  {copied ? "Copied" : "Copy"}
                </Button>
              </div>
            </li>
            {!manual.current && <li>Wait here. It connects by itself.</li>}
          </ol>
          <p className="mt-1.5 text-ink-muted" data-testid="lightkey-install-waiting">
            {`${manual.current ? "" : "Waiting for the helper\u2026 "}The command works for ${countdownText(left)} more, on this network only.`}
          </p>
          <p className="mt-1 text-ink-muted">
            Python comes with Apple&apos;s command-line tools. If Terminal says it is missing, run <code className="font-mono">xcode-select --install</code> first.
            macOS may ask to let Lightkey API use your local network and read your Documents and Downloads folders. Click Allow each time (the Lighting Mac must be unlocked).
          </p>
          {error && <p className="mt-1 text-danger-text" role="alert">{error}</p>}
          <div className="mt-1.5">
            <Button className={CONTROL_34} onClick={cancel} variant="secondary">Cancel</Button>
          </div>
        </div>
      )}

      {(phase === "expired" || phase === "error") && (
        <div className="mt-1.5" data-testid="lightkey-install-problem">
          <p className={phase === "error" ? "text-danger-text" : "text-warning-text"} role="alert">
            {phase === "expired" ? "That command has expired. Make a new one." : error}
          </p>
          <div className="mt-1.5">
            <Button className={CONTROL_34} onClick={() => void start()} variant="secondary" tone="lighting">
              {phase === "expired" ? "Make a new command" : "Try again"}
            </Button>
          </div>
        </div>
      )}
    </section>
  );
}
