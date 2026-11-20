import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { helperStatus, linkController, presentation } from "../test/propresenterLinkFixtures";
import { ProPresenterSetupPanel } from "./ProPresenterSetupPanel";

const panel = (link?: ReturnType<typeof linkController>) => render(
  <ProPresenterSetupPanel error={null} link={link} message={null} onRefreshTimers={vi.fn()} onSave={vi.fn()} onTest={vi.fn()} pendingOperation={null} propresenter={presentation()} />,
);

describe("ProPresenterSetupPanel with the ProPresenter Link", () => {
  it("shows the Live Link block under the timer and Look setup, and leaves that setup exactly as it was", () => {
    panel(linkController(helperStatus("ready")));
    expect(screen.getByRole("region", { name: "ProPresenter Live Link" })).toBeInTheDocument();
    for (const name of ["Test connection", "Refresh timers and Looks", "Save configuration"]) expect(screen.getByRole("button", { name })).toBeInTheDocument();
    expect(screen.getByLabelText("Host")).toBeInTheDocument();
    expect(screen.getByLabelText("ProPresenter Look")).toBeInTheDocument();
    // The block sits after the buttons and before the save bar: the save bar stays last of the form.
    const order = [screen.getByRole("button", { name: "Test connection" }), screen.getByRole("region", { name: "ProPresenter Live Link" }), screen.getByRole("button", { name: "Save configuration" })];
    for (let i = 0; i < order.length - 1; i += 1) expect(order[i]!.compareDocumentPosition(order[i + 1]!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("without a controller the panel is the timer and Look setup alone", () => {
    panel();
    expect(screen.queryByRole("region", { name: "ProPresenter Live Link" })).toBeNull();
    expect(screen.getByRole("button", { name: "Test connection" })).toBeInTheDocument();
  });

  it("a computer without the Link shows no warning anywhere in the panel", () => {
    panel(linkController(helperStatus("not_installed")));
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
