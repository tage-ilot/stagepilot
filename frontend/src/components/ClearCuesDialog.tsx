import { Button } from "./ui/Button";
import { Dialog } from "./ui/Dialog";
import type { ClearCuesRequest } from "./lightingCueConfirmations";

export type { ClearCuesRequest } from "./lightingCueConfirmations";

const plural = (count: number, one: string, many = `${one}s`) => `${count} ${count === 1 ? one : many}`;

/**
 * THE confirmation for every Clear of lighting data (Clear song, Clear all, Clear loudness, and the song-chip clears).
 * Erasing is permanent, so the safe answer comes first and has the focus; Escape and a click on the backdrop keep the cues.
 * `request.onErase` is called exactly once, and only by "Erase permanently".
 */
export function ClearCuesDialog({ request, onClose }: { request: ClearCuesRequest | null; onClose: () => void }) {
  const loudnessOnly = request?.kind === "loudness";
  return (
    <Dialog
      actions={request && (
        <>
          <Button autoFocus onClick={onClose} size="touch" variant="secondary">Keep cues</Button>
          <Button onClick={() => { onClose(); request.onErase(); }} size="touch" variant="destructive">Erase permanently</Button>
        </>
      )}
      description={request && (
        <>
          {loudnessOnly ? "The loudness for " : "The cues for "}
          <strong className="text-ink">{request.subject}</strong>
          {" will be permanently erased. This cannot be undone."}
        </>
      )}
      onClose={onClose}
      open={request !== null}
      title={loudnessOnly ? "Erase this loudness permanently?" : "Erase these cues permanently?"}
      tone="danger"
    >
      {request && (
        <ul aria-label="What will be erased" className="list-disc space-y-1 pl-5 text-sm">
          {request.looks > 0 && <li data-testid="erase-looks">{plural(request.looks, "look")}</li>}
          {request.handmade > 0 && <li data-testid="erase-handmade">{plural(request.handmade, "hand-made cue")}</li>}
          {request.loudness > 0 && <li data-testid="erase-loudness">The recorded loudness for this song</li>}
          {request.looks + request.handmade + request.loudness === 0 && <li>Nothing is saved to erase.</li>}
        </ul>
      )}
    </Dialog>
  );
}
