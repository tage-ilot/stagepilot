# StagePilot frontend facelift: plan of action

Built from: impeccable (Operate mode, extract/audit/harden/adapt), design-md, web-design-guidelines (Vercel rules),
no-ai-design-slop + audit-ai-design-slop, frontend-evidence-audit, webapp-testing, plus a scan of beta 31
(`cfb439f`, 1.1.104-beta.31). Nothing here is started beyond this documentation PR. `DESIGN.md` stays the source of truth.

## What the scan found (counts, not guesses)
- 23 component files; 817 inline Tailwind colour classes; 26 distinct hex values; Tailwind 3.4.17 (matches the `json-tailwind` export).
- Borders today: white/5, /7, /10, /15, /20, /30 (six). Map to the three: 5,7 -> 6%; 10 -> 10%; 15,20,30 -> 18%.
- Focus: global `:focus-visible` rule exists in index.css and `color-scheme: dark` is set. Not a finding.
- Vercel-rule hits: 8 `transition-all`; 5 placeholders without "…"; only 3 `tabular-nums` (timer/clock numbers should all have it); no `touch-action: manipulation` / `overscroll-behavior` found.
- 10 controls use h-8/h-9 (36px). Fine on desktop; phones should get 44px (see decision 2 below).
- 39 `toHaveClass` assertions in tests: class-name changes will break tests, so each PR updates its own tests.
- No PRODUCT.md (impeccable wants one); DESIGN.md exists in the checkout root (untracked).

## What each skill recommends
| Skill | Recommendation for this project |
| --- | --- |
| impeccable (Operate mode) | Familiarity beats expression in a control surface. One family (Inter). Fixed rem scale, no fluid type. Standardise hover/focus/active/disabled/loading/error on every control. Motion 150-250ms, only for state. Skeleton or inline loading over spinners in content. Modals are a last resort. Extract components only when used 3+ times with the same intent. Order: document -> extract -> adapt -> harden -> audit -> polish. |
| design-md | Lint on every change; export `json-tailwind` into tailwind.config as the single token source; diff DESIGN.md between releases to catch regressions. |
| web-design-guidelines | Run on every changed file: explicit transition properties, "…" in placeholders and loading text, tabular-nums for numbers, touch-action, overscroll in dialogs, destructive actions need confirm or undo, URL/state, aria-live for async. |
| no-ai-design-slop / audit-ai-design-slop | Passive gate while building (system, hierarchy, removal test). Audit per phase: one primary action per view, no decorative stacking, no fake-live pulsing, no repeated icon cards, consistent radius/colour/icon family. Preserves our art direction instead of neutralising it. |
| frontend-evidence-audit + webapp-testing | Evidence over claims: real-browser screenshots at set widths, text-range clipping checks (not just scrollWidth), reduced-motion checked separately, honest note when touch was emulated. |
| design-taste-frontend | NOT used for the app. Its own scope excludes dashboards and it bans Inter and warm palettes that we deliberately chose. Only relevant if a marketing page is ever built. |
| claude-design / auteur | Prototype undecided questions as HTML first (as we did). auteur is for cinematic pages: not for this app. |
| humanizer / deslop | Pass over all button labels and messages after the vocabulary rename. |

## Phases (each a small PR; styling and behaviour never mixed)
0. **Sign-off.** Resolve the open items below; open the documentation-only PR (DESIGN.md + this plan). Optionally `impeccable init` to write PRODUCT.md (users, tone, the no-training principle).
1. **Baseline.** Before touching code: Playwright screenshots of every panel, dialog, gate and startup state at 1280, 1000, 390 and 320 wide, stored as the "before". Run impeccable `audit` and the Vercel review once to get the starting score.
2. **Plumbing, no visible change.** Bundle Inter; generate theme from DESIGN.md (`json-tailwind`) and CSS variables; alias the old `--sp-*` names to the new tokens. Screenshots must match baseline.
3. **Primitives** (each used 3+ times): Button, Field/Select, Panel, StatusDot/Badge, Disclosure (Advanced and Activity variants), SegmentedControl, SaveBar, Dialog. Built in a single components folder with all states.
4. **Surfaces, one PR each, lowest risk first:** loaders/startup (incl. removing the white line and the phone bar+spinner bug) -> header and status cards (wide/compact at 1000px) -> StagePilot panel -> Presentation -> Services -> Lights -> Playback (Connection method) -> live controls -> dialogs and sign-in gates.
5. **Behaviour changes, separate from styling,** each with its own tests and review: vocabulary renames; save-as-you-go with held changes and the always-visible Save configuration bar; Connection method select; "Other computer…"; MIDI network/channel held, rest of Lights immediate.
6. **Harden.** Long names, many songs, offline, API errors, double-click on Save, 200% zoom, reduced motion, existing keyboard support not regressed; touch tested with real devices if possible, otherwise labelled as emulated.
7. **Lock it in.** Delete duplicated styles; add a lint/check that blocks raw hex and off-palette Tailwind colours; CI step runs `design.md lint`; final audit scores vs baseline.

## Per-PR gate
`npm run typecheck && npm run lint && npm test` + `design.md lint` + web-design-guidelines on changed files + audit-ai-design-slop on the changed screen + before/after screenshots at the four widths. Skip redundant CI runs.

## Decisions (owner, 2026-10-09)
1. Held changes: as listed in DESIGN.md. Lights holds only the MIDI network (output) and channel; everything else in Lights saves immediately.
2. **Send test cue** runs when pressed and uses the saved output. It is disabled, with the reason shown, while a new output is selected but not saved.
3. Touch targets: 44px for every button on touch devices (`pointer: coarse`); 36px buttons remain for mouse pointers.
4. Phases 1-2 (baseline screenshots, plumbing with no visible change) may start before every guide detail is final.
5. Icons: the service icons are already generic pixel-art illustrations; keep them (they count as 8-bit moments).
6. PRODUCT.md added at the repo root.
7. Reviews of screens happen through LAN-viewable HTML/screenshots before merge.

## Plumbing notes (from testing the DESIGN.md exports, 2026-10-09)
The `json-tailwind`, `css-tailwind` and `dtcg` exports all run (exit 0). Things Phase 2 must handle:
- The export covers colours, font sizes, radii and spacing only. The `elevation`, `elevation-options` and `text-shadow` sections are custom and are **not exported**; map them by hand into `boxShadow` and a small `textShadow` utility.
- Bare `rounded` is not in the export. Add `DEFAULT: 8px` so the 12 bare uses become 8px by design.
- The token `neutral` shares its name with Tailwind's built-in `neutral-*` palette. The app uses none of those classes today, but rename it (for example `idle`) in the same PR to avoid confusion.
- Colour tokens named `text`, `border`, `panel` become classes like `text-text` and `border-border`. Prefer prefixed names (`ink`, `edge`, `surface`) when generating the theme.
- Replace `stage.*` colours and the old `panel` boxShadow in `tailwind.config.js` rather than keeping two systems.
