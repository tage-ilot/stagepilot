import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Frame } from "./Gallery";

/** The Playback song-scan group: the ONE button in each of its states, with the label it should have and its reason when greyed out. */
const sample = (id: string) => document.querySelector(`[data-sample="song-scan-${id}"]`) as HTMLElement;
const button = (id: string) => within(sample(id)).getByRole("button", { name: /^Scan Playback (API|Library)$/ });

describe("the Playback song scan in the gallery", () => {
  it("lists the four labels and the greyed-out states", () => {
    render(<Frame only={["song-scan"]} />);
    const ids = Array.from(document.querySelectorAll('[data-group="song-scan"] [data-sample]')).map((node) => node.getAttribute("data-sample")?.replace("song-scan-", ""));
    expect(ids).toEqual(["library", "api", "library-failed", "reading", "off-not-connected", "off-playing", "off-running"]);
  });

  it.each([
    ["library", "Scan Playback Library", null],
    ["api", "Scan Playback API", null],
    ["library-failed", "Scan Playback API", null],
    ["off-not-connected", "Scan Playback API", "Connect to the Playback API first."],
    ["off-playing", "Scan Playback API", "Stop Playback to scan songs."],
    ["off-running", "Scan Playback API", "A scan is running."],
  ] as const)("%s: %s", (id, label, reason) => {
    render(<Frame only={["song-scan"]} />);
    expect(button(id)).toHaveTextContent(label);
    if (reason) {
      expect(button(id)).toBeDisabled();
      expect(button(id)).toHaveAttribute("title", reason);
      expect(button(id)).toHaveAccessibleDescription(reason);
    } else {
      expect(button(id)).toBeEnabled();
    }
    expect(within(sample(id)).queryByRole("button", { name: "Scan with Playback instead" })).toBeNull();
  });

  it("reading: the Library button keeps its words and shows it is busy", () => {
    render(<Frame only={["song-scan"]} />);
    expect(button("reading")).toHaveAttribute("aria-busy", "true");
    expect(screen.getAllByText(/^Scan Playback (API|Library)$/).length).toBeGreaterThanOrEqual(7);
  });
});
