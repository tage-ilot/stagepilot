import { Button } from "./ui/Button";
import { Dialog } from "./ui/Dialog";

/** One Clean up glitches waiting for the person's answer: what was found, and what to do on "Remove flashes". */
export interface CleanGlitchesRequest {
  /** The song title. */
  subject: string;
  flashes: number;
  lights: number;
  onClean: () => void;
}

const plural = (count: number, one: string, many = `${one}es`) => `${count.toLocaleString("en-US")} ${count === 1 ? one : many}`;

/** The question before a saved song's rows are repaired. Nothing is lost for good (the toolbar's Undo brings the old song back), so the answer that changes the song is a plain button; the safe answer comes first and has the focus. */
export function CleanGlitchesDialog({ request, onClose }: { request: CleanGlitchesRequest | null; onClose: () => void }) {
  return (
    <Dialog
      actions={request && (
        <>
          <Button autoFocus onClick={onClose} size="touch" variant="secondary">Keep as is</Button>
          <Button onClick={() => { onClose(); request.onClean(); }} size="touch" variant="primary">Remove flashes</Button>
        </>
      )}
      description={request && (
        <>
          {`Found ${plural(request.flashes, "flash")} on ${request.lights.toLocaleString("en-US")} ${request.lights === 1 ? "light" : "lights"} in `}
          <strong className="text-ink">{request.subject}</strong>
          {". Remove them? You can Undo."}
        </>
      )}
      onClose={onClose}
      open={request !== null}
      title="Clean up glitches?"
    />
  );
}
