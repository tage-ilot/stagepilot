import { useCallback, useEffect, useRef, useState } from "react";

import { ApiError, endLightkeyHelperInstall, startLightkeyHelperInstall } from "../api";
import { describeError } from "../errorText";
import type { LightkeyHelperInstall, LightkeyStatus } from "../types";
import { countdownText, helperIsOutdated } from "./lightkeyProject";
import { CONTROL_34 } from "./compactControl";
import { LiveLinkField } from "./LightingOscParts";
import { Button } from "./ui/Button";
import { Input } from "./ui/Input";

type Phase = "idle" | "starting" | "ready" | "expired" | "error" | "done";

/**
 * "Set up the Lighting computer", in the Lightkey strip. Shown while there is no connected Lighting
 * computer, and as "Update the helper" when the helper that answers is older than the one StagePilot
 * carries. StagePilot makes a one-time command; pasted into Terminal on the Lighting Mac it fetches
 * the matching helper from StagePilot itself (this network only, 30 minutes). The box ends the
 * session by itself when the helper answers.
 */
export function LightkeyInstallBox({ status }: { status: LightkeyStatus }) {
  const connected = status.link === "connected";
  const outdated = helperIsOutdated(status);
  const [phase, setPhase] = useState<Phase>("idle");
  const [session, setSession] = useState<LightkeyHelperInstall | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [needAddress, setNeedAddress] = useState(false);
  const [address, setAddress] = useState("");
  const [left, setLeft] = useState(0);
  const [copied, setCopied] = useState(false);
  const startedConnected = useRef(false);
  const alive = useRef(true);
  const heading = connected ? "Update the helper" : "Set up the Lighting computer";

  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);

  const start = useCallback(async (typedAddress?: string) => {
    setPhase("starting");
    setError(null);
    setCopied(false);
    try {
      const made = await startLightkeyHelperInstall(typedAddress);
      if (!alive.current) return;
      startedConnected.current = connected;
      setSession(made);
      setLeft(Date.parse(made.expires_at) - Date.now());
      setNeedAddress(false);
      setPhase("ready");
    } catch (cause) {
      if (!alive.current) return;
      // 409: this computer's address on the Lighting network could not be found; ask for it.
      setNeedAddress(cause instanceof ApiError && cause.status === 409);
      setError(describeError(cause, "Could not make the setup command."));
      setPhase("error");
    }
  }, [connected]);

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
    if (phase !== "ready") return;
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

  const cancel = () => {
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
  if (connected && !outdated && phase === "idle") return <div>{live}</div>;
  if (status.link === "off" && phase === "idle") return <div>{live}</div>;

  const panel = "mt-2 rounded-ds-lg border border-edge-subtle bg-surface-950 px-3 py-2 text-type-body-sm text-ink-body";
  return (
    <section aria-label={heading} className={panel} data-testid="lightkey-install">
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
              <div className="mt-1.5 flex flex-wrap items-center gap-2">
                <code className="min-w-0 flex-[1_1_20rem] select-all break-all rounded-ds-lg border border-edge bg-surface-panel-strong px-2.5 py-1.5 font-mono text-[0.8125rem] text-ink" data-testid="lightkey-install-command" tabIndex={0}>
                  {session.command}
                </code>
                <Button className={CONTROL_34} onClick={() => void copy()} variant="secondary" tone="lighting">
                  {copied ? "Copied" : "Copy"}
                </Button>
              </div>
            </li>
            <li>Wait here. It connects by itself.</li>
          </ol>
          <p className="mt-1.5 text-ink-muted" data-testid="lightkey-install-waiting">
            {`Waiting for the helper\u2026 The command works for ${countdownText(left)} more, on this network only.`}
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
          {phase === "error" && needAddress && (
            <div className="mt-1.5">
              <LiveLinkField caption="This computer's address on the Lighting network">
                <Input aria-label="This computer's address" className="w-56 font-mono" inputMode="decimal" onChange={(event) => setAddress(event.target.value)} placeholder="192.168.1.20" spellCheck={false} value={address} />
              </LiveLinkField>
            </div>
          )}
          <div className="mt-1.5">
            <Button className={CONTROL_34} disabled={phase === "error" && needAddress && !address.trim()} onClick={() => void start(needAddress ? address.trim() : undefined)} variant="secondary" tone="lighting">
              {phase === "expired" ? "Make a new command" : needAddress ? "Use this address" : "Try again"}
            </Button>
          </div>
        </div>
      )}
    </section>
  );
}
