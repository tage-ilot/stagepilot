# Dev-only visual text and color editor

A development tool for the web frontend. Click an element in the running app, edit
its text or color in a side panel, and **Save** writes the change into
`frontend/src` so `git diff` shows exactly that edit. It never ships:
the Vite plugin is `apply: "serve"`, so `vite build` output contains no
`data-src`, overlay or endpoint (asserted by `dev-editor/productionBuild.test.ts`).

## Start it

Loopback only (default, `http://127.0.0.1:5173`):

    cd frontend && npm run dev

Reachable from the LAN (explicit flag, port 8101, all interfaces):

    cd frontend && npm run dev:lan      # http://<host-ip>:8101/

Anyone who can open the LAN URL can use the editor on that checkout, so run it
only on a trusted network, and on a throwaway branch. The endpoint also requires
a random per-run token that is embedded in the served page.

## Use it

1. Click the **Edit mode** pill (bottom right). Clicks and keys no longer reach the app.
2. Hover to outline, click to lock. The panel shows `file:line`, a breadcrumb
   (select ancestors), the text boxes and color swatches (theme tokens from
   `tailwind.config.js` plus a custom picker).
3. Edits preview live in the browser. **Save** and **Revert** enable once something changed.
4. **Save** rewrites the source (minimal diff, no reformatting). **Revert** undoes
   saves made on the current selection. **Commit** commits the files saved this session
   to the current branch; it refuses `main`/`master`/detached HEAD and never pushes.
5. "Change the theme token everywhere" edits the color in `DESIGN.md` and regenerates
   `tailwind.config.js` / `design-tokens.css` (`npm run tokens:generate`).

For components such as `Button`, the clicked DOM node has no `className` of its
own in the source; the panel offers the **component call site** (`<Button ...>`)
as the edit target, and a color change adds a class to that call site's `className`.

## What is editable

- Text: static JSX text and string-literal children. In mixed content
  (`Hello, {name} and welcome`) the static pieces are editable, `{name}` is locked.
- Colors: `text-*`, `bg-*`, `border-*` classes in a static `className` (also inside
  template strings, ternaries and `cx()` calls). Variants such as `hover:` are left alone.
  Arbitrary values (`bg-[#123456]`) work.

## Limits

- Dynamic text (`{expr}`), translations, line breaks and `{ } < >` are refused.
- A `className` that is built entirely at runtime cannot be edited.
- Only `.tsx`/`.jsx`/`.css` under `frontend/src` (plus `DESIGN.md` and generated token
  files for token edits). Symlinks and path traversal are rejected.
- Shared components and list rows: the panel warns "edits the template for N
  rendered instances".
- Not reachable in a plain browser: screens that need the Tauri shell (title bar,
  updater, native dialogs, backend supervisor), and screens behind a backend that is
  not running (the dashboard sits behind the access gate; without the backend on
  `:8099` only the gate screen renders). Start the backend per the README to reach them.
- Layout, spacing, font size and structure are deliberately not editable.
- No network access, telemetry or cloud service is involved.

## Tests

    cd frontend && npx vitest run dev-editor      # unit + production-build tests
    CHROME=/path/to/chrome REPO=$PWD/.. URL=http://localhost:8101/ node dev-editor/e2e/proof.mjs
    # needs `playwright-core`; asserts git diff, Revert and click-blocking
