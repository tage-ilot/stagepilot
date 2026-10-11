# Design token plumbing

`DESIGN.md` is authoritative. Run `npm run tokens:generate` here after editing it;
`npm run lint` checks committed output without network access. `npm test` also
runs three Node regression tests (compatibility, self-hosted font, stale output).
The lockfile-pinned Google json-tailwind exporter supplies the theme; the generator
hand-maps the custom elevation/default, overlay, level-1 and label text shadow.

## Names and migration

Colors `text*` -> `ink*`, `border*` -> `edge*`, `panel*` -> `surface-panel*`.
Thus `text-ink`, `border-edge`, `bg-surface-panel` do not collide with CSS
recipes. Other colors retain token names. Radius/spacing names use `ds-`;
typography sizes/families use `type-`. Bare `rounded` remains stock 4px;
`rounded-ds-default` is 8px. Phase 3 promotes DEFAULT only when its surfaces
migrate. Stock radii/spacing/fonts/colors remain untouched. The exporter currently
omits numeric lineHeight values; generated typography is not used by components
in this phase, so surface migrations must explicitly apply the DESIGN.md leading.
CSS colors retain authored rgba precision rather than exporter's hex rounding.

`stage.950/900/850` alias background/scrollbar-track/card. Unused stage.800/700
remain compatibility-only until lock-in. `--sp-bg/text/accent/accent-hover/focus/
danger` alias design background/ink/primary/primary-hover/primary-focus/danger-soft.
Motion variables stay in index.css; they are unchanged. No component classes change.

## Compatibility mismatch ledger (baseline output wins in this PR)

| Old name | Preserved rendered value | Design target / handling |
| --- | --- | --- |
| --sp-surface | rgb(15 23 32 / .94) | panel #0f1720; preserve documented 94% alpha |
| --sp-surface-strong | rgb(5 10 16 / .9) | panel-strong #050a10; preserve documented 90% alpha |
| --sp-border | rgb(255 255 255 / .09) | border rgba(255,255,255,.10) |
| --sp-text-muted | #9aa9bb | text-muted #94a3b8 |
| --sp-text-quiet | #718096 | no direct token; later text ladder migration |
| --sp-accent-subtle | rgb(255 98 56 / .13) | primary #ff6238; retain existing wash opacity |
| --sp-success | #42dda4 | success #34d399 |
| --sp-warning | #f6c453 | warning #fbbf24 |
| --sp-info | #7dd3fc | info #38bdf8 |
| --sp-panel-shadow / expressive-shadow | 5px 6px 0 rgb(3 7 12 / .32), 0 18px 48px rgb(0 0 0 / .24) | elevation.default warm layered shadow |
| shadow-panel | 0 18px 50px rgba(0,0,0,.22) | elevation.default |
| stage.800 / stage.700 | #17212c / #223142 | deleted in design; unused compatibility retained |
| bare rounded | 4px | 8px deferred to surfaces |

These exceptions live in generator compatibility maps, not DESIGN.md. New
code must use normative tokens, not compat variables. Remove aliases at lock-in.

Inter Variable is bundled with font-display: swap and no CDN. Existing Tauri
font-src 'self' data: permits emitted local woff2 assets; no CSP change needed.
The actual family name is Inter Variable (the spec's Inter), applied only to the
existing root stack. Brand and monospace timers are unchanged. Font metrics/glyphs
are the sole intended visual exception; review separate font-on screenshots.
