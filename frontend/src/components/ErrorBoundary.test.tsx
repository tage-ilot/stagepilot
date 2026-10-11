import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ sendDiagnosticsBundle: vi.fn(), copyDiagnosticsLog: vi.fn() }));
vi.mock("../diagnostics", async (original) => ({
  ...(await original<typeof import("../diagnostics")>()),
  sendDiagnosticsBundle: mocks.sendDiagnosticsBundle,
  copyDiagnosticsLog: mocks.copyDiagnosticsLog,
}));

import { ErrorBoundary } from "./ErrorBoundary";

function Boom(): never {
  throw new Error("kaboom");
}

describe("ErrorBoundary", () => {
  it("renders children when there is no error", () => {
    render(
      <ErrorBoundary>
        <div>All good</div>
      </ErrorBoundary>,
    );
    expect(screen.getByText("All good")).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("catches a thrown render error and shows the send-logs UI", () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);

    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );

    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText("StagePilot ran into a problem")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send logs to developer" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Reload StagePilot" })).toBeInTheDocument();

    consoleError.mockRestore();
  });

  it("sends logs when the send-logs button is clicked", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    mocks.sendDiagnosticsBundle.mockResolvedValue({ ok: true, message: "Logs sent to the developer." });

    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Send logs to developer" }));

    expect(await screen.findByText("Logs sent to the developer.")).toBeInTheDocument();
    expect(mocks.sendDiagnosticsBundle).toHaveBeenCalledOnce();
  });

  it("renders on the boundary layer, non-dismissible", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );
    expect(document.querySelector("[data-dialog-backdrop]")).toHaveAttribute("data-layer", "boundary");
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
});
