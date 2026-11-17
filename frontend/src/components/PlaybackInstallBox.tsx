import { useCallback, useEffect, useRef, useState } from "react";

import { ApiError, endPlaybackHelperInstall, startPlaybackHelperInstall } from "../api";
import { describeError } from "../errorText";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { HelperState, PlaybackHelperInstall } from "../types";
import { CONTROL_34 } from "./compactControl";
import { LiveLinkField } from "./LightingOscParts";
import { countdownText } from "./lightkeyProject";
import { Button } from "./ui/Button";
import { Input } from "./ui/Input";

type Phase = "idle" | "starting" | "ready" | "expired" | "error" | "done";

const WHY: Record<Exclude<HelperState, "ready">, string> = {
  not_installed: "The network script is optional. It lets StagePilot read each song's key, BPM and arrangement from Playback. StagePilot gives you one command to paste into Terminal on the Playback computer. The command runs there and that computer needs no internet.",
  outdated: "The network script on the Playback computer is older than the one StagePilot carries. It still works. Run one command in Terminal on that computer to update it. No internet needed.",
  needs_key: "The network script on the Playback computer does not know this StagePilot yet. Run the command again, in Terminal on that computer. No internet needed.",
  unreachable: "StagePilot cannot reach the network script. Is the Playback computer on and awake? If the network script was set up again from another computer, run the command again here.",
  forgotten: "StagePilot forgot the network script, so it cannot use it. Run the command in Terminal on the Playback computer to use it again. No internet needed.",
};

/**
 * "Set up the Playback computer", in the Playback Live Link block. Shown while the network script is not ready, and as "Update the network
 * script" when the one that answers is older than the one StagePilot carries. StagePilot makes a one-time command; pasted into Terminal on
 * the Playback computer it fetches the matching script from StagePilot itself (this network only, 30 minutes, no internet), together with
 * the key that only this StagePilot can use. The box ends the session by itself when the script answers. The command holds only the
 * one-time address: the key is never on screen.
 *
 * It is the Lighting computer's `LightkeyInstallBox` copied, not shared: that box is written around the Lightkey status (its `connected`
 * and `outdated` checks, its data-testids and its Lightkey words are pinned by its own tests), so a shared component would have had to
 * change those tests. The countdown text, the field caption, the control size and the shapes of the requests are shared.
 */
export function PlaybackInstallBox({ library, hasComputer }: { library: PlaybackLibraryController; hasComputer: boolean }) {
  const state: HelperState | null = library.status?.helper.state ?? null;
  const ready = state === "ready";
  const [phase, setPhase] = useState<Phase>("idle");
  const [session, setSession] = useState<PlaybackHelperInstall | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [needAddress, setNeedAddress] = useState(false);
  const [address, setAddress] = useState("");
  const [left, setLeft] = useState(0);
  const [copied, setCopied] = useState(false);
  const alive = useRef(true);
  const heading = state === "outdated" ? "Update the network script" : "Set up the Playback computer";
  const { watch } = library;

  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);

  const start = useCallback(async (typedAddress?: string) => {
    setPhase("starting");
    setError(null);
    setCopied(false);
    try {
      const made = await startPlaybackHelperInstall(typedAddress);
      if (!alive.current) return;
      setSession(made);
      setLeft(Date.parse(made.expires_at) - Date.now());
      setNeedAddress(false);
      setPhase("ready");
    } catch (cause) {
      if (!alive.current) return;
      // 409: this computer's address on the Playback network could not be found; ask for it.
      const noAddress = cause instanceof ApiError && cause.status === 409 && !/Playback computer first/.test(cause.message);
      setNeedAddress(noAddress);
      setError(describeError(cause, "Could not make the setup command."));
      setPhase("error");
    }
  }, []);

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

  // While the command waits, the window looks every few seconds (the backend ends the session by itself when the script answers).
  useEffect(() => {
    watch(phase === "ready");
    return () => watch(false);
  }, [phase, watch]);

  // The script answers with the key: the box is done and the session ends.
  useEffect(() => {
    if (phase !== "ready" || !ready) return;
    setPhase("done");
    void endPlaybackHelperInstall().catch(() => undefined);
  }, [phase, ready]);

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
    void endPlaybackHelperInstall().catch(() => undefined);
  };

  // The live region stays on screen so the change to "ready" is announced even though the box closes.
  const announcement = phase === "done"
    ? "The network script is ready."
    : phase === "ready" ? "Waiting for the network script\u2026" : "";
  const live = <p aria-live="polite" className="sr-only" data-testid="playback-install-live" role="status">{announcement}</p>;

  if (phase === "done") return <div data-testid="playback-install-done">{live}</div>;
  // Nothing to set up while the script is ready; nothing to say before it has answered once.
  if ((ready || state === null) && phase === "idle") return <div>{live}</div>;
  if (!hasComputer) {
    return <div>{live}<p className="mt-2 text-type-body-sm text-ink-muted" data-testid="playback-install-need-computer">Choose the Playback computer above, then set up its network script.</p></div>;
  }

  const panel = "mt-2 rounded-ds-lg border border-edge-subtle bg-surface-950 px-3 py-2 text-type-body-sm text-ink-body";
  const why = WHY[(state && state !== "ready" ? state : "not_installed") as Exclude<HelperState, "ready">];
  return (
    <section aria-label={heading} className={panel} data-testid="playback-install">
      {live}
      <h4 className="text-[0.8125rem] font-semibold text-ink">{heading}</h4>

      {(phase === "idle" || phase === "starting") && (
        <div className="mt-1.5 flex flex-wrap items-center gap-2">
          <p className="min-w-0 flex-[1_1_16rem] text-ink-muted">{why}</p>
          <Button className={CONTROL_34} loading={phase === "starting"} loadingLabel={"Making the command\u2026"} onClick={() => void start()} variant="secondary" tone="playback">
            {heading}
          </Button>
        </div>
      )}

      {phase === "ready" && session && (
        <div data-testid="playback-install-ready">
          <ol className="mt-1.5 list-decimal space-y-1 pl-5">
            <li>
              On the Playback computer, open Terminal and paste this command. It runs there and needs no internet.
              <p className="mt-1.5 font-medium text-ink" data-testid="playback-install-label">
                Network script for the Playback computer:
              </p>
              <div className="mt-1 flex flex-wrap items-center gap-2">
                <code className="min-w-0 flex-[1_1_20rem] select-all break-all rounded-ds-lg border border-edge bg-surface-panel-strong px-2.5 py-1.5 font-mono text-[0.8125rem] text-ink" data-testid="playback-install-command" tabIndex={0}>
                  {session.command}
                </code>
                <Button className={CONTROL_34} onClick={() => void copy()} variant="secondary" tone="playback">
                  {copied ? "Copied" : "Copy"}
                </Button>
              </div>
            </li>
            <li>Wait here. It connects by itself.</li>
          </ol>
          <p className="mt-1.5 text-ink-muted" data-testid="playback-install-waiting">
            {`Waiting for the network script\u2026 The command works for ${countdownText(left)} more, on this network only.`}
          </p>
          <p className="mt-1 text-ink-muted">
            Python comes with Apple&apos;s command-line tools. If Terminal says it is missing, run <code className="font-mono">xcode-select --install</code> first.
            macOS may ask to let Playback Library read Playback&apos;s data. Click Allow each time (the Playback computer must be unlocked).
          </p>
          {error && <p className="mt-1 text-danger-text" role="alert">{error}</p>}
          <div className="mt-1.5">
            <Button className={CONTROL_34} onClick={cancel} variant="secondary">Cancel</Button>
          </div>
        </div>
      )}

      {(phase === "expired" || phase === "error") && (
        <div className="mt-1.5" data-testid="playback-install-problem">
          <p className={phase === "error" ? "text-danger-text" : "text-warning-text"} role="alert">
            {phase === "expired" ? "That command has expired. Make a new one." : error}
          </p>
          {phase === "error" && needAddress && (
            <div className="mt-1.5">
              <LiveLinkField caption="This computer's address on the Playback network">
                <Input aria-label="This computer's address" className="w-56 font-mono" inputMode="decimal" onChange={(event) => setAddress(event.target.value)} placeholder="192.168.1.20" spellCheck={false} value={address} />
              </LiveLinkField>
            </div>
          )}
          <div className="mt-1.5">
            <Button className={CONTROL_34} disabled={phase === "error" && needAddress && !address.trim()} onClick={() => void start(needAddress ? address.trim() : undefined)} variant="secondary" tone="playback">
              {phase === "expired" ? "Make a new command" : needAddress ? "Use this address" : "Try again"}
            </Button>
          </div>
        </div>
      )}
    </section>
  );
}
