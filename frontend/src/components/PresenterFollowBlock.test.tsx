import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { followController, followStatus } from "../test/presenterFollowFixtures";
import { FOLLOW_LABEL, LOCKED_LIBRARY, LOCKED_LINK } from "../presenterFollow";
import { PresenterFollowBlock } from "./PresenterFollowBlock";

const draw = (follow = followController(), linkReady = true, libraryReady = true) => render(<PresenterFollowBlock follow={follow} libraryReady={libraryReady} linkReady={linkReady} />);
const status = () => screen.getByTestId("presenter-follow-status");

describe("Follow Playback in ProPresenter", () => {
  it("is a labelled switch that is off by default, with the one status line saying Off", () => {
    draw(followController({ enabled: false, status: followStatus({ enabled: false, state: "off" }) }));
    const box = screen.getByRole("switch", { name: FOLLOW_LABEL });
    expect(box).not.toBeChecked();
    expect(box).toBeEnabled();
    expect(status()).toHaveTextContent("Off");
    expect(status()).toHaveAttribute("data-state", "off");
  });

  it("turns following on and off through the controller", async () => {
    const follow = followController({ enabled: false });
    draw(follow);
    await userEvent.setup().click(screen.getByRole("switch", { name: FOLLOW_LABEL }));
    expect(follow.setEnabled).toHaveBeenCalledWith(true);
  });

  it("says where it follows: the section and the slide, with a mark as well as a colour", () => {
    draw(followController({ status: followStatus({ state: "following", section: { index: 3, name: "Chorus" }, slide: 2 }) }));
    expect(status()).toHaveTextContent("Following: Chorus, slide 3");
    expect(within(status()).getByText("\u2713")).toHaveAttribute("aria-hidden", "true");
    expect(status()).toHaveAttribute("role", "status");
  });

  it("offers Try again only while following is paused by failures", async () => {
    const follow = followController({ status: followStatus({ state: "paused", can_try_again: true, message: "Paused: ProPresenter did not take 5 slide changes in a row (x)." }) });
    const { unmount } = draw(follow);
    await userEvent.setup().click(screen.getByRole("button", { name: "Try again" }));
    expect(follow.tryAgain).toHaveBeenCalledTimes(1);
    unmount();
    // A click in ProPresenter holds the song by itself at the next section: no button.
    draw(followController({ status: followStatus({ state: "held", hold_reason: "Paused: someone changed the slide in ProPresenter", message: "x" }) }));
    expect(status()).toHaveTextContent("Paused: someone changed the slide in ProPresenter");
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
  });

  it("is greyed, not hidden, with its reason, while the Presenter Link is not ready", () => {
    draw(followController({ enabled: false }), false, true);
    expect(screen.getByRole("switch", { name: FOLLOW_LABEL })).toBeDisabled();
    expect(screen.getByTestId("presenter-follow")).toHaveAttribute("data-locked", "true");
    expect(screen.getByTestId("presenter-follow-reason")).toHaveTextContent(LOCKED_LINK);
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
  });

  it("is greyed with its own reason while Playback's Library Link is not ready", () => {
    draw(followController({ enabled: false }), true, false);
    expect(screen.getByRole("switch", { name: FOLLOW_LABEL })).toBeDisabled();
    expect(screen.getByTestId("presenter-follow-reason")).toHaveTextContent(LOCKED_LIBRARY);
  });

  it("is drawn locked when there is no controller at all", () => {
    render(<PresenterFollowBlock libraryReady linkReady />);
    expect(screen.getByRole("switch", { name: FOLLOW_LABEL })).toBeDisabled();
  });

  it("is off while a save runs", () => {
    draw(followController({ pending: "switch" }));
    expect(screen.getByRole("switch", { name: FOLLOW_LABEL })).toBeDisabled();
  });
});

describe("the Timing group", () => {
  const toggle = () => screen.getByRole("button", { name: /^Timing/ });

  it("is folded, with the offset as its summary, and holds one small number box", async () => {
    draw(followController({ offsetMs: 300 }));
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    expect(toggle()).toHaveTextContent("Offset 300 ms");
    await userEvent.setup().click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    const box = screen.getByRole("spinbutton", { name: "Offset (ms)" });
    expect(box).toHaveValue(300);
    expect(box).toHaveAttribute("min", "-2000");
    expect(box).toHaveAttribute("max", "2000");
  });

  it("saves a whole number when the person leaves the box, and on Enter", async () => {
    const follow = followController();
    draw(follow);
    const user = userEvent.setup();
    await user.click(toggle());
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
    draw(follow);
    const user = userEvent.setup();
    await user.click(toggle());
    const box = screen.getByRole("spinbutton", { name: "Offset (ms)" });
    await user.clear(box);
    await user.type(box, "5000");
    fireEvent.blur(box);
    expect(screen.getByRole("alert")).toHaveTextContent("Enter a whole number from -2000 to 2000.");
    expect(follow.setOffset).not.toHaveBeenCalled();
  });

  it("does not save an offset that did not change", async () => {
    const follow = followController({ offsetMs: 100 });
    draw(follow);
    const user = userEvent.setup();
    await user.click(toggle());
    const box = screen.getByRole("spinbutton", { name: "Offset (ms)" });
    await user.click(box);
    fireEvent.blur(box);
    expect(follow.setOffset).not.toHaveBeenCalled();
  });

  it("cannot be changed while the switch is locked", async () => {
    draw(followController(), false, true);
    await userEvent.setup().click(toggle());
    expect(screen.getByRole("spinbutton", { name: "Offset (ms)" })).toBeDisabled();
  });
});
