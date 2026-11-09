import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import type { SettingsResponse } from "../types";
import { SettingsNoticeBanner } from "./SettingsNoticeBanner";
import { DEGRADED_LEAD, NEWER_LEAD, NOTICE_LEAD, resetDismissedSettingsNotices, settingsNotices } from "./settingsNotices";

const base = { planning_center_secret_saved: true, warning: null, restart_required: false } as unknown as SettingsResponse;
const withNotice = (settings_notice: string | null): SettingsResponse => ({ ...base, settings_notice });
const setAside = (song: string) => `The lighting data for ${song} was damaged and set aside; record it again.`;

beforeEach(() => { resetDismissedSettingsNotices(); });

describe("SettingsNoticeBanner: a song that was set aside", () => {
  it("says which song, in the backend's own plain sentence, on the main screen", () => {
    render(<SettingsNoticeBanner settings={withNotice(setAside("Ever Be"))} where="main" />);
    const banner = screen.getByTestId("settings-notice");
    expect(within(banner).getByText(NOTICE_LEAD)).toBeInTheDocument();
    expect(screen.getByTestId("settings-notice-text")).toHaveTextContent("The lighting data for Ever Be was damaged and set aside; record it again.");
    // calm: a status, not an alert that interrupts, and still the info tone (not the warning one)
    expect(banner).toHaveAttribute("role", "status");
    expect(banner).toHaveAttribute("data-tone", "info");
    expect(banner.className).toContain("border-info/30");
    expect(within(banner).getByText(NOTICE_LEAD).className).toContain("text-info-text");
  });

  it("shows the same notice on the Lighting screen", () => {
    render(<SettingsNoticeBanner settings={withNotice(setAside("Ever Be"))} where="lighting" />);
    expect(screen.getByTestId("settings-notice-text")).toHaveTextContent("Ever Be");
  });

  it("keeps a very long song name whole and lets it wrap instead of pushing the button away", () => {
    const long = `Ever Be ${"(Reprise, Extended Live Version) ".repeat(12)}`.trim();
    render(<SettingsNoticeBanner settings={withNotice(setAside(long))} where="main" />);
    const text = screen.getByTestId("settings-notice-text");
    expect(text.textContent).toContain(long);
    expect(text.className).toContain("[overflow-wrap:anywhere]");
    expect(screen.getByRole("button", { name: /Got it/ })).toBeInTheDocument();
  });

  it("names every song when several were set aside", () => {
    render(<SettingsNoticeBanner settings={withNotice(`${setAside("Ever Be")} ${setAside("Holy Forever")}`)} where="main" />);
    const text = screen.getByTestId("settings-notice-text").textContent ?? "";
    expect(text).toContain("Ever Be");
    expect(text).toContain("Holy Forever");
  });

  it("draws nothing when there is no notice: null, undefined, blank, or no settings loaded yet", () => {
    for (const settings of [withNotice(null), withNotice("   "), base, null, undefined]) {
      const { container, unmount } = render(<SettingsNoticeBanner settings={settings} where="main" />);
      expect(container).toBeEmptyDOMElement();
      unmount();
    }
  });
});

describe("SettingsNoticeBanner: the whole file could not be read", () => {
  const degraded = (reason: string | null): SettingsResponse => ({ ...base, settings_degraded: true, settings_degraded_reason: reason });

  it("says StagePilot started with the defaults", () => {
    render(<SettingsNoticeBanner settings={degraded("not valid JSON")} where="main" />);
    const banner = screen.getByTestId("settings-degraded");
    expect(within(banner).getByText(DEGRADED_LEAD)).toBeInTheDocument();
    expect(banner).toHaveTextContent("default settings");
    expect(banner).not.toHaveTextContent("not valid JSON");
  });

  it("wears the warning tone: the warning colours, a heading in the warning text colour, and role alert", () => {
    render(<SettingsNoticeBanner settings={degraded("not valid JSON")} where="main" />);
    const banner = screen.getByTestId("settings-degraded");
    expect(banner).toHaveAttribute("data-tone", "warning");
    expect(banner).toHaveAttribute("role", "alert");
    expect(banner.className).toContain("border-warning/30");
    expect(banner.className).toContain("bg-warning/10");
    expect(within(banner).getByText(DEGRADED_LEAD).className).toContain("text-warning-text");
    expect(within(banner).getByText(DEGRADED_LEAD).className).not.toContain("text-info-text");
  });

  it("the 'newer StagePilot' notice is a warning too, with its heading still in plain words", () => {
    render(<SettingsNoticeBanner settings={degraded("schema_version 3 is newer than 2")} where="main" />);
    const banner = screen.getByTestId("settings-degraded");
    expect(banner).toHaveAttribute("data-tone", "warning");
    expect(banner).toHaveAttribute("role", "alert");
    expect(within(banner).getByText(NEWER_LEAD).className).toContain("text-warning-text");
  });

  it("can be closed for the session, like the song notice", async () => {
    render(<SettingsNoticeBanner settings={degraded("not valid JSON")} where="main" />);
    await userEvent.setup().click(screen.getByRole("button", { name: `Got it: ${DEGRADED_LEAD}` }));
    expect(screen.queryByTestId("settings-degraded")).not.toBeInTheDocument();
    expect(window.sessionStorage.getItem("stagepilot.dismissed-settings-notices")).toContain("main|degraded:corrupt");
  });

  it("says so differently when the file is from a newer StagePilot", () => {
    render(<SettingsNoticeBanner settings={degraded("schema_version 3 is newer than 2")} where="main" />);
    const banner = screen.getByTestId("settings-degraded");
    expect(within(banner).getByText(NEWER_LEAD)).toBeInTheDocument();
    expect(banner).toHaveTextContent("will not change the saved file");
  });

  it("is not drawn when settings_degraded is false, and not on the Lighting screen", () => {
    const { container, rerender } = render(<SettingsNoticeBanner settings={{ ...base, settings_degraded: false, settings_degraded_reason: null }} where="main" />);
    expect(container).toBeEmptyDOMElement();
    rerender(<SettingsNoticeBanner settings={degraded("not valid JSON")} where="lighting" />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows both when a damaged file also carries a song notice, the file first", () => {
    render(<SettingsNoticeBanner settings={{ ...degraded("not valid JSON"), settings_notice: setAside("Ever Be") }} where="main" />);
    const kinds = Array.from(document.querySelectorAll("[data-kind]")).map((node) => node.getAttribute("data-kind"));
    expect(kinds).toEqual(["degraded", "notice"]);
  });
});

describe("SettingsNoticeBanner: spacing and readability", () => {
  const both: SettingsResponse = { ...base, settings_degraded: true, settings_degraded_reason: "not valid JSON", settings_notice: setAside("Ever Be") };

  it("on the main screen the gap between banners and the gap below the last one are the same step (20px)", () => {
    render(<SettingsNoticeBanner settings={both} where="main" />);
    const stack = screen.getByTestId("settings-notices-main");
    expect(stack.className).toContain("flex-col");
    expect(stack.className).toContain("gap-5");
    expect(stack.className).toContain("mb-5");
    expect(stack.className).not.toMatch(/space-y-/);
  });

  it("on the Lighting screen the same rule at the panel's 24px step, above and below", () => {
    render(<SettingsNoticeBanner settings={withNotice(setAside("Ever Be"))} where="lighting" />);
    const stack = screen.getByTestId("settings-notices-lighting");
    expect(stack.className).toContain("gap-6");
    expect(stack.className).toContain("my-6");
  });

  it("the stack adds no side margin or padding, so the banners keep the same left and right edges as the cards beside them", () => {
    render(<SettingsNoticeBanner settings={both} where="main" />);
    const stack = screen.getByTestId("settings-notices-main");
    expect(stack.className).not.toMatch(/(^|\s)(m[xlr]|p[xlr])-/);
    stack.querySelectorAll('[data-part="base"]').forEach((node) => expect(node.className).not.toMatch(/(^|\s)(m[xlr]|p[xlr])-/));
  });

  it("sits each banner on an opaque dark base, so the words do not depend on the photo behind", () => {
    render(<SettingsNoticeBanner settings={both} where="main" />);
    const bases = document.querySelectorAll('[data-part="base"]');
    expect(bases).toHaveLength(2);
    bases.forEach((node) => {
      expect(node.className).toContain("bg-surface-950");
      expect(node.querySelector("[data-kind]")).not.toBeNull();
    });
  });
});

describe("SettingsNoticeBanner: Got it", () => {
  it("closes the banner for this session, and it stays closed when the screen is drawn again", async () => {
    const settings = withNotice(setAside("Ever Be"));
    const { unmount } = render(<SettingsNoticeBanner settings={settings} where="main" />);
    await userEvent.setup().click(screen.getByRole("button", { name: /Got it/ }));
    expect(screen.queryByTestId("settings-notice")).not.toBeInTheDocument();
    unmount();
    render(<SettingsNoticeBanner settings={settings} where="main" />);
    expect(screen.queryByTestId("settings-notice")).not.toBeInTheDocument();
  });

  it("is remembered in the session only, and closing it on one screen does not hide it on the other", async () => {
    const settings = withNotice(setAside("Ever Be"));
    render(
      <>
        <SettingsNoticeBanner settings={settings} where="main" />
        <SettingsNoticeBanner settings={settings} where="lighting" />
      </>,
    );
    await userEvent.setup().click(within(screen.getByTestId("settings-notices-main")).getByRole("button", { name: /Got it/ }));
    expect(screen.queryByTestId("settings-notices-main")).not.toBeInTheDocument();
    expect(screen.getByTestId("settings-notices-lighting")).toBeInTheDocument();
    expect(window.sessionStorage.getItem("stagepilot.dismissed-settings-notices")).toContain("main|notice:");
    expect(window.localStorage.getItem("stagepilot.dismissed-settings-notices")).toBeNull();
  });

  it("a different song set aside later is a new notice and is shown again", async () => {
    const { rerender } = render(<SettingsNoticeBanner settings={withNotice(setAside("Ever Be"))} where="main" />);
    await userEvent.setup().click(screen.getByRole("button", { name: /Got it/ }));
    expect(screen.queryByTestId("settings-notice")).not.toBeInTheDocument();
    rerender(<SettingsNoticeBanner settings={withNotice(setAside("Holy Forever"))} where="main" />);
    expect(screen.getByTestId("settings-notice-text")).toHaveTextContent("Holy Forever");
  });

  it("closing the file notice leaves the song notice, and the other way round", async () => {
    render(<SettingsNoticeBanner settings={{ ...base, settings_degraded: true, settings_degraded_reason: "not valid JSON", settings_notice: setAside("Ever Be") }} where="main" />);
    await userEvent.setup().click(within(screen.getByTestId("settings-degraded")).getByRole("button", { name: /Got it/ }));
    expect(screen.queryByTestId("settings-degraded")).not.toBeInTheDocument();
    expect(screen.getByTestId("settings-notice")).toBeInTheDocument();
  });
});

describe("settingsNotices", () => {
  it("is empty without anything to say", () => {
    expect(settingsNotices(withNotice(null))).toEqual([]);
    expect(settingsNotices(null)).toEqual([]);
  });
});
