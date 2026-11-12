import { describe, expect, it } from "vitest";
// node builtin without types (node types are not in the app tsconfig); same pattern as narrowZoom.test.ts.
// @ts-expect-error node builtin without types
import { readdirSync, statSync } from "node:fs";

declare const process: { cwd: () => string };

// Every picture in src/assets is downloaded by every phone, tablet and computer that opens StagePilot, and it is packed into
// every installer. The dashboard icons once weighed 1.4 MB each (1024 px pictures shown at 50-70 px). Fonts are not checked here.
const LIMIT_KB = 300;
// The one exception: the page background is a 1700 x 2560 picture of film grain, shown at that size (see body in index.css). Grain does
// not compress, and shrinking it changes how the page looks. It was 1.5 MB; do not let it grow back.
const EXCEPTIONS_KB: Record<string, number> = { "film-flare.jpg": 900 };
const IMAGE = /\.(png|jpe?g|gif|webp|avif|svg|bmp|tiff?|ico)$/i;

describe("picture size guard", () => {
  it("no picture in src/assets is larger than 300 KB (the film-flare background has its own limit)", () => {
    const dir = `${process.cwd()}/src/assets`;
    const images = (readdirSync(dir) as string[]).filter((name) => IMAGE.test(name));
    expect(images.length).toBeGreaterThan(0);
    const tooBig = images
      .map((name) => ({ name, kb: Math.round((statSync(`${dir}/${name}`) as { size: number }).size / 1024), limit: EXCEPTIONS_KB[name] ?? LIMIT_KB }))
      .filter(({ kb, limit }) => kb > limit)
      .map(({ name, kb, limit }) => `${name} is ${kb} KB (limit ${limit} KB): resize it to about 3x the size it is shown at, and recompress it, before committing`);
    expect(tooBig).toEqual([]);
  });
});
