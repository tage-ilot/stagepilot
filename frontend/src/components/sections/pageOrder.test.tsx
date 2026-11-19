import { render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { installMadeUpApi } from "../../gallery/madeUpApi";
import { backendPages, lightingPages, playbackPages, presentationPages, servicesPages } from "../../gallery/PageStates";
import { remotePages } from "../../gallery/RemoteStates";
import { BODY_SLOTS, PANEL_ORDER, bodyOrder, type BodySlot } from "./panelOrder";

/**
 * ONE test per configuration page: it draws the REAL page (made-up data) and reads what `PanelLayout` put in the body, top to bottom,
 * by the section template each part is (a template carries its own `data-section`). Two things must hold, so an accidental reorder fails:
 *   1. the parts come in the order `PanelLayout` writes (`PANEL_ORDER`, with `summaryAt` for the page that opens with its summary);
 *   2. the page is exactly the list pinned here (a section added, dropped or moved needs this list changed on purpose).
 * `other` is what only that page has (the song list, its own connection row); it is not a template, so it is pinned but not ranked.
 */
type Part = BodySlot | "other";

const partOf = (node: Element): Part => {
  if (node.matches("[data-section=connection]")) return "connection";
  if (node.matches("[data-section=settings]")) return "settings";
  if (node.matches("[data-section=actions]")) return "actions";
  if (node.matches("[data-section=summary]")) return "summary";
  if (node.matches("[data-section=band], [data-variant=advanced], [data-variant=activity], [data-variant=feature]")) return "optional";
  if (node.matches("[data-save-bar]")) return "saveBar";
  if (node.matches("[data-part=notice]")) return "notices";
  return "other";
};

const before = (first: Element, second: Element) => Boolean(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING);

/** The page's panel: the header first, the alert lines (when there are any) next, then the body. Returns the body's parts. */
function partsOf(panel: HTMLElement): Part[] {
  const header = panel.querySelector("[data-setup-panel-header]");
  const body = panel.querySelector("[data-panel-body]");
  expect(header, "the page has its header").not.toBeNull();
  expect(body, "the page is built from PanelLayout").not.toBeNull();
  expect(before(header!, body!), "the header is above the body").toBe(true);
  const alerts = panel.querySelector("[data-slot=alerts]");
  if (alerts) expect(before(header!, alerts) && before(alerts, body!), "the alert lines sit between the header and the body").toBe(true);
  return Array.from(body!.children).map(partOf);
}

/** Every ranked part must come at or after the one before it, by PanelLayout's own order. */
function expectPanelOrder(parts: Part[], summaryAt: "top" | "bottom" = "bottom") {
  const order = bodyOrder(summaryAt);
  const ranks = parts.filter((part): part is BodySlot => part !== "other").map((part) => order.indexOf(part));
  expect(ranks, `parts ${parts.join(", ")} against ${order.join(", ")}`).toEqual([...ranks].sort((a, b) => a - b));
}

const real = window.fetch;
beforeEach(() => { installMadeUpApi(); window.localStorage.clear(); });
afterEach(() => { window.fetch = real; });
const draw = (states: { id: string; render: () => React.ReactNode }[], id: string) => render(<>{states.find((state) => state.id === id)!.render()}</>);

describe("every configuration page follows PanelLayout's order", () => {
  it("knows the order: header, alert lines, connection, settings, actions, optional, content, notices, Save bar, summary", () => {
    expect([...PANEL_ORDER]).toEqual(["header", "alerts", "connection", "settings", "actions", "optional", "content", "notices", "saveBar", "summary"]);
    expect([...BODY_SLOTS]).toEqual(PANEL_ORDER.slice(2));
    expect([...bodyOrder("top")]).toEqual(["summary", "connection", "settings", "actions", "optional", "content", "notices", "saveBar"]);
  });

  it("Presentation", async () => {
    draw(presentationPages, "clean");
    const panel = document.getElementById("propresenter-configuration")!;
    await waitFor(() => expect(panel.querySelector("[data-section=connection]")).not.toBeNull());
    const parts = partsOf(panel);
    expectPanelOrder(parts);
    expect(parts).toEqual(["connection", "settings", "settings", "actions", "saveBar", "summary"]);
  });

  it("Services (Planning Center)", () => {
    draw(servicesPages, "signed-out");
    const parts = partsOf(document.getElementById("planning-center-configuration")!);
    expectPanelOrder(parts);
    // The sign-in notice and the optional Manual API Connection share the connection slot (one wrapper, "other").
    expect(parts).toEqual(["other", "settings", "actions", "saveBar", "summary"]);
  });

  it("StagePilot (the Status cards first)", () => {
    draw(backendPages, "clean");
    const parts = partsOf(document.getElementById("backend-configuration")!);
    expectPanelOrder(parts, "top");
    expect(parts).toEqual(["summary", "optional", "other", "saveBar"]);
  });

  it("Playback / MIDI (both views)", async () => {
    const api = draw(playbackPages, "api-open");
    const apiParts = partsOf(document.getElementById("midi-configuration")!);
    expectPanelOrder(apiParts);
    api.unmount();
    draw(playbackPages, "midi");
    const midiParts = partsOf(document.getElementById("midi-configuration")!);
    expectPanelOrder(midiParts);
    // The Save bar of this page sits under the body (`below`), shared by both views.
    expect({ apiParts, midiParts }).toEqual({ apiParts: ["connection", "optional"], midiParts: ["other", "optional", "other"] });
  });

  it("Lighting (OSC and MIDI views)", async () => {
    const osc = draw(lightingPages, "osc");
    await waitFor(() => expect(document.querySelector("#lighting-configuration [data-section=connection]")).not.toBeNull());
    const oscParts = partsOf(document.getElementById("lighting-configuration")!);
    expectPanelOrder(oscParts);
    osc.unmount();
    draw(lightingPages, "midi");
    await waitFor(() => expect(document.querySelector("#lighting-configuration [data-section=settings]")).not.toBeNull());
    const midiParts = partsOf(document.getElementById("lighting-configuration")!);
    expectPanelOrder(midiParts);
    expect({ oscParts, midiParts }).toEqual({ oscParts: ["connection", "optional", "other", "saveBar"], midiParts: ["settings", "actions", "optional", "optional", "other", "saveBar"] });
  });

  it("Remote access (inside StagePilot): the link group, then the people group", () => {
    draw(remotePages, "connected");
    const groups = Array.from(document.querySelectorAll("[data-remote-group]")).map((node) => node.getAttribute("data-remote-group"));
    expect(groups).toEqual(["link", "people"]);
  });
});
