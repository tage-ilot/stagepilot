import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Frame } from "./Gallery";

const sample = (id: string) => document.querySelector(`[data-sample="template-audit-${id}"]`) as HTMLElement;

describe("the design-audit follow-up states in the gallery", () => {
  it("lists every new state", () => {
    render(<Frame only={["template-audit"]} />);
    const ids = Array.from(document.querySelectorAll('[data-group="template-audit"] [data-sample]')).map((n) => n.getAttribute("data-sample")?.replace("template-audit-", ""));
    expect(ids).toEqual(["reasons-services", "reasons-midi", "event-stream", "check-list", "save-bar-held", "save-bar-auto", "song-order-learning"]);
  });

  it("switched-off buttons carry their reason as a title and as a described-by copy", () => {
    render(<Frame only={["template-audit"]} />);
    const button = within(sample("reasons-services")).getByRole("button", { name: "Load service types" });
    expect(button).toBeDisabled();
    expect(button.closest("span[title]")).toHaveAttribute("title", "Sign in to Planning Center first.");
    expect(button).toHaveAccessibleDescription("Sign in to Planning Center first.");
  });

  it("the event stream is in words: no dotted name, no plugin.status_changed row, the raw name in the hover text", () => {
    render(<Frame only={["template-audit"]} />);
    const text = Array.from(sample("event-stream").querySelectorAll(".event-row")).map((row) => row.textContent).join(" | ");
    expect(text).toContain("Playback connected");
    expect(text).toContain("Service plan loaded");
    expect(text).toContain("Timer stopped");
    expect(text).not.toMatch(/\w+\.\w+/);
    expect(sample("event-stream").querySelectorAll(".event-row")).toHaveLength(6);
    expect(sample("event-stream").querySelector("[title='service_plan.loaded · planning_center']")).not.toBeNull();
  });

  it("the check list marks a blocking row ×, an optional one !, a fine one ✓", () => {
    render(<Frame only={["template-audit"]} />);
    const tones = Array.from(sample("check-list").querySelectorAll("[data-row-tone]")).map((n) => `${n.getAttribute("data-row-tone")}:${n.textContent}`);
    expect(tones).toEqual(["ok:\u2713", "blocking:\u00D7", "blocking:\u00D7", "optional:!"]);
  });

  it("the tinted Save bar is opaque below lg; the held one is already a solid colour", () => {
    render(<Frame only={["template-audit"]} />);
    expect(sample("save-bar-auto").querySelector("[data-save-bar]")).toHaveClass("max-lg:bg-surface-panel", "max-lg:border-t-2");
    expect(sample("save-bar-held").querySelector("[data-save-bar]")).toHaveClass("bg-surface-900");
  });

  it("the Song order header says the learning state with one neutral chip", () => {
    render(<Frame only={["template-audit"]} />);
    expect(within(sample("song-order-learning")).getByText("Learning order (1 of 4)")).toBeInTheDocument();
    expect(screen.queryByText(/is song \d in the plan but song/)).toBeNull();
  });
});
