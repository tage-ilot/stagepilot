import type { SettingsResponse } from "../types";
import { settingsNotices, useDismissedSettingsNotices } from "./settingsNotices";
import { Button } from "./ui/Button";
import { InlineAlert } from "./ui/InlineAlert";

/** The one vertical gap, in the unit of the place it stands in: the dashboard separates its sections by 20px (spacing xl), the Lighting panel its blocks by 24px (spacing 2xl). The same gap sits above the first banner, between banners, and below the last one. */
const SPACING = {
  main: { stack: "gap-5", outside: "mb-5" },
  lighting: { stack: "gap-6", outside: "my-6" },
} as const;

/** Heading colour per tone: the same hue the alert uses for its border, in its small-text shade (DESIGN.md section 3). */
const LEAD_TONE = { info: "text-info-text", warning: "text-warning-text" } as const;

/**
 * What StagePilot has to say about the settings file when it started: a song whose lighting data was damaged
 * and set aside (`settings_notice`, the song name is in the sentence, calm info tone), or a whole file that could
 * not be used (`settings_degraded`, everything went back to the defaults: warning tone). One banner each, in
 * plain words, with "Got it" to close it for this session. Nothing is drawn when there is nothing to say.
 * `where` keeps the main screen and the Lighting screen apart, so closing one does not hide the other; the
 * Lighting screen only ever shows the song notice. Each banner sits on an opaque dark base (`surface-950`), so
 * its words stay readable over the photo background; the stack owns its spacing (see SPACING).
 */
export function SettingsNoticeBanner({ settings, where, className = "" }: {
  settings: SettingsResponse | null | undefined;
  where: "main" | "lighting";
  className?: string;
}) {
  const { dismiss, isDismissed } = useDismissedSettingsNotices(where);
  const items = settingsNotices(settings, where === "lighting" ? "notice" : undefined).filter((item) => !isDismissed(item.key));
  if (items.length === 0) return null;
  const spacing = SPACING[where];
  return (
    <div className={`flex flex-col ${spacing.stack} ${spacing.outside} ${className}`} data-testid={`settings-notices-${where}`}>
      {items.map((item) => {
        const tone = item.kind === "degraded" ? "warning" : "info";
        return (
          <div className="rounded-ds-lg bg-surface-950" data-part="base" key={item.key}>
            <InlineAlert
              className="flex flex-wrap items-center gap-x-4 gap-y-2"
              data-kind={item.kind}
              data-testid={`settings-${item.kind}`}
              neutral
              tone={tone}
            >
              <div className="min-w-0 flex-[1_1_18rem]">
                <p className={`font-type-kicker text-type-kicker uppercase ${LEAD_TONE[tone]}`}>{item.lead}</p>
                <p className="mt-0.5 text-type-body-sm text-ink-body [overflow-wrap:anywhere]" data-testid="settings-notice-text">{item.text}</p>
              </div>
              <Button aria-label={`Got it: ${item.lead}`} onClick={() => dismiss(item.key)} size="touch" variant="secondary">Got it</Button>
            </InlineAlert>
          </div>
        );
      })}
    </div>
  );
}
