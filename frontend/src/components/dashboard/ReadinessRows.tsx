import { READINESS_ROW_MARK, READINESS_ROW_MARK_CLASS, READINESS_ROW_TEXT_CLASS, readinessRowTone, type ReadinessCheck } from "./dashboardReadiness";

/** The header's check list, one row per check: a mark that says it (`✓`, `×` for a row that blocks a service, `!` for an optional one that is not ready) and the words. The Dashboard and the gallery draw the same rows. */
export function ReadinessRows({ checks }: { checks: readonly ReadinessCheck[] }) {
  return (
    <ul className="grid gap-1.5">
      {checks.map((check) => (
        <li className="flex items-center gap-2 rounded-ds-lg bg-edge-subtle px-3 py-2 text-sm" key={check.id} title={check.detail}>
          <span className={`grid h-5 w-5 shrink-0 place-items-center rounded-full text-type-micro font-black ${READINESS_ROW_MARK_CLASS[readinessRowTone(check)]}`} data-row-tone={readinessRowTone(check)}>
            {READINESS_ROW_MARK[readinessRowTone(check)]}
          </span>
          <span className={READINESS_ROW_TEXT_CLASS[readinessRowTone(check)]}>{check.label}</span>
        </li>
      ))}
    </ul>
  );
}
