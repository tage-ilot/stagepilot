import { useEffect, useRef, useState } from "react";

import { type BackendCrashLoopDetected } from "../desktop";
import { copyDiagnosticsLog, sendDiagnosticsBundle } from "../diagnostics";
import { Button, Toast } from "./ui";

const failureKindLabel = (kind: BackendCrashLoopDetected["failure_kind"]): string | null => {
  switch (kind) {
    case "port_occupied":
      return "The backend port is occupied.";
    case "server_exited":
      return "The StagePilot server keeps stopping.";
    case "timeout":
      return "The backend keeps timing out on startup.";
    default:
      return null;
  }
};

/**
 * Non-blocking alert for a repeat backend crash loop. Mounted at the App
 * root (see App.tsx) so it can appear regardless of whether the user is on
 * the startup screen or the live dashboard. Dismissing it never disables
 * backend retry logic; a fresh alert can still fire later per the re-arm
 * rule enforced on the Rust side.
 */
export function CrashLoopAlertDialog({
  crashLoop,
  onDismiss,
}: {
  crashLoop: BackendCrashLoopDetected | null;
  onDismiss: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const [copyMessage, setCopyMessage] = useState<string | null>(null);
  const [sending, setSending] = useState(false);

  useEffect(() => {
    if (!crashLoop) return;
    setCopyMessage(null);
    dialog.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onDismiss();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [crashLoop, onDismiss]);

  if (!crashLoop) return null;

  const detail = failureKindLabel(crashLoop.failure_kind);

  const copyLog = async () => {
    try {
      const content = await copyDiagnosticsLog();
      await navigator.clipboard.writeText(content);
      setCopyMessage("Backend log copied.");
    } catch (error) {
      setCopyMessage(error instanceof Error ? error.message : "Unable to copy the backend log.");
    }
  };

  const sendLogs = async () => {
    setSending(true);
    setCopyMessage(null);
    const result = await sendDiagnosticsBundle(detail ?? crashLoop.message);
    setSending(false);
    setCopyMessage(result.message);
  };

  return (
    <Toast
      ref={dialog}
      side="right"
      kicker="Repeated backend crash"
      title="StagePilot&rsquo;s backend keeps crashing"
      description={detail ?? undefined}
      onDismiss={onDismiss}
      actions={
        <>
          <Button size="touch" variant="secondary" disabled={sending} onClick={() => void sendLogs()}>
            {sending ? "Sending…" : "Send to developer"}
          </Button>
          <Button size="touch" variant="destructive-soft" onClick={() => void copyLog()}>
            Copy Log
          </Button>
        </>
      }
    >
      {copyMessage && (
        <p aria-live="polite" className="mt-2 text-type-body-sm text-ink-muted">
          {copyMessage}
        </p>
      )}
    </Toast>
  );
}
