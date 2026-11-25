import { apiOrigin } from "../api";
import logo from "../assets/versevision-logo.svg";
import { isDesktopShell, openExternalUrl } from "../desktop";
import { useDashboardAccess } from "../access/AccessContext";
import { SectionButton } from "./sections";

/** VerseVision is served by StagePilot itself, beside the dashboard (docs/versevision.md). */
export const VERSEVISION_PATH = "/versevision/";

/**
 * The big VerseVision button of the Presentation panel. In a browser it is a plain link that opens a new tab; in the desktop app the
 * page is not on the server's address, so the click hands the full address to the system browser (the same `openExternalUrl` the
 * other "open in the browser" buttons use). Not drawn for a remote session: the page is for this computer and the local network.
 */
export function VerseVisionButton() {
  const access = useDashboardAccess();
  if (access.mode === "remote") return null;
  return (
    <SectionButton
      description="Verse slides from your notes"
      href={`${apiOrigin}${VERSEVISION_PATH}`}
      logo={<img alt="" className="size-12" data-part="logo" height={48} src={logo} width={48} />}
      onOpen={isDesktopShell() ? (href) => void openExternalUrl(href) : undefined}
      testId="versevision-button"
      title="VerseVision"
    />
  );
}
