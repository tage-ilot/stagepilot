import type { CSSProperties } from "react";

import type { LibrarySection } from "../types";
import { arrangementLabel } from "./playbackLibrary";
import { sectionColor } from "./sectionColors";

const clock = (seconds: number) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;

/**
 * The arranged song as one thin bar, left to right: a block per section (repeats are separate blocks), as wide as the time it plays,
 * tinted the way Playback tints it (`.library-seg` in index.css: 30 % of the section's color over the panel, a 3 px top border in the
 * full color). No words on it; a block's name and times are its hover text, and the bar as a whole is one image for a screen
 * reader. A song with no sections gets no bar.
 */
export function ArrangementBar({ sections }: { sections: LibrarySection[] }) {
  if (!sections.length) return null;
  return (
    <div aria-label={arrangementLabel(sections)} className="library-bar mt-[3px]" data-testid="library-bar" role="img">
      {sections.map((section, index) => (
        <span
          className="library-seg"
          // Repeats are separate blocks with the same name, so the place in the list is the only key.
          key={index}
          style={{ "--c": sectionColor(section.name), "--w": Number((section.end - section.start).toFixed(3)) } as CSSProperties}
          title={`${section.name} · ${clock(section.start)}–${clock(section.end)}`}
        />
      ))}
    </div>
  );
}
