import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { Frame, Gallery } from "./Gallery";
import { installMadeUpApi } from "./madeUpApi";

/** The section gallery: every state of the Connection section for every connection, made-up data, no backend. */
const sample = (id: string) => document.querySelector(`[data-sample="${id}"]`) as HTMLElement;
/** The Details button of a sample (the setup box inside has a button of nearly the same words, which has no aria-expanded). */
const toggle = (id: string) => within(sample(id)).getAllByRole("button").find((button) => button.hasAttribute("aria-expanded") && /setup|Update the/.test(button.textContent ?? "")) as HTMLElement;
const ids = (group: string) => Array.from(document.querySelectorAll(`[data-group="${group}"] [data-sample]`)).map((node) => node.getAttribute("data-sample")?.replace(`${group}-`, ""));

describe("the section gallery page", () => {
  it("shows every state the review asked for, for Presentation", () => {
    render(<Frame />);
    expect(ids("presentation")).toEqual(["ready", "checking", "not-connected", "scanning", "found-one", "found-many", "not-found", "blocked", "address-list", "connecting", "host-error", "held", "not-installed", "update-by-hand", "update-by-itself", "needs-command", "not-answering", "import-blocked", "this-computer", "no-link"]);
  });

  it("shows every state the review asked for, for Playback (including scanning)", () => {
    render(<Frame />);
    expect(ids("playback")).toEqual(["ready", "scanning", "not-connected", "error", "found-many", "found-one", "address-list", "blocked", "connecting", "not-installed", "update-by-hand", "update-by-itself", "needs-command", "not-answering", "no-link"]);
  });

  it("shows every state the review asked for, for Lighting", () => {
    render(<Frame />);
    expect(ids("lighting")).toEqual(["ready", "searching", "found-one", "found-many", "address-list", "not-found", "not-connected", "host-error", "held", "closed", "update-by-hand", "update-by-itself", "replay-off", "off"]);
  });

  it("draws each state with the REAL block: a Live Link section with its title, pills and (where there is a Link) a Details button", async () => {
    render(<Frame />);
    expect(within(sample("presentation-ready")).getByRole("region", { name: "ProPresenter Live Link" })).toBeInTheDocument();
    expect(within(sample("playback-ready")).getByRole("region", { name: "Playback Live Link" })).toBeInTheDocument();
    expect(await within(sample("lighting-ready")).findByTestId("lightkey-strip")).toHaveAttribute("aria-label", "sACN DMX Live Link");
    expect(toggle("presentation-ready")).toHaveAttribute("aria-expanded", "false");
    expect(toggle("playback-ready")).toHaveAttribute("aria-expanded", "false");
    expect(within(sample("presentation-no-link")).queryByRole("button", { name: /setup/ })).toBeNull();
  });

  it("opens the Details by themselves where something needs doing, and not where nothing does", async () => {
    render(<Frame />);
    for (const id of ["presentation-not-installed", "presentation-update-by-hand", "presentation-needs-command", "presentation-not-answering", "playback-not-installed", "playback-update-by-hand", "playback-needs-command", "playback-not-answering"]) {
      expect(toggle(id), id).toHaveAttribute("aria-expanded", "true");
    }
    for (const id of ["presentation-ready", "presentation-update-by-itself", "presentation-this-computer", "presentation-import-blocked", "playback-ready", "playback-update-by-itself"]) {
      expect(toggle(id), id).toHaveAttribute("aria-expanded", "false");
    }
    await waitFor(() => expect(toggle("lighting-not-connected")).toHaveAttribute("aria-expanded", "true"));
    await waitFor(() => expect(toggle("lighting-ready")).toHaveAttribute("aria-expanded", "false"));
  });

  it("marks the held change and the field error in words", async () => {
    render(<Frame />);
    expect(within(sample("presentation-held")).getByText("Not saved yet")).toBeInTheDocument();
    expect(within(sample("presentation-host-error")).getByRole("alert")).toHaveTextContent("Enter the computer's name");
    await waitFor(() => expect(within(sample("lighting-host-error")).getByRole("alert")).toBeInTheDocument());
  });

  it("has a width switch that sizes the framed page, so the 640 px breakpoint really changes", async () => {
    render(<Gallery />);
    const frame = screen.getByTestId("gallery-frame");
    expect(frame).toHaveStyle({ width: "1280px" });
    expect(screen.getByRole("button", { name: "1280 px" })).toHaveAttribute("aria-pressed", "true");
    await userEvent.setup().click(screen.getByRole("button", { name: "360 px" }));
    expect(frame).toHaveStyle({ width: "360px" });
    expect(screen.getByRole("button", { name: "360 px" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "1280 px" })).toHaveAttribute("aria-pressed", "false");
    await userEvent.setup().click(screen.getByRole("button", { name: "Fit" }));
    expect(frame).toHaveStyle({ width: "100%" });
    expect(frame.getAttribute("src")).toBe("?frame=1");
  });
});

describe("the gallery's made-up API", () => {
  const real = window.fetch;
  afterEach(() => { window.fetch = real; });

  it("answers the setup boxes with a made-up command, ends it, and refuses everything else", async () => {
    let reached = false;
    window.fetch = (async () => { reached = true; return new Response("{}"); }) as typeof window.fetch;
    installMadeUpApi();
    const made = await (await fetch("/api/v1/propresenter-link/helper-install", { method: "POST" })).json() as { command: string; expires_at: string };
    expect(made.command).toContain("made-up");
    expect(Date.parse(made.expires_at)).toBeGreaterThan(Date.now() + 29 * 60_000);
    expect(await (await fetch("/api/v1/lights/lightkey/helper-install", { method: "DELETE" })).json()).toEqual({ ended: true });
    expect((await fetch("/api/v1/settings")).status).toBe(404);
    expect(reached).toBe(false);
    await fetch("/some-asset.png");
    expect(reached).toBe(true);
  });
});
