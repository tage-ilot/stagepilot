import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";

import { helperStatus, linkController, localStatus, presentation } from "../test/propresenterLinkFixtures";
import { ProPresenterSetupPanel } from "./ProPresenterSetupPanel";
import { PlaybackApiPanel } from "./PlaybackApiPanel";
import { rememberAddress } from "../hooks/useRecentAddresses";
import { libraryController } from "../test/libraryFixtures";
import { playbackStatus } from "../test/playbackFixtures";

vi.mock("../api", async (original) => ({ ...(await original<typeof import("../api")>()), getProPresenterPlaylists: vi.fn().mockResolvedValue({ playlists: [] }) }));
void api;

const panel = (link?: ReturnType<typeof linkController>, over: Partial<Parameters<typeof ProPresenterSetupPanel>[0]> = {}) => render(
  <ProPresenterSetupPanel error={null} link={link} message={null} onClose={vi.fn()} onRefreshTimers={vi.fn()} onSave={vi.fn()} onTest={vi.fn()} pendingOperation={null} propresenter={presentation()} {...over} />,
);
const block = () => screen.getByRole("region", { name: "ProPresenter Live Link" });
beforeEach(() => window.localStorage.clear());
const before = (a: Element, b: Element) => Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);

describe("ProPresenterSetupPanel: the Live Link block is where the other setup panels put theirs", () => {
  it("is the first block under the header, in the Playback panel's wrapper (mt-6 space-y-4, block !mt-0)", () => {
    const { container } = panel(linkController(helperStatus("ready")));
    const wrapper = block().parentElement as HTMLElement;
    expect(wrapper).toHaveClass("mt-6", "space-y-4");
    expect(wrapper.firstElementChild).toBe(block());
    expect(block()).toHaveClass("live-link", "!mt-0", "rounded-ds-xl", "border", "border-edge", "bg-surface-panel-strong");
    // Same wrapper as the Playback panel's own first block.
    const playback = render(<PlaybackApiPanel library={libraryController("ready")} playback={{ status: playbackStatus({}), events: [], error: null, message: null, pending: null, saveArrangement: vi.fn(), save: vi.fn(), selectSource: vi.fn(), scan: vi.fn(), cancelScan: vi.fn(), discover: vi.fn(), refreshFromLibrary: vi.fn(), libraryRead: null }} />);
    const playbackBlock = playback.container.firstElementChild!.firstElementChild!;
    expect(playbackBlock.parentElement!.className).toContain("mt-6 space-y-4");
    expect(playbackBlock.className).toContain("!mt-0");
    // The header comes first, then the block, then the rest.
    const header = within(container).getByRole("heading", { name: "Presentation" });
    expect(before(header, block())).toBe(true);
  });

  it("DOM order: header, alerts, block, Detected timer, Timeout, Look, Refresh, the note, Save bar, summary list", async () => {
    const { container } = panel(linkController(helperStatus("ready")), { message: "Saved." });
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "192.0.2.10" } });
    const order = [
      within(container).getByRole("heading", { name: "Presentation" }),
      screen.getByText("Saved."),
      block(),
      screen.getByRole("combobox", { name: /Detected timer/i }),
      screen.getByLabelText("Timeout (seconds)"),
      screen.getByRole("combobox", { name: "ProPresenter Look" }),
      screen.getByRole("button", { name: "Refresh timers and Looks" }),
      screen.getByText(/Refresh uses the saved settings/),
      screen.getByRole("button", { name: "Save configuration" }),
      container.querySelector("dl")!,
    ];
    for (let i = 0; i < order.length - 1; i += 1) expect(before(order[i]!, order[i + 1]!), `${i} before ${i + 1}`).toBe(true);
  });

  it("holds the connection: Host, API, Scan and Connect are inside the block, above the status line and the setup box", () => {
    panel(linkController(helperStatus("not_installed")));
    const inside = within(block());
    for (const control of [inside.getByLabelText("Host"), inside.getByLabelText("ProPresenter API port"), inside.getByRole("button", { name: "Search" }), inside.getByRole("button", { name: "Connect" })]) expect(block()).toContainElement(control);
    const lines = screen.getByTestId("propresenter-live-link-sub");
    const row = screen.getByTestId("propresenter-live-link-line");
    // The connection row is on top (DOM order changed on purpose: it used to sit under the status lines).
    expect(before(row, lines)).toBe(true);
    expect(before(row, screen.getByTestId("propresenter-install"))).toBe(true);
    // Everything that is not the connection stays outside of it.
    for (const outside of [screen.getByRole("combobox", { name: /Detected timer/i }), screen.getByLabelText("Timeout (seconds)"), screen.getByLabelText("ProPresenter Look"), screen.getByRole("button", { name: "Refresh timers and Looks" }), screen.getByRole("button", { name: "Save configuration" })]) expect(block()).not.toContainElement(outside);
  });

  it("the recent addresses are the arrow's list inside the Host box (no chip row under the row), and one pick fills Host", async () => {
    rememberAddress("presentation", "10.30.0.211");
    panel(linkController(helperStatus("ready")));
    expect(within(block()).queryByRole("group", { name: "Recent addresses" })).toBeNull();
    expect(within(block()).queryByRole("button", { name: "Use 10.30.0.211" })).toBeNull();
    const host = within(block()).getByRole("combobox", { name: "Host" });
    const row = screen.getByTestId("propresenter-live-link-line");
    expect(row).toContainElement(host);
    expect(row).toContainElement(within(block()).getByRole("button", { name: "Show recent and found addresses" }));
    await userEvent.setup().click(within(block()).getByRole("button", { name: "Show recent and found addresses" }));
    expect(within(screen.getByRole("group", { name: "Recent" })).getAllByRole("option").map((option) => option.textContent)).toEqual(["10.30.0.211"]);
    await userEvent.setup().click(screen.getByRole("option", { name: "10.30.0.211" }));
    expect(screen.getByLabelText("Host")).toHaveValue("10.30.0.211");
    expect(within(block()).getByText("Not saved yet")).toBeInTheDocument();
  });

  it("is shown while the Presenter Link is still being read: the block says Checking, with the connection boxes", () => {
    panel(linkController(null));
    expect(screen.getByTestId("propresenter-link-status")).toHaveTextContent("Presenter Link: Checking…");
    expect(screen.getByTestId("propresenter-link-pill")).toHaveTextContent("Presenter Link: Checking");
    expect(within(block()).getByLabelText("Host")).toBeInTheDocument();
  });

  it("is shown before ProPresenter has been read at all (the panel still loading)", () => {
    panel(linkController(null), { propresenter: null });
    expect(block()).toBeInTheDocument();
    expect(screen.getByTestId("propresenter-api-pill")).toHaveTextContent("ProPresenter API: Checking");
    // Nothing has been read yet: Connect has nothing to save or check, and says why. Scan does not need it.
    expect(within(block()).getByRole("button", { name: "Connect" })).toBeDisabled();
    expect(within(block()).getByRole("button", { name: "Search" })).toBeEnabled();
  });

  it("without a controller the block still holds the connection and the ProPresenter API line, and draws no Presenter Link line", () => {
    panel();
    expect(within(block()).getByRole("button", { name: "Connect" })).toBeInTheDocument();
    expect(within(block()).getByLabelText("Host")).toBeInTheDocument();
    expect(screen.queryByTestId("propresenter-link-status")).toBeNull();
    expect(screen.queryByTestId("propresenter-link-pill")).toBeNull();
  });

  it("this computer: still no setup button, and the import-blocker line stays", () => {
    panel(linkController(localStatus({ can_import: false, import_blocker: "Allow StagePilot in System Settings > Privacy & Security > Accessibility, then try again." })));
    expect(screen.queryByRole("button", { name: /Set up the ProPresenter computer|Update the Presenter Link/ })).toBeNull();
    expect(within(block()).getByTestId("propresenter-link-note")).toHaveTextContent("cannot load a missing song");
    expect(within(block()).getByTestId("propresenter-link-local")).toBeInTheDocument();
  });

  it("a computer without the Presenter Link shows no warning anywhere in the panel", () => {
    panel(linkController(helperStatus("not_installed")));
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

describe("ProPresenterSetupPanel: tab order follows the new visual order", () => {
  it("Host, API, Scan, Connect, the Details button, then Detected timer, Timeout, Look, Refresh, Save", async () => {
    const user = userEvent.setup();
    panel(linkController(helperStatus("ready")));
    const seen: string[] = [];
    for (let i = 0; i < 14; i += 1) {
      await user.tab();
      const active = document.activeElement as HTMLInputElement;
      seen.push(active.getAttribute("aria-label") ?? active.labels?.[0]?.textContent?.trim() ?? active.textContent?.replace(/[\u25B8\u25BE]/g, "").trim() ?? "");
      if (active.textContent === "Save configuration") break;
    }
    expect(seen.filter((name) => !/^Close/.test(name))).toEqual([
      "Host", "ProPresenter API port", "Search", "Connect", "Presenter Link setup",
      "Detected timer", "Timeout (seconds)", "ProPresenter Look", "Refresh timers and Looks",
      // The Presenter Link section sits between the settings and the Save bar (it has no list to tick without a plan).
      "Playlist",
      "Save configuration",
    ]);
  });
});

describe("ProPresenterSetupPanel: the connection boxes lay out like the other Live Link blocks", () => {
  it("one row that wraps: Host takes the whole row on a narrow block and shares it on a wide one (live-link-host), API is a small box, Scan and Connect are fixed-size buttons", () => {
    panel(linkController(helperStatus("ready")));
    const row = screen.getByTestId("propresenter-live-link-line");
    expect(row).toHaveClass("flex", "flex-wrap", "items-end");
    const host = screen.getByLabelText("Host");
    const port = screen.getByLabelText("ProPresenter API port");
    const scan = screen.getByRole("button", { name: "Search" });
    const connect = screen.getByRole("button", { name: "Connect" });
    expect(host.closest("label")).toHaveClass("live-link-host", "min-w-0");
    expect(port.closest("label")).not.toHaveClass("live-link-host");
    for (const control of [host, port, scan, connect]) expect(control).toHaveClass("!h-10", "!min-h-10", "coarse:!h-11", "coarse:!min-h-[44px]");
    expect(host).toHaveClass("w-full", "font-mono");
    expect(port).toHaveClass("!w-[5.25rem]", "text-center", "tabular-nums");
    for (const button of [scan, connect]) {
      expect(button).toHaveClass("flex-none");
      expect(button).toHaveAttribute("data-tone", "presentation");
      expect(button).toHaveAttribute("data-variant", "tinted");
    }
    // The block is a container: the narrow and wide rules (stack below 34rem, side by side above) are the shared index.css ones.
    expect(block()).toHaveClass("live-link");
  });
});

describe("ProPresenterSetupPanel: the save flow is unchanged with the boxes inside the block", () => {
  it("edit the host: the marker and the warning border show in the block, Save sends it, Discard puts it back", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    panel(linkController(helperStatus("ready")), { onSave });
    expect(block().querySelector("[data-held-marker]")).toBeNull();
    await user.clear(screen.getByLabelText("Host"));
    await user.type(screen.getByLabelText("Host"), "192.0.2.10");
    expect(within(block()).getByText("Not saved yet")).toBeInTheDocument();
    expect(screen.getByLabelText("Host")).toHaveClass("!border-warning");
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-state", "held");
    expect(screen.getByText(/Refresh uses the saved settings/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Discard" }));
    expect(screen.getByLabelText("Host")).toHaveValue("192.168.1.60");
    expect(block().querySelector("[data-held-marker]")).toBeNull();
    expect(onSave).not.toHaveBeenCalled();
    await user.clear(screen.getByLabelText("Host"));
    await user.type(screen.getByLabelText("Host"), "192.0.2.10");
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ host: "192.0.2.10", port: 1025 }));
  });

  it("a held API is marked in the block too, and a bad API port is said in words with an alert wired to the box", async () => {
    const user = userEvent.setup();
    panel(linkController(helperStatus("ready")));
    await user.clear(screen.getByLabelText("ProPresenter API port"));
    await user.type(screen.getByLabelText("ProPresenter API port"), "99999");
    expect(within(block()).getByText("Not saved yet")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    const alert = within(block()).getByRole("alert");
    expect(alert).toHaveTextContent("Enter the API port from 1 to 65535.");
    expect(screen.getByLabelText("ProPresenter API port")).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByLabelText("ProPresenter API port").getAttribute("aria-describedby")).toContain(alert.id);
  });

  it("Connect with nothing held runs the same test as before, and waits while a request is in flight", async () => {
    const onTest = vi.fn();
    const { unmount } = panel(linkController(helperStatus("ready")), { onTest });
    await userEvent.setup().click(within(block()).getByRole("button", { name: "Connect" }));
    expect(onTest).toHaveBeenCalledTimes(1);
    unmount();
    panel(linkController(helperStatus("ready")), { pendingOperation: "test" });
    expect(within(block()).getByRole("button", { name: "Connecting…" })).toBeDisabled();
    expect(screen.getByLabelText("Host")).toBeDisabled();
  });
});

describe("ProPresenterSetupPanel: the two names", () => {
  it("say ProPresenter API and Presenter Link in the pills, the lines, the hover text and the live region", () => {
    panel(linkController(helperStatus("ready")));
    expect(screen.getByTestId("propresenter-api-pill")).toHaveTextContent("ProPresenter API: Connected");
    expect(screen.getByTestId("propresenter-link-pill")).toHaveTextContent("Presenter Link: Connected");
    expect(screen.getByTestId("propresenter-api-pill")).toHaveAttribute("title", expect.stringMatching(/^ProPresenter API: Connected\./));
    expect(screen.getByTestId("propresenter-link-pill")).toHaveAttribute("aria-label", expect.stringMatching(/^Presenter Link: Connected\./));
    const live = screen.getByTestId("propresenter-link-status").closest("[aria-live]")!;
    expect(live).toHaveTextContent("ProPresenter API: Connected to ProPresenter at 192.168.1.60. · Presenter Link: Ready, version 0.1.0.");
    expect(document.body.textContent).not.toContain("ProPresenter Link");
  });
});
