import type { SettingsResponse } from "../types";
import { settingsNotices, useDismissedSettingsNotices } from "./settingsNotices";
import { Button } from "./ui/Button";
import { InlineAlert } from "./ui/InlineAlert";

/**
 * What StagePilot has to say about the settings file when it started: a song whose lighting data was damaged
 * and set aside (`settings_notice`, the song name is in the sentence), or a whole file that could not be used
 * (`settings_degraded`). One calm banner each, in plain words, with "Got it" to close it for this session. Nothing
 * is drawn when there is nothing to say. `where` keeps the main screen and the Lighting screen apart, so closing
 * one does not hide the other; the Lighting screen only ever shows the song notice.
 */
export function SettingsNoticeBanner({ settings, where, className = "" }: {
  settings: SettingsResponse | null | undefined;
  where: "main" | "lighting";
  className?: string;
}) {
  const { dismiss, isDismissed } = useDismissedSettingsNotices(where);
  const items = settingsNotices(settings, where === "lighting" ? "notice" : undefined).filter((item) => !isDismissed(item.key));
  if (items.length === 0) return null;
  return (
    <div className={`space-y-2 ${className}`} data-testid={`settings-notices-${where}`}>
      {items.map((item) => (
        <InlineAlert
          className="flex flex-wrap items-center gap-x-4 gap-y-2"
          data-kind={item.kind}
          data-testid={`settings-${item.kind}`}
          key={item.key}
          neutral
          tone="info"
        >
          <div className="min-w-0 flex-[1_1_18rem]">
            <p className="font-type-kicker text-type-kicker uppercase text-info-text">{item.lead}</p>
            <p className="mt-0.5 text-type-body-sm text-ink-body [overflow-wrap:anywhere]" data-testid="settings-notice-text">{item.text}</p>
          </div>
          <Button aria-label={`Got it: ${item.lead}`} onClick={() => dismiss(item.key)} size="touch" variant="secondary">Got it</Button>
        </InlineAlert>
      ))}
    </div>
  );
}
