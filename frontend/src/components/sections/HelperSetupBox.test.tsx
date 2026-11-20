import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { HelperSetupBox, type HelperSetupBoxProps, type HelperSetupWording } from "./HelperSetupBox";

/**
 * The template's own contract, with made-up words for a made-up helper ("Zed Link"): the three real helpers' words and behaviour are in
 * `components/HelperSetupBox.helpers.test.tsx`. Here only what the template promises to every helper.
 */
const wording: HelperSetupWording = {
  testId: "zed-install",
  headings: { setup: "Set up the Zed computer", update: "Update the Zed Link", command: "Zed Link command" },
  why: { not_installed: "Zed Link is optional.", outdated: "Zed Link is old.", not_answering: "Zed Link is silent." },
  waitingFor: "the Zed Link",
  done: "The Zed Link is ready.",
  before: "First, open Zed.",
  paste: "Paste this in Terminal.",
  label: "Zed Link:",
  permissions: "macOS may ask about Zed.",
};
const session = (minutes = 30) => ({ command: "curl zed | sh", expires_at: new Date(Date.now() + minutes * 60_000).toISOString() });
const controller = () => ({ start: vi.fn().mockResolvedValue(session()), end: vi.fn().mockResolvedValue({ ended: true }), watch: vi.fn() });
const box = (over: Partial<HelperSetupBoxProps> = {}) => <HelperSetupBox controller={controller()} state="not_installed" wording={wording} {...over} />;

afterEach(() => { vi.useRealTimers(); });

describe("HelperSetupBox", () => {
  it("is a labelled region with the heading, the reason for the state and one button; nothing is made until the button", () => {
    const ctl = controller();
    render(box({ controller: ctl }));
    expect(screen.getByRole("region", { name: "Set up the Zed computer" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 4, name: "Set up the Zed computer" })).toBeInTheDocument();
    expect(screen.getByTestId("zed-install-why")).toHaveTextContent("Zed Link is optional.");
    expect(screen.getByRole("button", { name: "Set up the Zed computer" })).toBeEnabled();
    expect(ctl.start).not.toHaveBeenCalled();
  });

  it.each([["outdated", "Update the Zed Link", "Zed Link is old."], ["not_answering", "Set up the Zed computer", "Zed Link is silent."], ["needs_command", "Set up the Zed computer", "Zed Link is optional."]] as const)("heading and reason follow the state: %s", (state, heading, why) => {
    render(box({ state }));
    expect(screen.getByRole("button", { name: heading })).toBeInTheDocument();
    expect(screen.getByTestId("zed-install-why")).toHaveTextContent(why);
  });

  it("puts a lead sentence ahead of the reason", () => {
    render(box({ state: "outdated", lead: "It could not update itself." }));
    expect(screen.getByTestId("zed-install-why")).toHaveTextContent("It could not update itself. Zed Link is old.");
  });

  it("makes the command on the button: steps in order, the command in a focusable code element, the countdown, the live region, watch on", async () => {
    const ctl = controller();
    render(box({ controller: ctl }));
    await userEvent.setup().click(screen.getByRole("button", { name: "Set up the Zed computer" }));
    const ready = await screen.findByTestId("zed-install-ready");
    expect(Array.from(ready.querySelectorAll("ol > li")).map((li) => li.textContent?.slice(0, 12))).toEqual(["First, open ", "Paste this i", "Wait here. I"]);
    expect(screen.getByTestId("zed-install-label")).toHaveTextContent("Zed Link:");
    expect(screen.getByTestId("zed-install-command")).toHaveAttribute("tabindex", "0");
    expect(screen.getByTestId("zed-install-waiting")).toHaveTextContent(/Waiting for the Zed Link… The command works for (30:00|29:5\d) more, on this network only\./);
    expect(ready).toHaveTextContent("macOS may ask about Zed.");
    expect(ready).toHaveTextContent("xcode-select --install");
    expect(screen.getByTestId("zed-install-live")).toHaveAttribute("aria-live", "polite");
    expect(screen.getByTestId("zed-install-live")).toHaveTextContent("Waiting for the Zed Link…");
    expect(ctl.watch).toHaveBeenLastCalledWith(true);
  });

  it("without the optional first step the list has two steps, and a helper that needs no `watch` works without one", async () => {
    render(box({ controller: { start: vi.fn().mockResolvedValue(session()), end: vi.fn().mockResolvedValue({}) }, wording: { ...wording, before: undefined } }));
    await userEvent.setup().click(screen.getByRole("button", { name: "Set up the Zed computer" }));
    expect((await screen.findByTestId("zed-install-ready")).querySelectorAll("ol > li")).toHaveLength(2);
  });

  it("`answered` (it replies, it may be behind) ends the session for a helper that was not heard when the command was made, `ready` for one that was behind", async () => {
    const ctl = controller();
    const { rerender } = render(box({ controller: ctl }));
    await userEvent.setup().click(screen.getByRole("button", { name: "Set up the Zed computer" }));
    await screen.findByTestId("zed-install-ready");
    // It answers, but is still behind: for a helper that was not there at all, answering is enough.
    rerender(box({ controller: ctl, state: "outdated", answered: true }));
    await waitFor(() => expect(screen.getByTestId("zed-install-done")).toBeInTheDocument());
    expect(ctl.end).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("zed-install-live")).toHaveTextContent("The Zed Link is ready.");
  });

  it("a command made while all is well just runs: it says nothing about waiting, never closes by itself, and Cancel ends it", async () => {
    const ctl = controller();
    render(box({ controller: ctl, state: "ready", quiet: true, autoStart: true }));
    expect(await screen.findByTestId("zed-install-command")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Zed Link command" })).toBeInTheDocument();
    expect(screen.getByTestId("zed-install-waiting")).toHaveTextContent(/^The command works for 30:00 more/);
    expect(screen.getByTestId("zed-install-ready").querySelectorAll("ol > li")).toHaveLength(2);
    await userEvent.setup().click(screen.getByRole("button", { name: "Cancel" }));
    expect(ctl.end).toHaveBeenCalledTimes(1);
    expect(ctl.start).toHaveBeenCalledTimes(1);
  });

  it("quiet: only the live region (and the note) while nothing is to be set up and nobody asked", () => {
    render(box({ state: "ready", quiet: true, quietNote: <p data-testid="note">Updated by itself.</p> }));
    expect(screen.queryByTestId("zed-install")).toBeNull();
    expect(screen.getByTestId("zed-install-live")).toBeInTheDocument();
    expect(screen.getByTestId("note")).toBeInTheDocument();
  });

  it("with no computer it asks for one instead of making a command, even when opened on purpose", () => {
    const ctl = controller();
    render(box({ controller: ctl, hasComputer: false, needComputer: "Choose the Zed computer first.", autoStart: true }));
    expect(screen.getByTestId("zed-install-need-computer")).toHaveTextContent("Choose the Zed computer first.");
    expect(screen.queryByRole("button", { name: "Set up the Zed computer" })).toBeNull();
    expect(ctl.start).not.toHaveBeenCalled();
  });

  it("endOnClose: closing the box ends a command made on purpose; without it the command waits for the next opening", async () => {
    for (const endOnClose of [true, false]) {
      const ctl = controller();
      const { rerender, unmount } = render(box({ controller: ctl, state: "ready", quiet: true, autoStart: true, endOnClose }));
      await screen.findByTestId("zed-install-command");
      rerender(box({ controller: ctl, state: "ready", quiet: true, autoStart: false, endOnClose }));
      if (endOnClose) await waitFor(() => expect(ctl.end).toHaveBeenCalledTimes(1)); else expect(ctl.end).not.toHaveBeenCalled();
      unmount();
    }
  });

  it("expires with a plain message and offers a new command; the countdown is text only", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const ctl = controller();
    ctl.start.mockResolvedValueOnce(session(1)).mockResolvedValueOnce(session());
    render(box({ controller: ctl }));
    await act(async () => { screen.getByRole("button", { name: "Set up the Zed computer" }).click(); });
    await screen.findByTestId("zed-install-ready");
    await act(async () => { await vi.advanceTimersByTimeAsync(61_000); });
    expect(screen.getByRole("alert")).toHaveTextContent("That command has expired. Make a new one.");
    await act(async () => { screen.getByRole("button", { name: "Make a new command" }).click(); });
    expect(await screen.findByTestId("zed-install-ready")).toBeInTheDocument();
  });

  it("uses the tone for its buttons only (no colour of its own)", () => {
    render(box({ tone: "playback" }));
    expect(screen.getByTestId("zed-install")).toHaveClass("border-edge-subtle", "bg-surface-950");
  });
});
