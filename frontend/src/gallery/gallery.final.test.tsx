import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { builtFromLine, type BuildInfo } from "./build";
import { Frame, Gallery } from "./Gallery";
import { installMadeUpApi } from "./madeUpApi";

/** The finished gallery: the commit it was built from, one frame per connection side by side, the panel order and the worked example. */
const real = window.fetch;
beforeEach(() => { installMadeUpApi(); window.localStorage.clear(); });
afterEach(() => { window.fetch = real; });

describe("the gallery says what it was built from", () => {
  it("writes the commit, an uncommitted-changes warning and the time", () => {
    const info: BuildInfo = { commit: "abc12345", builtAt: "2026-11-18 20:00 UTC", dirty: false };
    expect(builtFromLine(info)).toBe("Last built from abc12345 on 2026-11-18 20:00 UTC");
    expect(builtFromLine({ ...info, dirty: true })).toBe("Last built from abc12345 + uncommitted changes on 2026-11-18 20:00 UTC");
    expect(builtFromLine({ commit: "unknown", builtAt: "", dirty: false })).toBe("Last built from unknown");
  });

  it("shows the line at the top of the page, always", () => {
    render(<Gallery />);
    expect(screen.getByTestId("gallery-built-from")).toHaveTextContent(/^Last built from \S+/);
  });
});

describe("side by side", () => {
  it("has a view switch, and shows one frame per connection at the chosen width", async () => {
    const user = userEvent.setup();
    render(<Gallery />);
    expect(screen.getByRole("button", { name: "One column" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByTestId("gallery-columns")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Side by side" }));
    expect(screen.getByRole("button", { name: "Side by side" })).toHaveAttribute("aria-pressed", "true");
    const columns = screen.getByTestId("gallery-columns");
    const names = within(columns).getAllByRole("heading", { level: 2 }).map((node) => node.textContent);
    expect(names).toEqual(["Presentation", "Playback / MIDI", "Lighting", "Services", "StagePilot"]);
    const frames = within(columns).getAllByTestId("gallery-column-frame");
    expect(frames.map((frame) => frame.getAttribute("src"))).toEqual([
      "?frame=1&only=presentation,page-presentation,presenter-link",
      "?frame=1&only=playback,page-playback",
      "?frame=1&only=lighting,page-lighting",
      "?frame=1&only=page-services",
      "?frame=1&only=page-backend,page-remote",
    ]);
    const section = (name: string) => within(columns).getByRole("region", { name });
    expect(section("Lighting")).toHaveStyle({ width: "1280px" });
    await user.click(screen.getByRole("button", { name: "360 px" }));
    expect(section("Lighting")).toHaveStyle({ width: "360px" });
    await user.click(screen.getByRole("button", { name: "One column" }));
    expect(screen.getByTestId("gallery-frame")).toHaveStyle({ width: "360px" });
  });

  it("a frame with `only` draws just those groups, and none of the page's notes", () => {
    render(<Frame only={["page-services"]} />);
    expect(Array.from(document.querySelectorAll("[data-group]")).map((node) => node.getAttribute("data-group"))).toEqual(["page-services"]);
    expect(document.querySelector("[data-gallery-frame]")?.textContent).not.toMatch(/Press the Details button/);
  });
});

describe("the panel order and the worked example", () => {
  it("shows PanelLayout with every slot, in order, and with the summary first", () => {
    render(<Frame only={["template-layout"]} />);
    const order = (id: string) => Array.from(document.querySelectorAll(`#${id} [data-slot-sample]`)).map((node) => node.getAttribute("data-slot-sample"));
    expect(order("example-order")).toEqual(["alert lines", "connection", "settings", "actions", "optional", "content", "notices", "saveBar", "summary"]);
    expect(order("example-summary-top")).toEqual(["summary", "connection", "settings", "actions", "optional", "content", "notices", "saveBar"]);
  });

  it("draws the new-connection example from the templates, and it answers a test", async () => {
    const user = userEvent.setup();
    render(<Frame only={["template-layout"]} />);
    const page = document.getElementById("acme-configuration")!;
    expect(page.querySelector("[data-section=connection]")).not.toBeNull();
    await user.clear(within(page).getByLabelText("Host"));
    await user.type(within(page).getByLabelText("Host"), "bad");
    await user.click(within(page).getByRole("button", { name: "Test connection" }));
    await waitFor(() => expect(within(page).getByText("Check the host and the port, then test again.")).toBeInTheDocument());
    expect(within(page).getByText("Acme did not answer.")).toBeInTheDocument();
  });
});
