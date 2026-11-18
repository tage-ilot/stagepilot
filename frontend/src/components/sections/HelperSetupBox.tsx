import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

import { describeError } from "../../errorText";
import { countdownText } from "../lightkeyProject";
import { CONTROL_34 } from "../compactControl";
import { Button } from "../ui/Button";
import type { Tone } from "../ui/types";

/** Where the helper stands, in the words every Live Link shares (each page maps its own status onto these). `null`: not heard from yet. */
export type HelperBoxState = "ready" | "outdated" | "not_installed" | "needs_command" | "not_answering" | "checking" | null;

/** The three requests of the one-time command: make it, end it, and (optionally) look more often while it waits. */
export interface HelperInstallController {
  start: () => Promise<{ command: string; expires_at: string }>;
  end: () => Promise<unknown>;
  watch?: (on: boolean) => void;
}

/** Every sentence of the box. A new helper brings its own wording; the behaviour is the same for all. */
export interface HelperSetupWording {
  /** The box's test id; `${testId}-live`, `-done`, `-ready`, `-label`, `-command`, `-waiting`, `-problem`, `-why`, `-need-computer` follow. */
  testId: string;
  /** The box's heading and button: before the first command, when the helper is behind, and when the command is made on purpose while all is well. */
  headings: { setup: string; update: string; command: string };
  /** The sentence under the heading, by state (a state without one reads like `not_installed`). */
  why: Partial<Record<"not_installed" | "outdated" | "needs_command" | "not_answering", string>> & { not_installed: string };
  /** "the Library Link": "Waiting for the Library Link…" */
  waitingFor: string;
  /** Said once, politely, when the helper answers and the box closes. */
  done: string;
  /** An extra first step ("turn on sACN output in Lightkey"). */
  before?: ReactNode;
  /** The step that holds the command. */
  paste: ReactNode;
  /** The label over the command. */
  label: ReactNode;
  /** Under the steps: what macOS may ask for. The Python note is part of the box. */
  permissions: ReactNode;
}

export interface HelperSetupBoxProps {
  wording: HelperSetupWording;
  controller: HelperInstallController;
  state: HelperBoxState;
  /** The helper answers at all (it may be behind). Default: `state === "ready"`. */
  answered?: boolean;
  /** Nothing to set up now: only the live region is drawn, unless the box was opened on purpose. */
  quiet?: boolean;
  /** Drawn in the quiet case (the line about an update StagePilot does by itself). */
  quietNote?: ReactNode;
  /** A sentence ahead of the reason, when StagePilot tried to update the helper and could not. */
  lead?: string;
  /** Opened on purpose by the person: make the command at once, once per opening. */
  autoStart?: boolean;
  /** False until the computer is known: the command waits, and `needComputer` says what to do. */
  hasComputer?: boolean;
  needComputer?: ReactNode;
  /** Closing the box (autoStart turns off) ends a command that was made on purpose. */
  endOnClose?: boolean;
  tone?: Tone;
}

type Phase = "idle" | "starting" | "ready" | "expired" | "error" | "done";

const panel = "mt-2 rounded-ds-lg border border-edge-subtle bg-surface-950 px-3 py-2 text-type-body-sm text-ink-body";

/**
 * The helper setup box, ONE for every helper (Lightkey's, the Playback Library Link, the Presenter Link). StagePilot makes a one-time
 * command; pasted into Terminal on the other computer it fetches the matching helper from StagePilot itself (this network only, 30
 * minutes, no internet). The box shows the command, a copy button and a countdown, ends the session by itself when the helper
 * answers, and says so in a live region that stays on screen even after the box closes. Behind helper: "Update ...". Ready, and
 * opened on purpose: the command for another computer, which just runs until it expires or is cancelled.
 */
export function HelperSetupBox({
  wording, controller, state, answered, quiet = false, quietNote, lead, autoStart = false, hasComputer = true, needComputer, endOnClose = false, tone,
}: HelperSetupBoxProps) {
  const ready = state === "ready";
  const heard = answered ?? ready;
  const [phase, setPhase] = useState<Phase>("idle");
  const [session, setSession] = useState<{ command: string; expires_at: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [left, setLeft] = useState(0);
  const [copied, setCopied] = useState(false);
  // What was true when the command was made: a command made while all was well "cannot be answered" and just runs on.
  const startedReady = useRef(false);
  const startedHeard = useRef(false);
  const autoRan = useRef(false);
  const alive = useRef(true);
  const ctl = useRef(controller);
  ctl.current = controller;
  const { testId } = wording;
  const heading = ready ? wording.headings.command : state === "outdated" ? wording.headings.update : wording.headings.setup;

  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);

  const start = useCallback(async () => {
    setPhase("starting");
    setError(null);
    setCopied(false);
    try {
      const made = await ctl.current.start();
      if (!alive.current) return;
      startedReady.current = ready;
      startedHeard.current = heard;
      setSession(made);
      setLeft(Date.parse(made.expires_at) - Date.now());
      setPhase("ready");
    } catch (cause) {
      if (!alive.current) return;
      setError(describeError(cause, "Could not make the setup command."));
      setPhase("error");
    }
  }, [ready, heard]);

  // Opened on purpose: make the command at once, once per opening (Cancel leaves it cancelled).
  useEffect(() => {
    if (!autoStart) { autoRan.current = false; return; }
    if (autoRan.current || phase !== "idle" || !hasComputer) return;
    autoRan.current = true;
    void start();
  }, [autoStart, phase, hasComputer, start]);

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

  // While the command waits, the page may look more often (the backend ends the session by itself when the helper answers).
  useEffect(() => {
    const watch = ctl.current.watch;
    if (!watch) return undefined;
    watch(phase === "ready");
    return () => watch(false);
  }, [phase]);

  // The helper answers: the box is done and the session ends. A command made while all was well just runs until it expires or is cancelled.
  useEffect(() => {
    if (phase !== "ready" || startedReady.current) return;
    const finished = startedHeard.current ? ready : heard;
    if (!finished) return;
    setPhase("done");
    void ctl.current.end().catch(() => undefined);
  }, [phase, ready, heard]);

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

  const reset = useCallback(() => {
    startedReady.current = false;
    setPhase("idle");
    setSession(null);
    void ctl.current.end().catch(() => undefined);
  }, []);
  // Closing the box (the Details button) ends a command that was made on purpose.
  useEffect(() => {
    if (!endOnClose || autoStart || !startedReady.current || phase === "idle") return;
    reset();
  }, [endOnClose, autoStart, phase, reset]);

  // The live region stays on screen so the change to "ready" is announced even though the box closes.
  const announcement = phase === "done" ? wording.done : phase === "ready" ? `Waiting for ${wording.waitingFor}\u2026` : "";
  const live = <p aria-live="polite" className="sr-only" data-testid={`${testId}-live`} role="status">{announcement}</p>;

  if (phase === "done") return <div data-testid={`${testId}-done`}>{live}</div>;
  if (quiet && phase === "idle" && !autoStart) return <div>{live}{quietNote}</div>;
  if (!hasComputer) return <div>{live}<p className="mt-2 text-type-body-sm text-ink-muted" data-testid={`${testId}-need-computer`}>{needComputer}</p></div>;

  const why = wording.why[state === "outdated" || state === "needs_command" || state === "not_answering" ? state : "not_installed"] ?? wording.why.not_installed;
  return (
    <section aria-label={heading} className={panel} data-testid={testId}>
      {live}
      <h4 className="text-[0.8125rem] font-semibold text-ink">{heading}</h4>

      {(phase === "idle" || phase === "starting") && (
        <div className="mt-1.5 flex flex-wrap items-center gap-2">
          <p className="min-w-0 flex-[1_1_16rem] text-ink-muted" data-testid={`${testId}-why`}>{lead ? `${lead} ` : ""}{why}</p>
          <Button className={CONTROL_34} loading={phase === "starting"} loadingLabel={"Making the command\u2026"} onClick={() => void start()} tone={tone} variant="secondary">
            {heading}
          </Button>
        </div>
      )}

      {phase === "ready" && session && (
        <div data-testid={`${testId}-ready`}>
          <ol className="mt-1.5 list-decimal space-y-1 pl-5">
            {wording.before && <li>{wording.before}</li>}
            <li>
              {wording.paste}
              <p className="mt-1.5 font-medium text-ink" data-testid={`${testId}-label`}>{wording.label}</p>
              <div className="mt-1 flex flex-wrap items-center gap-2">
                <code className="min-w-0 flex-[1_1_20rem] select-all break-all rounded-ds-lg border border-edge bg-surface-panel-strong px-2.5 py-1.5 font-mono text-[0.8125rem] text-ink" data-testid={`${testId}-command`} tabIndex={0}>
                  {session.command}
                </code>
                <Button className={CONTROL_34} onClick={() => void copy()} tone={tone} variant="secondary">
                  {copied ? "Copied" : "Copy"}
                </Button>
              </div>
            </li>
            {!startedReady.current && <li>Wait here. It connects by itself.</li>}
          </ol>
          <p className="mt-1.5 text-ink-muted" data-testid={`${testId}-waiting`}>
            {`${startedReady.current ? "" : `Waiting for ${wording.waitingFor}\u2026 `}The command works for ${countdownText(left)} more, on this network only.`}
          </p>
          <p className="mt-1 text-ink-muted">
            Python comes with Apple&apos;s command-line tools. If Terminal says it is missing, run <code className="font-mono">xcode-select --install</code> first.
            {" "}{wording.permissions}
          </p>
          {error && <p className="mt-1 text-danger-text" role="alert">{error}</p>}
          <div className="mt-1.5">
            <Button className={CONTROL_34} onClick={reset} variant="secondary">Cancel</Button>
          </div>
        </div>
      )}

      {(phase === "expired" || phase === "error") && (
        <div className="mt-1.5" data-testid={`${testId}-problem`}>
          <p className={phase === "error" ? "text-danger-text" : "text-warning-text"} role="alert">
            {phase === "expired" ? "That command has expired. Make a new one." : error}
          </p>
          <div className="mt-1.5">
            <Button className={CONTROL_34} onClick={() => void start()} tone={tone} variant="secondary">
              {phase === "expired" ? "Make a new command" : "Try again"}
            </Button>
          </div>
        </div>
      )}
    </section>
  );
}
