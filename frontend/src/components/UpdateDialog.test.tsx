import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createRef } from "react";
import { describe, expect, it, vi } from "vitest";

import type { UpdaterState } from "../hooks/useUpdater";
import { UpdateAvailableButton } from "./UpdateAvailableButton";
import { UpdateDialog } from "./UpdateDialog";

const confirmation: UpdaterState = {
  status: "confirmation",
  currentVersion: "1.1.5",
  availableVersion: "1.2.0",
  releaseNotes: "Signed updater support",
  releaseDate: null,
  progress: null,
  error: null,
  errorDialogOpen: false,
  successMessage: null,
  installBlockedReason: null,
};

describe("StagePilot update UI", () => {
  it("renders an accessible compact update button", async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    render(<UpdateAvailableButton onClick={onClick} version="1.2.0" />);

    const button = screen.getByRole("button", { name: "Update StagePilot to version 1.2.0" });
    await user.click(button);
    expect(onClick).toHaveBeenCalledOnce();
  });

  it("shows versions and safe release-note text without starting automatically", () => {
    const onConfirm = vi.fn();
    render(
      <UpdateDialog
        onCancel={vi.fn()}
        onCloseError={vi.fn()}
        onConfirm={onConfirm}
        onRetry={vi.fn()}
        returnFocus={createRef()}
        updater={confirmation}
      />,
    );
    expect(screen.getByText("1.1.5")).toBeInTheDocument();
    expect(screen.getByText("1.2.0")).toBeInTheDocument();
    expect(screen.getByText("Signed updater support")).toBeInTheDocument();
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("cancels with Escape and returns focus", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    const returnFocus = createRef<HTMLButtonElement>();
    render(
      <>
        <button ref={returnFocus}>Update opener</button>
        <UpdateDialog
          onCancel={onCancel}
          onCloseError={vi.fn()}
          onConfirm={vi.fn()}
          onRetry={vi.fn()}
          returnFocus={returnFocus}
          updater={confirmation}
        />
      </>,
    );
    await user.keyboard("{Escape}");
    expect(onCancel).toHaveBeenCalledOnce();
    expect(returnFocus.current).toHaveFocus();
  });

  it("ignores Escape while the update is installing", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    render(<UpdateDialog onCancel={onCancel} onCloseError={vi.fn()} onConfirm={vi.fn()} onRetry={vi.fn()} returnFocus={createRef()} updater={{ ...confirmation, status: "installing" }} />);
    await user.keyboard("{Escape}");
    expect(onCancel).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeVisible();
  });

  it("focuses Cancel first and traps Tab inside the confirmation", async () => {
    const user = userEvent.setup();
    render(<UpdateDialog onCancel={vi.fn()} onCloseError={vi.fn()} onConfirm={vi.fn()} onRetry={vi.fn()} returnFocus={createRef()} updater={confirmation} />);
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: "Update and Restart" })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
  });

  it("renders known and indeterminate progress without cancellation controls", () => {
    const { rerender } = render(
      <UpdateDialog
        onCancel={vi.fn()}
        onCloseError={vi.fn()}
        onConfirm={vi.fn()}
        onRetry={vi.fn()}
        returnFocus={createRef()}
        updater={{
          ...confirmation,
          status: "downloading",
          progress: {
            downloadedBytes: 50,
            totalBytes: 100,
            percentage: 50,
            stage: "downloading",
          },
        }}
      />,
    );
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "50");
    expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument();

    rerender(
      <UpdateDialog
        onCancel={vi.fn()}
        onCloseError={vi.fn()}
        onConfirm={vi.fn()}
        onRetry={vi.fn()}
        returnFocus={createRef()}
        updater={{
          ...confirmation,
          status: "downloading",
          progress: {
            downloadedBytes: 50,
            totalBytes: null,
            percentage: null,
            stage: "downloading",
          },
        }}
      />,
    );
    expect(screen.getByRole("progressbar")).not.toHaveAttribute("aria-valuenow");
  });
});
