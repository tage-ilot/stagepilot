import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { ProPresenterLinkController } from "../hooks/useProPresenterLink";
import { endLightkeyHelperInstall, endPlaybackHelperInstall, endProPresenterHelperInstall, startLightkeyHelperInstall, startPlaybackHelperInstall, startProPresenterHelperInstall } from "../api";
import type { HelperState, LightkeyStatus } from "../types";
import { helperIsOutdated, helperUpdateLine, updatesItself } from "./lightkeyProject";
import { PRESENTER_LINK_NAME } from "./proPresenterLinkState";
import type { HelperBoxState, HelperSetupBoxProps } from "./sections";

/**
 * What each helper says and how its status maps onto the ONE setup box (`sections/HelperSetupBox`): the wording, the three requests, and
 * when there is nothing to set up. A page spreads these into the box (`<HelperSetupBox {...lightkeyBoxProps(status)} autoStart={...} />`);
 * the behaviour (the command, the countdown, ending the session, the live region) is the box's and is the same for all three.
 */
type BoxProps = Omit<HelperSetupBoxProps, "autoStart">;

const PLAYBACK_STATE: Record<HelperState, HelperBoxState> = { ready: "ready", outdated: "outdated", needs_key: "needs_command", not_installed: "not_installed", unreachable: "not_answering" };

/** The Lighting computer's Lightkey helper, from the Lightkey status. Quiet while it works (and while the link is Off: nothing to set up yet). */
export function lightkeyBoxProps(status: LightkeyStatus): BoxProps {
  const connected = status.link === "connected";
  const outdated = helperIsOutdated(status);
  const current = connected && !outdated;
  const update = current ? helperUpdateLine(status.helper?.update) : null;
  // The helper's own sentence that an update was undone, or StagePilot's that it gave up: the setup command is how to update it then.
  const manual = current && status.helper?.update?.mode === "manual" ? status.helper.update.message : undefined;
  return {
    state: connected ? (outdated ? "outdated" : "ready") : "not_installed",
    answered: connected,
    quiet: current || status.link === "off",
    quietNote: (
      <>
        {update && <p className="mt-2 text-type-body-sm text-ink-muted" data-testid="lightkey-update-line" role="status">{update}</p>}
        {manual && <p className="mt-2 text-type-body-sm text-warning-text" data-testid="lightkey-update-note" role="status">{manual}</p>}
      </>
    ),
    endOnClose: true,
    tone: "lighting",
    controller: { start: () => startLightkeyHelperInstall(), end: () => endLightkeyHelperInstall() },
    wording: {
      testId: "lightkey-install",
      headings: { setup: "Set up the Lighting computer", update: "Update the Lighting Link", command: "Lighting Link command" },
      why: {
        not_installed: "Paste one command into Terminal on the Lighting computer.",
        outdated: "The Lighting Link on the Lighting computer is older than the one StagePilot carries.",
      },
      waitingFor: "the Lighting Link",
      done: "The Lighting computer is connected.",
      before: "On the Lighting Mac, turn on sACN output in Lightkey and open your project.",
      paste: "Open Terminal there and paste this command.",
      label: <span title="Capture script for the Lighting computer (beta, only for Lightkey)">Capture script:</span>,
      permissions: "macOS may ask to let Lightkey API use your local network and read your Documents and Downloads folders. Click Allow each time (the Lighting Mac must be unlocked).",
    },
  };
}

/** The Playback Library Link. Quiet while ready, before it has answered once, and while StagePilot updates it by itself. */
export function playbackBoxProps(library: PlaybackLibraryController, hasComputer: boolean): BoxProps {
  const helper = library.status?.helper;
  const state: HelperBoxState = helper ? PLAYBACK_STATE[helper.state] : null;
  const update = helper?.update;
  return {
    state,
    quiet: state === "ready" || state === null || (state === "outdated" && updatesItself(update)),
    // StagePilot tried and could not, or the helper undid the update: say so in one sentence, then the command.
    lead: state === "outdated" && update?.mode === "manual" ? update.message : undefined,
    hasComputer,
    needComputer: "Choose the Playback computer above, then set up its Library Link.",
    tone: "playback",
    controller: { start: () => startPlaybackHelperInstall(), end: () => endPlaybackHelperInstall(), watch: library.watch },
    wording: {
      testId: "playback-install",
      headings: { setup: "Set up the Playback computer", update: "Update the Library Link", command: "Library Link command" },
      why: {
        not_installed: "The Library Link is optional: it reads each song's key, BPM and arrangement from Playback.",
        outdated: "The Library Link on the Playback computer is older than the one StagePilot carries. It still works.",
        needs_command: "The Library Link on the Playback computer does not know this StagePilot yet.",
        not_answering: "StagePilot cannot reach the Library Link. Is the Playback computer on and awake?",
      },
      waitingFor: "the Library Link",
      done: "The Library Link is ready.",
      paste: "On the Playback computer, open Terminal and paste this command. It runs there and needs no internet.",
      label: "Library Link for the Playback computer:",
      permissions: "macOS may ask to let Playback Library read Playback's data. Click Allow each time (the Playback computer must be unlocked).",
    },
  };
}

/** The Presenter Link on the ProPresenter computer. Not drawn at all when ProPresenter runs on this computer (the block says so and offers nothing). */
export function proPresenterBoxProps(link: ProPresenterLinkController, hasComputer: boolean): BoxProps {
  const state: HelperBoxState = link.status?.state ?? null;
  const update = link.status?.update;
  return {
    state,
    quiet: state === "ready" || state === null || state === "checking" || state === "waiting" || (state === "outdated" && updatesItself(update)),
    lead: state === "outdated" && update?.mode === "manual" ? update.message : undefined,
    hasComputer,
    needComputer: <>Save the Presentation connection above with the ProPresenter computer&apos;s address, then set up its {PRESENTER_LINK_NAME}.</>,
    controller: { start: () => startProPresenterHelperInstall(), end: () => endProPresenterHelperInstall(), watch: link.watch },
    wording: {
      testId: "propresenter-install",
      headings: { setup: "Set up the ProPresenter computer", update: `Update the ${PRESENTER_LINK_NAME}`, command: `${PRESENTER_LINK_NAME} command` },
      why: {
        not_installed: `The ${PRESENTER_LINK_NAME} is optional: it adds Playback's songs to a ProPresenter playlist on another computer.`,
        outdated: `The ${PRESENTER_LINK_NAME} on the ProPresenter computer is older than the one StagePilot carries. It still works.`,
        needs_command: `The ${PRESENTER_LINK_NAME} on the ProPresenter computer does not know this StagePilot yet.`,
        not_answering: `StagePilot cannot reach the ${PRESENTER_LINK_NAME}. Is the ProPresenter computer on and awake?`,
      },
      waitingFor: `the ${PRESENTER_LINK_NAME}`,
      done: `The ${PRESENTER_LINK_NAME} is ready.`,
      paste: "On the ProPresenter computer, open Terminal and paste this command. It runs there and needs no internet.",
      label: `${PRESENTER_LINK_NAME} for the ProPresenter computer:`,
      permissions: <>To let StagePilot load a song that ProPresenter does not have yet, also switch on &quot;ProPresenter Link&quot; (the name macOS shows for the {PRESENTER_LINK_NAME}) in System Settings &gt; Privacy &amp; Security &gt; Accessibility.</>,
    },
  };
}
