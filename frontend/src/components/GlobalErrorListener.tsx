import { useEffect, useState } from "react";

import { sendDiagnosticsBundle, copyDiagnosticsLog } from "../diagnostics";
import { Button, Toast } from "./ui";

/**
 * Catches uncaught async errors (`window.onerror`, `unhandledrejection`)
 * that an ErrorBoundary cannot see, and renders the same dark
 * "something went wrong, send logs?" alert-dialog used by
 * `CrashLoopAlertDialog.tsx` / `ErrorBoundary.tsx`. Mounted once at the app
 * root alongside `ErrorBoundary`.
 */
export function GlobalErrorListener() {
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [sendMessage, setSendMessage] = useState<string | null>(null);

  useEffect(() => {
    const onWindowError = (event: ErrorEvent) => {
      setError(event.message || "An unexpected error occurred.");
    };
    const onUnhandledRejection = (event: PromiseRejectionEvent) => {
      const reason = event.reason;
      setError(
        reason instanceof Error ? `${reason.name}: ${reason.message}` : String(reason ?? "Unhandled rejection."),
      );
    };
    window.addEventListener("error", onWindowError);
    window.addEventListener("unhandledrejection", onUnhandledRejection);
    return () => {
      window.removeEventListener("error", onWindowError);
      window.removeEventListener("unhandledrejection", onUnhandledRejection);
    };
  }, []);

  if (!error) return null;

  const sendLogs = async () => {
    setSending(true);
    setSendMessage(null);
    const result = await sendDiagnosticsBundle(error);
    if (result.ok) {
      setSending(false);
      setSendMessage(result.message);
      return;
    }
    try {
      const content = await copyDiagnosticsLog();
      await navigator.clipboard.writeText(content);
      setSending(false);
      setSendMessage(`${result.message} Backend log copied instead.`);
    } catch {
      setSending(false);
      setSendMessage(result.message);
    }
  };

  return (
    <Toast
      side="left"
      kicker="Something went wrong"
      title="StagePilot ran into a problem"
      description="This didn&rsquo;t stop the app, but you can send logs to the developer to help track it down."
      onDismiss={() => setError(null)}
      actions={
        <Button size="touch" variant="destructive-soft" disabled={sending} onClick={() => void sendLogs()}>
          {sending ? "Sending…" : "Send logs to developer"}
        </Button>
      }
    >
      {sendMessage && (
        <p aria-live="polite" className="mt-2 text-type-body-sm text-ink-muted">
          {sendMessage}
        </p>
      )}
    </Toast>
  );
}
