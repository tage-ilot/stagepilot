import { render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { Frame } from "./Gallery";
import { installMadeUpApi } from "./madeUpApi";

/** The section gallery's second part: every template, in every state, on the real configuration pages (made-up data, no backend). */
const sample = (id: string) => document.querySelector(`[data-sample="${id}"]`) as HTMLElement;
const ids = (group: string) => Array.from(document.querySelectorAll(`[data-group="${group}"] [data-sample]`)).map((node) => node.getAttribute("data-sample")?.replace(`${group}-`, ""));
const real = window.fetch;
/** Plain DOM queries: the pages hold hundreds of options, which makes role queries very slow in jsdom. */
const text = (root: HTMLElement, words: string | RegExp) => Array.from(root.querySelectorAll<HTMLElement>("*")).filter((node) => node.children.length === 0 && (typeof words === "string" ? node.textContent === words : words.test(node.textContent ?? "")));
const button = (root: HTMLElement, words: RegExp) => Array.from(root.querySelectorAll<HTMLElement>("button")).find((node) => words.test(node.textContent ?? "")) as HTMLElement;
const control = (root: HTMLElement, label: string) => { const caption = Array.from(root.querySelectorAll("label")).find((node) => node.textContent?.trim().startsWith(label)); return document.getElementById(caption?.htmlFor ?? "") as HTMLInputElement | HTMLSelectElement; };
const WAIT = { timeout: 15000 };
beforeEach(() => { installMadeUpApi(); window.localStorage.clear(); });
afterEach(() => { window.fetch = real; });

describe("the section gallery: configuration pages", () => {
  it("lists every state of every page and every template", () => {
    render(<Frame />);
    expect(ids("page-presentation")).toEqual(["clean", "held", "look-chosen", "unavailable", "timer-none", "not-connected", "error", "busy-refresh", "busy-save", "disabled", "failed", "loading"]);
    expect(ids("page-services")).toEqual(["signed-out", "signed-in", "expired", "manual-held", "busy", "no-types", "error"]);
    expect(ids("page-backend")).toEqual(["clean", "advanced-open", "error", "disconnected", "failure"]);
    expect(ids("page-playback")).toEqual(["api-open", "api-closed", "midi", "midi-held"]);
    expect(ids("page-lighting")).toEqual(["osc", "midi", "midi-held", "midi-busy", "timeline-closed"]);
    expect(ids("page-remote")).toEqual(["connected", "persisted-open", "persisted-closed", "bootstrap", "confirm-disable", "confirm-regenerate", "error", "read-only"]);
    expect(ids("template-settings")).toEqual(["clean", "title", "held", "error", "busy", "columns-standard", "columns-inset", "columns-pair", "columns-triple", "columns-lead", "columns-single", "boxed"]);
    expect(ids("template-actions")).toEqual(["idle", "busy", "disabled", "primary"]);
    expect(ids("template-summary")).toEqual(["list-ok", "list-warn", "list-four", "cards", "cards-danger"]);
    expect(ids("template-notices")).toEqual(["boxes", "title-actions", "group", "lines"]);
    expect(ids("template-optional")).toEqual(["advanced-closed", "advanced-open", "activity", "feature", "forced", "band-open", "band-closed", "band-forced"]);
  });

  it("draws the Presentation page from the templates, in the fixed order, and reaches a held change and an error by doing what a person does", async () => {
    render(<Frame />);
    const clean = sample("page-presentation-clean");
    const sections = Array.from(clean.querySelectorAll("[data-section]")).map((node) => node.getAttribute("data-section"));
    // The Live Link block, the settings pair (Timer and Look), the actions, the Presenter Link band (it starts folded; inside: its own options, its song cards with no header row over them, and its button row), then the summary.
    expect(sections).toEqual(["connection", "settings", "actions", "band", "settings", "actions", "summary"]);
    expect(clean.querySelector("[data-panel-body]")).toBeInTheDocument();
    await waitFor(() => expect(sample("page-presentation-held").querySelectorAll("[data-held-marker]").length).toBeGreaterThanOrEqual(2), WAIT);
    await waitFor(() => expect(Array.from(sample("page-presentation-error").querySelectorAll("[role=alert]")).some((node) => /Enter a host\./.test(node.textContent ?? ""))).toBe(true), WAIT);
    // Busy: the section disables every control; the button says why in words.
    const busy = sample("page-presentation-busy-refresh");
    expect(control(busy, "Timer")).toBeDisabled();
    expect(control(busy, "Look")).toBeDisabled();
    expect(button(busy, /^Refreshing…/)).toBeTruthy();
    // The Presentation settings states: (current) by default, the saved Look, unavailable, no timer, not connected.
    expect(control(clean, "Timer")).toHaveDisplayValue("Song Countdown (current)");
    expect(control(clean, "Look")).toHaveDisplayValue("Worship (current)");
    expect(clean.querySelector("[data-panel-body]")!.textContent).not.toMatch(/Timeout|Current Look/);
    expect(control(sample("page-presentation-look-chosen"), "Look")).toHaveDisplayValue("Message");
    expect(control(sample("page-presentation-unavailable"), "Timer")).toHaveDisplayValue("Old Timer (unavailable)");
    expect(control(sample("page-presentation-unavailable"), "Look")).toHaveDisplayValue("l9 (unavailable)");
    expect(control(sample("page-presentation-timer-none"), "Timer")).toHaveDisplayValue("Choose a timer");
    expect(control(sample("page-presentation-not-connected"), "Look")).toBeDisabled();
    expect(sample("page-presentation-not-connected").textContent).toContain("Connect to ProPresenter first");
    // Warn words in the summary.
    expect(text(sample("page-presentation-disabled"), "Not found")[0]).toHaveAttribute("data-tone", "warn");
  }, 60000);

  it("shows each Services notice tone with its role, and the manual connection's held secret", async () => {
    render(<Frame />);
    const roleOf = (id: string, title: string) => text(sample(id), title)[0]?.closest("[role]")?.getAttribute("role");
    expect(roleOf("page-services-signed-out", "Sign in with Planning Center")).toBe("status");
    expect(roleOf("page-services-signed-in", "Connected to Planning Center")).toBe("status");
    expect(roleOf("page-services-expired", "Your Services connection has expired")).toBe("alert");
    await waitFor(() => expect(button(sample("page-services-manual-held"), /Manual API Connection/)).toHaveAttribute("aria-expanded", "true"), WAIT);
    await waitFor(() => expect(sample("page-services-manual-held").querySelectorAll("[data-held-marker]").length).toBeGreaterThanOrEqual(1), WAIT);
    expect(control(sample("page-services-busy"), "Timezone")).toBeDisabled();
    expect(control(sample("page-services-no-types"), "Service type")).toBeDisabled();
    expect(control(sample("page-services-no-types"), "Timezone")).toBeEnabled();
  }, 60000);

  it("shows the optional sections open, closed and remembered", async () => {
    render(<Frame />);
    expect(button(sample("page-backend-clean"), /^Advanced/)).toHaveAttribute("aria-expanded", "false");
    await waitFor(() => expect(button(sample("page-backend-advanced-open"), /^Advanced/)).toHaveAttribute("aria-expanded", "true"), WAIT);
    expect(button(sample("page-playback-api-open"), /Song order/)).toHaveAttribute("aria-expanded", "true");
    await waitFor(() => expect(button(sample("page-playback-api-closed"), /Song order/)).toHaveAttribute("aria-expanded", "false"), WAIT);
    expect(button(sample("page-remote-persisted-open"), /Remote access/)).toHaveAttribute("aria-expanded", "true");
    await waitFor(() => expect(button(sample("page-remote-persisted-closed"), /Remote access/)).toHaveAttribute("aria-expanded", "false"), WAIT);
    expect(button(sample("template-optional-forced"), /Remote access/)).toHaveAttribute("aria-expanded", "true");
  }, 60000);

  it("shows the summary cards with ok and danger words, and every notice tone", () => {
    render(<Frame />);
    expect(sample("page-backend-clean").querySelector("[data-section=summary] [data-tone=ok]")).toHaveTextContent("Connected");
    expect(sample("page-backend-disconnected").querySelector("[data-section=summary] [data-tone=danger]")).toHaveTextContent("Not connected");
    const tones = Array.from(sample("template-notices-boxes").querySelectorAll("[data-tone]")).map((node) => node.getAttribute("data-tone"));
    expect(tones).toEqual(["info", "success", "warning", "danger"]);
    expect(sample("template-notices-group").querySelector("[role=group][aria-label='Confirm disable Remote access']")).toBeInTheDocument();
  });

  it("draws the Lighting and MIDI pages with their settings group and action row", async () => {
    render(<Frame />);
    const midi = sample("page-lighting-midi");
    expect(midi.querySelector("[data-columns=lead] label")).toHaveTextContent("MIDI Output");
    expect(button(midi, /^Refresh outputs/)).toBeTruthy();
    expect(button(sample("page-lighting-midi-busy"), /^Refreshing…/)).toBeTruthy();
    await waitFor(() => expect(button(sample("page-lighting-timeline-closed"), /Song lighting timeline/)).toHaveAttribute("aria-expanded", "false"), WAIT);
    expect(button(midi, /Song lighting timeline/)).toHaveAttribute("aria-expanded", "true");
    expect(sample("page-playback-midi-held").querySelector("[data-columns=triple]")).toBeInTheDocument();
    await waitFor(() => expect(sample("page-lighting-midi-held").querySelectorAll("[data-held-marker]").length).toBeGreaterThanOrEqual(1), WAIT);
  }, 60000);
});
