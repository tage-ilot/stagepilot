import { Component, type ErrorInfo, type ReactNode } from "react";

import { sendDiagnosticsBundle, copyDiagnosticsLog } from "../diagnostics";
import { Button, Dialog } from "./ui";

interface ErrorBoundaryState {
  error: Error | null;
  sendMessage: string | null;
  sending: boolean;
}

/**
 * Top-level render/lifecycle error boundary, mounted around the app root
 * in App.tsx. Shares CrashLoopAlertDialog's dark alert-dialog visual style
 * (see that component) and the same log-gathering helper
 * (`../diagnostics`), so there is one place that knows how to collect and
 * upload a diagnostic bundle.
 *
 * `window.onerror` / `unhandledrejection` are handled separately (see
 * `installGlobalErrorHandlers` below) since a boundary can't catch those;
 * both paths render through this same component so there is one surface.
 */
export class ErrorBoundary extends Component<{ children: ReactNode }, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null, sendMessage: null, sending: false };

  static getDerivedStateFromError(error: Error): Partial<ErrorBoundaryState> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("StagePilot render error", error, info.componentStack);
  }

  reload = () => {
    window.location.reload();
  };

  copyDetails = async () => {
    const error = this.state.error;
    const text = error ? `${error.name}: ${error.message}\n${error.stack ?? ""}`.trim() : "";
    try {
      await navigator.clipboard.writeText(text);
      this.setState({ sendMessage: "Error details copied." });
    } catch {
      this.setState({ sendMessage: "Could not copy the error details. Reload StagePilot and try again." });
    }
  };

  sendLogs = async () => {
    this.setState({ sending: true, sendMessage: null });
    const result = await sendDiagnosticsBundle(
      this.state.error ? `${this.state.error.name}: ${this.state.error.message}` : undefined,
    );
    if (result.ok) {
      this.setState({ sending: false, sendMessage: result.message });
      return;
    }
    try {
      const content = await copyDiagnosticsLog();
      await navigator.clipboard.writeText(content);
      this.setState({ sending: false, sendMessage: `${result.message} Backend log copied instead.` });
    } catch {
      this.setState({ sending: false, sendMessage: result.message });
    }
  };

  render() {
    if (!this.state.error) return this.props.children;

    // Primitives only (Dialog, Button, plain elements): nothing here may depend on app state that can throw.
    return (
      <div className="contents" onMouseDown={(event) => event.stopPropagation()}>
        <Dialog
          open
          dismissible={false}
          layer="boundary"
          tone="danger"
          title="StagePilot ran into a problem"
          description="Reloading usually fixes this. You can also send logs to the developer to help track it down."
          actions={
            <>
              <Button disabled={this.state.sending} onClick={() => void this.sendLogs()} size="touch" variant="secondary">
                {this.state.sending ? "Sending…" : "Send logs to developer"}
              </Button>
              <Button onClick={() => void this.copyDetails()} size="touch" variant="secondary">
                Copy error details
              </Button>
              <Button onClick={this.reload} size="touch" variant="destructive-soft">
                Reload StagePilot
              </Button>
            </>
          }
        >
          {this.state.sendMessage && (
            <p aria-live="polite" className="text-type-body-sm text-ink-muted">
              {this.state.sendMessage}
            </p>
          )}
        </Dialog>
      </div>
    );
  }
}
