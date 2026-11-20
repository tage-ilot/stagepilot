import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { LightkeyStatus } from "../types";
import { lightkeyBoxProps } from "./helperBoxProps";
import { UPDATE_HELPER_TIP } from "./lightkeyProject";
import { HelperSetupBox } from "./sections";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  startLightkeyHelperInstall: vi.fn(),
  endLightkeyHelperInstall: vi.fn(),
}));

/** D1 (design audit): the Lightkey helper is the "Lighting Link" to an operator; "helper" stays in code and docs. */
const status = (over: Partial<LightkeyStatus>): LightkeyStatus => ({
  api_url: "http://192.168.1.20:8770", link: "unreachable", detail: "No answer.", active_count: null, recording: false, recording_song: null,
  replay: true, replay_blocked: null, replaying_song: null, message: null, ...over,
});

describe("Lighting Link wording", () => {
  it.each([
    ["not set up", status({})],
    ["older than the one StagePilot carries", status({ link: "connected", detail: null, helper: { outdated: true } })],
  ])("never says helper in the setup box (%s)", (_name, lightkey) => {
    const { container } = render(<HelperSetupBox {...lightkeyBoxProps(lightkey)} autoStart={false} />);
    expect(container.textContent ?? "").not.toMatch(/helper/i);
    // The aria labels and headings an operator hears say it too.
    expect([...container.querySelectorAll("[aria-label]")].map((node) => node.getAttribute("aria-label")).join(" ")).not.toMatch(/helper/i);
  });

  it("names the update box Update the Lighting Link and waits for the Lighting Link", () => {
    const props = lightkeyBoxProps(status({ link: "connected", detail: null, helper: { outdated: true } }));
    expect(props.wording.headings).toEqual({ setup: "Set up the Lighting computer", update: "Update the Lighting Link", command: "Lighting Link command" });
    expect(props.wording.waitingFor).toBe("the Lighting Link");
    expect(UPDATE_HELPER_TIP).toBe("Update the Lighting Link to use this.");
  });

  it("labels the command Capture script and keeps that it is for Lightkey in the title", () => {
    const { getByTestId } = render(<HelperSetupBox {...lightkeyBoxProps(status({}))} autoStart={false} />);
    // The label shows once the person asks for the command; the wording itself is the contract here.
    const label = render(<>{lightkeyBoxProps(status({})).wording.label}</>).container.firstElementChild;
    expect(label).toHaveTextContent("Capture script:");
    expect(label).toHaveAttribute("title", expect.stringMatching(/only for Lightkey/));
    expect(getByTestId("lightkey-install")).toBeInTheDocument();
  });
});
