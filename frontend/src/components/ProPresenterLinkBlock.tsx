import { useId, useState } from "react";

import type { ProPresenterLinkController } from "../hooks/useProPresenterLink";
import type { ProPresenterStatusResponse } from "../types";
import { helperUpdateLine, updatesItself } from "./lightkeyProject";
import { DisclosureButton, LiveLinkPill, LiveLinkShell } from "./LightingOscParts";
import { ProPresenterInstallBox } from "./ProPresenterInstallBox";
import { importBlocked, linkLine, linkNote, linkPill, proPresenterLine, proPresenterPill } from "./proPresenterLinkState";

const TITLE = "ProPresenter Live Link";

/**
 * The ProPresenter Live Link block of the Presentation panel, built like the Playback Live Link block: a bordered card with a title, a
 * state pill beside it, and two status lines (ProPresenter itself, and the optional ProPresenter Link).
 *
 * The Link is how StagePilot reaches the song files and the screen of the ProPresenter computer; it adds Playback's songs to a
 * playlist (the "Add to ProPresenter" window, in the Playback panel). When ProPresenter runs on THIS computer there is nothing to set
 * up: the line says "on this computer", there is no setup button, and an import blocker is shown in plain words with its one-sentence
 * fix. Another computer gets the button **Set up the ProPresenter computer** (one command to paste into Terminal there).
 *
 * The Link is optional. Without it nothing here is a warning: the line says "Not installed" in grey, and nothing anywhere else asks for it.
 * The timer and Look settings above are not part of this block and are never changed by it.
 */
export function ProPresenterLinkBlock({ propresenter, link }: { propresenter: ProPresenterStatusResponse | null; link: ProPresenterLinkController }) {
  const setupId = useId();
  // The box under the strip, opened by its button (null = the default: open while the Link is not ready).
  const [setupOpen, setSetupOpen] = useState<boolean | null>(null);
  const status = link.status;
  const hasComputer = Boolean(propresenter?.enabled && propresenter.host);
  const local = status?.mode === "local";
  const helper = status?.mode === "helper";
  const api = proPresenterLine(propresenter);
  const line = linkLine(status, hasComputer);
  const pillApi = proPresenterPill(propresenter);
  const pillLink = linkPill(status, hasComputer);
  const state = status?.state ?? null;
  // A Link that is behind and can update itself is updated by StagePilot: its setup box stays closed, with only a line to read.
  const updatesBySelf = state === "outdated" && updatesItself(status?.update);
  const needsSetup = helper && state !== "ready" && state !== null && state !== "checking" && !updatesBySelf;
  const setupShown = helper && (setupOpen ?? needsSetup);
  const setupLabel = state === "outdated" && !updatesBySelf ? "Update the ProPresenter Link" : "Set up the ProPresenter computer";
  const updateLine = helperUpdateLine(status?.update);
  const note = linkNote(status);

  return (
    <LiveLinkShell
      className="!mt-5"
      pill={(
        <>
          <LiveLinkPill detail={api.text} name="" testId="propresenter-api-pill" text={`ProPresenter: ${pillApi.word}`} tone={pillApi.tone} />
          <LiveLinkPill detail={line.text} name="" testId="propresenter-link-pill" text={`ProPresenter Link: ${pillLink.word}`} tone={pillLink.tone} />
        </>
      )}
      testId="propresenter-live-link"
      title={TITLE}
    >
      <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-2 text-[0.78125rem] text-ink-muted" data-testid="propresenter-live-link-sub">
        <p aria-live="polite" className="min-w-0 flex-[1_1_16rem]" role="status">
          <span className="block sm:inline" data-testid="propresenter-banner" data-tone={api.tone}>ProPresenter: {api.text}.</span>
          <span aria-hidden="true" className="hidden sm:inline">{" \u00B7 "}</span>
          <span className="block sm:inline" data-testid="propresenter-link-status" data-tone={line.tone}>ProPresenter Link: {line.text}.</span>
        </p>
        {helper && (
          <div className="flex min-w-0 flex-wrap items-center gap-2" data-testid="propresenter-live-link-actions">
            <DisclosureButton controls={setupId} onToggle={() => setSetupOpen(!setupShown)} open={setupShown}>{setupLabel}</DisclosureButton>
          </div>
        )}
      </div>

      {note && <p className={`mt-2 text-type-body-sm ${importBlocked(status) ? "text-ink-body" : "text-warning-text"}`} data-testid="propresenter-link-note" role="status">{note}</p>}
      {updateLine && <p className="mt-2 text-type-body-sm text-ink-muted" data-testid="propresenter-link-update" role="status">{updateLine}</p>}
      {local && <p className="mt-2 text-type-body-sm text-ink-muted" data-testid="propresenter-link-local">ProPresenter runs on this computer, so there is nothing to set up. StagePilot uses its folder directly.</p>}
      {helper && (
        <div hidden={!setupShown} id={setupId}>
          <ProPresenterInstallBox autoStart={setupOpen === true} hasComputer={hasComputer} link={link} />
        </div>
      )}
      {link.error && <p className="mt-2 text-sm text-danger-text" role="alert">{link.error}</p>}
    </LiveLinkShell>
  );
}
