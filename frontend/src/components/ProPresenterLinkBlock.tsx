import { useId } from "react";

import { RecentAddresses } from "./RecentAddresses";

import type { ProPresenterLinkController } from "../hooks/useProPresenterLink";
import type { ProPresenterStatusResponse } from "../types";
import { helperUpdateLine, updatesItself } from "./lightkeyProject";
import { proPresenterBoxProps } from "./helperBoxProps";
import { ConnectionSection, HelperSetupBox, LiveLinkField, LiveLinkPill } from "./sections";
import { CONTROL_40 } from "./liveLink";
import { PRESENTER_LINK_NAME, apiWords, importBlocked, linkLine, linkNote, linkPill, linkWords, proPresenterLine, proPresenterPill } from "./proPresenterLinkState";
import { Button } from "./ui/Button";
import { Input } from "./ui/Input";

const TITLE = "ProPresenter Live Link";
const smallBox = "!w-[5.25rem] text-center tabular-nums";

/**
 * The connection the block holds, like the Lighting strip holds its Host and port: the panel owns the values and the save logic (a
 * held change is saved only by the panel's Save bar); the block only draws the boxes and the Test button.
 */
export type ProPresenterConnectionFields = {
  host: string;
  port: string;
  onHost: (host: string) => void;
  onPort: (port: string) => void;
  /** Edits that differ from the saved values: marked in words and with the warning border, never by colour alone. */
  held: { host: boolean; port: boolean };
  /** Plain-words problems with what was typed (shown after a Save was tried). */
  errors: { host?: string; port?: string };
  /** A request is in flight: the boxes and the button wait. */
  busy: boolean;
  /** Test connection is not offered (ProPresenter is not saved yet) or a request is in flight. */
  testDisabled: boolean;
  testing: boolean;
  onTest: () => void;
};

function HeldMarker({ id }: { id: string }) {
  return (
    <span className="inline-flex items-center gap-1.5 text-type-body-sm font-normal normal-case tracking-normal text-warning-text" data-held-marker id={id}>
      <span aria-hidden="true" className="h-2 w-2 rounded-ds-full bg-warning" />
      Not saved yet
    </span>
  );
}

/**
 * The ProPresenter Live Link block of the Presentation panel, built like the Playback and Lighting Live Link blocks: the FIRST block under
 * the header, a bordered card with a title, a state pill beside it, and two status lines (ProPresenter API, and the optional Presenter
 * Link). It holds the CONNECTION: the Host and Port boxes (with the recent addresses) and Test connection, under the two lines and above the
 * setup box. Timer, Look, Refresh and Save stay in the panel, after the block.
 *
 * The Presenter Link is how StagePilot reaches the song files and the screen of the ProPresenter computer; it adds Playback's songs to a
 * playlist (the "Add to ProPresenter" window, in the Playback panel). When ProPresenter runs on THIS computer there is nothing to set
 * up: the line says "on this computer", there is no setup button, and an import blocker is shown in plain words with its one-sentence
 * fix. Another computer gets the button **Set up the ProPresenter computer** (one command to paste into Terminal there).
 *
 * The Presenter Link is optional. Without it nothing here is a warning: the line says "Not installed" in grey, and nothing anywhere else asks for it.
 * The timer and Look settings are not part of this block and are never changed by it. Left out, `link` draws no Presenter Link line, pill or
 * setup; left out, `connection` draws no boxes.
 */
export function ProPresenterLinkBlock({ propresenter, link, connection }: { propresenter: ProPresenterStatusResponse | null; link?: ProPresenterLinkController; connection?: ProPresenterConnectionFields }) {
    const heldHostId = useId();
  const heldPortId = useId();
  const errorHostId = useId();
  const errorPortId = useId();
  const status = link?.status ?? null;
  const hasComputer = Boolean(propresenter?.enabled && propresenter.host);
  const local = Boolean(link) && status?.mode === "local";
  const helper = Boolean(link) && status?.mode === "helper";
  const api = proPresenterLine(propresenter);
  const line = linkLine(status, hasComputer);
  const pillApi = proPresenterPill(propresenter);
  const pillLink = linkPill(status, hasComputer);
  const state = status?.state ?? null;
  // A Link that is behind and can update itself is updated by StagePilot: its setup box stays closed, with only a line to read.
  const updatesBySelf = state === "outdated" && updatesItself(status?.update);
  const needsSetup = helper && state !== "ready" && state !== null && state !== "checking" && !updatesBySelf;
  // The Details button: "Presenter Link setup", or what is to be done when the Link is behind and StagePilot cannot update it.
  const detailsLabel = state === "outdated" && !updatesBySelf ? `Update the ${PRESENTER_LINK_NAME}` : `${PRESENTER_LINK_NAME} setup`;
  const updateLine = helperUpdateLine(status?.update);
  const note = link ? linkNote(status) : null;

  const fields = connection;
  const hostMarker = fields?.held.host ? <HeldMarker id={heldHostId} /> : undefined;
  const portMarker = fields?.held.port ? <HeldMarker id={heldPortId} /> : undefined;

  const hasDetails = Boolean(link) && (helper || local || Boolean(note) || Boolean(updateLine));

  return (
    <ConnectionSection
      attention={needsSetup || (Boolean(note) && !importBlocked(status))}
      className="!mt-0"
      connection={fields && (
        <>
          <div className="mt-2 flex flex-wrap items-end gap-x-2.5 gap-y-2" data-testid="propresenter-live-link-line">
            <LiveLinkField caption="Host" className="live-link-host" marker={hostMarker}>
              <Input
                aria-describedby={[hostMarker ? heldHostId : null, fields.errors.host ? errorHostId : null].filter(Boolean).join(" ") || undefined}
                aria-label="Host"
                autoCapitalize="none"
                autoCorrect="off"
                className={`w-full font-mono ${CONTROL_40} ${fields.held.host ? "!border-warning" : ""}`}
                disabled={fields.busy}
                error={Boolean(fields.errors.host)}
                onChange={(event) => fields.onHost(event.target.value)}
                spellCheck={false}
                value={fields.host}
              />
            </LiveLinkField>
            <LiveLinkField caption="Port" marker={portMarker}>
              <Input
                aria-describedby={[portMarker ? heldPortId : null, fields.errors.port ? errorPortId : null].filter(Boolean).join(" ") || undefined}
                aria-label="Port"
                className={`${smallBox} font-mono ${CONTROL_40} ${fields.held.port ? "!border-warning" : ""}`}
                disabled={fields.busy}
                error={Boolean(fields.errors.port)}
                inputMode="numeric"
                onChange={(event) => fields.onPort(event.target.value)}
                value={fields.port}
              />
            </LiveLinkField>
            <Button className={`flex-none ${CONTROL_40}`} disabled={fields.testDisabled} onClick={fields.onTest} tone="presentation" variant="tinted">
              {fields.testing ? "Testing…" : "Test connection"}
            </Button>
          </div>
          <RecentAddresses className="mt-2" disabled={fields.busy} onPick={fields.onHost} scope="presentation" value={fields.host} />
          {fields.errors.host && <p className="mt-1 text-type-body-sm text-danger-text" id={errorHostId} role="alert">{fields.errors.host}</p>}
          {fields.errors.port && <p className="mt-1 text-type-body-sm text-danger-text" id={errorPortId} role="alert">{fields.errors.port}</p>}
        </>
      )}
      details={hasDetails && link ? ({ byChoice }) => (
        <>
          {local && <p className="mt-2 text-type-body-sm text-ink-muted" data-testid="propresenter-link-local">ProPresenter runs on this computer, so there is nothing to set up. StagePilot uses its folder directly.</p>}
          {note && <p className={`mt-2 text-type-body-sm ${importBlocked(status) ? "text-ink-body" : "text-warning-text"}`} data-testid="propresenter-link-note" role="status">{note}</p>}
          {updateLine && <p className="mt-2 text-type-body-sm text-ink-muted" data-testid="propresenter-link-update" role="status">{updateLine}</p>}
          {helper && <HelperSetupBox {...proPresenterBoxProps(link, hasComputer)} autoStart={byChoice} />}
        </>
      ) : undefined}
      detailsLabel={detailsLabel}
      footer={link?.error ? <p className="mt-2 text-sm text-danger-text" role="alert">{link.error}</p> : undefined}
      pills={(
        <>
          <LiveLinkPill detail={api.text} name="" testId="propresenter-api-pill" text={apiWords(pillApi.word)} tone={pillApi.tone} />
          {link && <LiveLinkPill detail={line.text} name="" testId="propresenter-link-pill" text={linkWords(pillLink.word)} tone={pillLink.tone} />}
        </>
      )}
      status={[
        { text: apiWords(`${api.text}.`), testId: "propresenter-banner", tone: api.tone },
        ...(link ? [{ text: linkWords(`${line.text}.`), testId: "propresenter-link-status", tone: line.tone }] : []),
      ]}
      testId="propresenter-live-link"
      testIdPrefix="propresenter-live-link"
      title={TITLE}
    />
  );
}
