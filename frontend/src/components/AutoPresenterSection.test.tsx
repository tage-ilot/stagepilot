import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { AUTO_PRESENTER_OPEN_KEY, FOLLOW_LABEL } from "../presenterFollow";
import { libraryController, libraryPlan, librarySong, librarySongOf, libraryStatus } from "../test/libraryFixtures";
import { followController, followStatus } from "../test/presenterFollowFixtures";
import { helperStatus, linkController } from "../test/propresenterLinkFixtures";
import { AutoPresenterSection } from "./AutoPresenterSection";

const TITLES = ["Holy Forever", "Battle Belongs", "Gratitude", "Firm Foundation"];
const plan = () => libraryPlan(TITLES.map((title, i) => librarySongOf(title, i + 1, { library: librarySong({ title, position: i + 1, setlist_song_id: 2000001 + i, content_id: 20000 + i }) })));
const READY = { link: () => linkController(helperStatus("ready")), library: () => libraryController("ready", { plan: plan() }) };
type Over = { link?: ReturnType<typeof linkController>; library?: ReturnType<typeof libraryController> };
const draw = (follow: ReturnType<typeof followController> | undefined = followController(), over: Over = {}) => render(
  <AutoPresenterSection follow={follow} library={over.library ?? READY.library()} link={over.link ?? READY.link()} />,
);
const summary = () => screen.getByTestId("auto-presenter-summary");
const toggle = () => screen.getByRole("button", { name: new RegExp(`^${FOLLOW_LABEL}`) });
const band = () => document.querySelector("[data-section=band]") as HTMLElement;
const off = (over = {}) => followController({ enabled: false, status: followStatus({ enabled: false, state: "off" }), ...over });
const locked = (over: Over, follow = off()) => draw(follow, over);
const NO_LINK: Over = { link: linkController(helperStatus("not_installed")) };
const NO_LIBRARY: Over = { library: libraryController("unreachable") };

beforeEach(() => window.localStorage.clear());
afterEach(() => window.localStorage.clear());

describe("Auto-Presenter, its own section", () => {
  it("is a card in the Presentation colour titled Auto-Presenter with ONE summary line and the switch on the header row", () => {
    draw(off());
    expect(band()).toHaveAttribute("data-tone", "presentation");
    expect(screen.getByRole("heading", { name: "Auto-Presenter" })).toBeInTheDocument();
    expect(summary()).toHaveTextContent(/^\u2212?Off$/);
    const header = band().querySelector("[data-part=band-header]") as HTMLElement;
    expect(within(header).getByRole("switch", { name: FOLLOW_LABEL })).toBeInTheDocument();
    // The switch is not in the body (no nested box); the body is below the header's rule.
    expect(within(band().querySelector("[data-part=body]") as HTMLElement).queryByRole("switch")).toBeNull();
  });

  it("is off by default, starts folded while it is off, and the switch turns it on through the controller", async () => {
    const follow = off();
    draw(follow);
    const box = screen.getByRole("switch", { name: FOLLOW_LABEL });
    expect(box).not.toBeChecked();
    expect(box).toBeEnabled();
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    await userEvent.setup().click(box);
    expect(follow.setEnabled).toHaveBeenCalledWith(true);
  });

  it("opens by itself while it is on and remembers the person's own choice on this device", async () => {
    draw(followController());
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    await userEvent.setup().click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    expect(window.localStorage.getItem(AUTO_PRESENTER_OPEN_KEY)).toBe("false");
  });

  it("says where it follows, with a mark as well as a colour", () => {
    draw(followController({ status: followStatus({ state: "following", section: { index: 3, name: "Chorus" }, slide: 2 }) }));
    expect(summary()).toHaveTextContent("Following: Chorus, slide 3");
    expect(within(summary()).getByText("\u2713")).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByTestId("presenter-follow-status")).toHaveAttribute("role", "status");
  });

  it("says that a click in ProPresenter paused it, with no button, and stays as folded as the person left it", () => {
    window.localStorage.setItem(AUTO_PRESENTER_OPEN_KEY, "false");
    draw(followController({ status: followStatus({ state: "held", hold_reason: "Paused: someone changed the slide in ProPresenter", message: "x" }) }));
    expect(summary()).toHaveTextContent("Paused: someone changed the slide in ProPresenter");
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
  });

  it("opens by itself when paused by failures, shows the whole sentence inside and Try again only then; the person can fold it", async () => {
    window.localStorage.setItem(AUTO_PRESENTER_OPEN_KEY, "false");
    const message = "Paused: ProPresenter did not take 5 slide changes in a row (ProPresenter did not answer). Following starts again with the next song, or press Try again.";
    const follow = followController({ status: followStatus({ state: "paused", can_try_again: true, message }) });
    draw(follow);
    expect(summary()).toHaveTextContent(/^!?Paused: ProPresenter did not take 5 slide changes in a row$/);
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("auto-presenter-detail")).toHaveTextContent("press Try again");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(follow.tryAgain).toHaveBeenCalledTimes(1);
    await user.click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
  });

  it("opens by itself for a song that cannot be followed", () => {
    window.localStorage.setItem(AUTO_PRESENTER_OPEN_KEY, "false");
    draw(followController({ status: followStatus({ state: "not_followed", message: "Arrangement was changed in ProPresenter" }) }));
    expect(summary()).toHaveTextContent("Not following: Arrangement was changed in ProPresenter");
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
  });

  it("is off while a save runs, and drawn locked with no controller", () => {
    const { unmount } = draw(followController({ pending: "switch" }));
    expect(screen.getByRole("switch", { name: FOLLOW_LABEL })).toBeDisabled();
    unmount();
    render(<AutoPresenterSection library={READY.library()} link={READY.link()} />);
    expect(screen.getByRole("switch", { name: FOLLOW_LABEL })).toBeDisabled();
    expect(band().querySelector("fieldset")).toBeDisabled();
  });
});

describe("Auto-Presenter locked", () => {
  it("says Needs the Playback Library Link, in the header and inside with where to set it up; the switch and the inside are locked", () => {
    locked(NO_LIBRARY);
    expect(summary()).toHaveTextContent("Needs the Playback Library Link");
    expect(screen.getByRole("switch", { name: FOLLOW_LABEL })).toBeDisabled();
    expect(screen.getByTestId("auto-presenter")).toHaveAttribute("data-locked", "true");
    expect(band().querySelector("fieldset")).toBeDisabled();
    expect(screen.getByTestId("presenter-follow-reason")).toHaveTextContent("Configuration \u2192 Playback (Library Link setup)");
    expect(within(band()).getByText("Needs the Playback Library Link", { selector: "[data-part=title], p, div, strong" })).toBeInTheDocument();
  });

  it("says Needs the Presenter Link when that one is missing, and current data when the Library Link only has an old copy", () => {
    const { unmount } = locked(NO_LINK);
    expect(summary()).toHaveTextContent("Needs the Presenter Link");
    expect(screen.getByTestId("presenter-follow-reason")).toHaveTextContent("Configuration \u2192 Presentation (Presenter Link setup)");
    unmount();
    locked({ library: libraryController("ready", { plan: plan(), status: libraryStatus("ready", { stale: true }) }) });
    expect(summary()).toHaveTextContent("Needs current data from the Playback Library Link");
  });

  it("names both links in ONE reason when both are missing", () => {
    locked({ ...NO_LINK, ...NO_LIBRARY });
    expect(summary()).toHaveTextContent("Needs the Presenter Link and the Playback Library Link");
    expect(screen.getAllByTestId("presenter-follow-reason")).toHaveLength(1);
  });

  it("cannot be switched on while locked, even by a click on the switch", async () => {
    const follow = off();
    locked(NO_LIBRARY, follow);
    await userEvent.setup().click(screen.getByRole("switch", { name: FOLLOW_LABEL }));
    fireEvent.click(screen.getByRole("switch", { name: FOLLOW_LABEL }));
    expect(follow.setEnabled).not.toHaveBeenCalled();
  });

  it("a switch saved as on and then locked says Waiting for ..., opens by itself, claims nothing, and can still be turned off", async () => {
    const follow = followController({ status: followStatus({ state: "following", section: { index: 3, name: "Chorus" }, slide: 2 }) });
    draw(follow, NO_LIBRARY);
    expect(summary()).toHaveTextContent("Waiting for the Playback Library Link");
    expect(document.body.textContent).not.toMatch(/Following/);
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
    const box = screen.getByRole("switch", { name: FOLLOW_LABEL });
    expect(box).toBeChecked();
    expect(box).toBeEnabled();
    await userEvent.setup().click(box);
    expect(follow.setEnabled).toHaveBeenCalledWith(false);
  });

  it("is usable when both links are ready", () => {
    draw(off());
    expect(screen.getByRole("switch", { name: FOLLOW_LABEL })).toBeEnabled();
    expect(screen.queryByTestId("presenter-follow-reason")).toBeNull();
    expect(band().querySelector("fieldset")).toBeEnabled();
  });
});

describe("the Timing group", () => {
  const timing = () => screen.getByRole("button", { name: /^Timing/ });
  const draw300 = (follow = followController({ offsetMs: 300 })) => { window.localStorage.setItem(AUTO_PRESENTER_OPEN_KEY, "true"); return draw(follow); };

  it("is folded, with the offset as its summary, and holds one small number box", async () => {
    draw300();
    expect(timing()).toHaveAttribute("aria-expanded", "false");
    expect(timing()).toHaveTextContent("Offset 300 ms");
    await userEvent.setup().click(timing());
    expect(timing()).toHaveAttribute("aria-expanded", "true");
    const box = screen.getByRole("spinbutton", { name: "Offset (ms)" });
    expect(box).toHaveValue(300);
    expect(box).toHaveAttribute("min", "-2000");
    expect(box).toHaveAttribute("max", "2000");
  });

  it("saves a whole number when the person leaves the box, and on Enter", async () => {
    const follow = followController();
    draw300(follow);
    const user = userEvent.setup();
    await user.click(timing());
    const box = screen.getByRole("spinbutton", { name: "Offset (ms)" });
    await user.clear(box);
    await user.type(box, "-250");
    fireEvent.blur(box);
    expect(follow.setOffset).toHaveBeenLastCalledWith(-250);
    await user.clear(box);
    await user.type(box, "400{Enter}");
    expect(follow.setOffset).toHaveBeenLastCalledWith(400);
  });

  it("refuses a number out of range, says so under the box and saves nothing", async () => {
    const follow = followController();
    draw300(follow);
    const user = userEvent.setup();
    await user.click(timing());
    const box = screen.getByRole("spinbutton", { name: "Offset (ms)" });
    await user.clear(box);
    await user.type(box, "5000");
    fireEvent.blur(box);
    expect(screen.getByRole("alert")).toHaveTextContent("Enter a whole number from -2000 to 2000.");
    expect(follow.setOffset).not.toHaveBeenCalled();
  });

  it("does not save an offset that did not change", async () => {
    const follow = followController({ offsetMs: 100 });
    draw300(follow);
    const user = userEvent.setup();
    await user.click(timing());
    const box = screen.getByRole("spinbutton", { name: "Offset (ms)" });
    await user.click(box);
    fireEvent.blur(box);
    expect(follow.setOffset).not.toHaveBeenCalled();
  });

  it("cannot be changed while the section is locked", () => {
    window.localStorage.setItem(AUTO_PRESENTER_OPEN_KEY, "true");
    draw(followController(), NO_LINK);
    expect(screen.getByRole("spinbutton", { name: "Offset (ms)" })).toBeDisabled();
  });
});
