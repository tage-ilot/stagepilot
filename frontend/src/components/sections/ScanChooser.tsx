import { Button } from "../ui/Button";
import type { Tone } from "../ui/types";
import type { ScanState } from "./scanState";
import type { ScanWording } from "./ScanProgress";

/**
 * The chooser a scan shows when SEVERAL computers answered: one button per computer, the name and its address. Choosing one puts it in the
 * block's fields (the block's `onChoose`); nothing connects until the person presses Connect. One computer answering needs no chooser.
 * It stays in view (outside the Details), because the person has to answer it.
 */
export function ScanChooser({ scan, wording, tone, onChoose, disabled = false, testId }: {
  scan: ScanState;
  wording: ScanWording;
  tone?: Tone;
  onChoose: (host: string, port?: number) => void;
  disabled?: boolean;
  testId?: string;
}) {
  if (scan.state !== "found" || scan.candidates.length < 2) return null;
  const title = wording.choose(scan.candidates.length);
  return (
    <div aria-label={title} className="rounded-ds-lg bg-edge-subtle p-3" data-part="scan-chooser" data-testid={testId} role="group">
      <p className="text-sm font-semibold text-ink">{title}</p>
      <ul className="mt-2 grid gap-2">
        {scan.candidates.map((candidate) => (
          <li key={`${candidate.host}:${candidate.port ?? ""}`}>
            <Button className="w-full justify-start text-left" disabled={disabled} onClick={() => onChoose(candidate.host, candidate.port)} tone={tone} variant="tinted">
              {candidate.label}
              {candidate.hint && <span className="ml-1.5 text-xs text-ink-muted">{candidate.hint}</span>}
            </Button>
          </li>
        ))}
      </ul>
    </div>
  );
}
