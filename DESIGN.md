---
version: alpha
name: StagePilot
description: >-
  A dark, high-contrast live-production control surface. Calm slate panels,
  one deep red accent (#c7231d) for "live / primary", and unmistakable status colors,
  readable at arm's length in a dim booth. These tokens are the source of truth for the app:
  npm run tokens:generate turns them into src/design-tokens.css and tailwind.config.js.

colors:
  # --- Surfaces (darkest to lightest) ---
  background: "#090d12"        # page; sits over the film-flare image
  surface-950: "#020617"       # inputs, deepest wells (slate-950)
  surface-900: "#0f172a"       # raised wells (slate-900)
  panel: "#0f1720"             # .stage-panel fill (--sp-surface, drawn at 94% alpha)
  panel-strong: "#050a10"      # live / now-playing fill (--sp-surface-strong, 90% alpha)
  card: "#111923"              # status cards and Backend panel sub-cards (was `stage-850`, the only stage-* shade components used)
  scrollbar-track: "#0d131a"   # was `stage-900`, used only as a hardcoded hex in the scrollbar CSS
  # DELETED (decided): stage-800 #17212c and stage-700 #223142, used nowhere.
  # --- Text ---
  text: "#f8f4ec"              # warm off-white: headings, key figures, timers, input text. DECIDED: the old cool-white text-strong (slate-100 #f1f5f9) is gone; `text-strong` now means this colour, bolder
  text-body: "#cbd5e1"         # slate-300, default panel copy
  text-muted: "#94a3b8"        # slate-400: labels, kickers, hints, placeholders. DECIDED: replaces text-muted (slate-500 #64748b failed AA at 3.4:1). 7:1+ on every surface
  # --- Brand / primary ---
  primary: "#c7231d"           # DECIDED (owner, 2026-10-11): deep red replaces coral. LIVE state, primary emphasis. 3.2:1 on panels, so for fills, rails, dots and borders, never text
  primary-hover: "#cd3934"     # tint 10%. The lightest tint that keeps white button text at 4.5:1 or better (4.96:1)
  primary-focus: "#dd7b77"     # tint 40%. Focus ring and focus outlines (6.2:1 on panels)
  primary-active: "#b3201a"    # shade 10%. Pressed state of a solid primary button (white text 6.7:1)
  primary-text: "#e3918e"      # tint 50%. Primary used AS TEXT or a thin icon on dark (7.5:1 on panels)
  on-primary: "#ffffff"        # text on a solid primary fill (5.7:1). Dark text is only 3.5:1 on this red, so it is retired
  on-light: "#020617"          # DARK text on the LIGHT fills (the warning orange, the Services green, the Presentation orange): the old on-primary value, renamed because primary is now a dark-enough red that takes white text
  # --- Status (meaning never changes between screens) ---
  success: "#31551d"           # DECIDED (owner, 2026-10-13): a deep green replaces emerald for connected / running / ready. Only 2.1:1 on panels, so it is a FILL (Ready pill, solid badges, Start next / Restart current on press) with light text (7.8:1). Never a dot or words on a dark surface
  success-soft: "#417227"      # the same green lifted just enough for 3:1 (3.15:1 on panels): dots, borders, 10-30% tints
  success-text: "#69b63e"      # the same hue as small text: 7.2:1 on panels, 6.1:1 on its own 20% tint
  warning: "#ff0000"           # connecting / attention / delayed. Pure red, the SAME as `danger` (owner, 2026-10-13): errors and warnings are one family and cannot be confused with the primary red `#c7231d`, Lighting, or Presentation. 4.5:1 on panels
  warning-text: "#ff4d4d"      # warning as small text: same as `danger-text` (5.4:1 on panels)
  danger: "#ff0000"            # error accent, dots, icons, borders, glows. CHANGED (owner, 2026-10-13): pure red 255,0,0 (was rose-500). As small text on panels it is 4.5:1, so text uses danger-text
  danger-soft: "#ff3b3b"       # hover / lighter edge of the pure red
  danger-text: "#ff4d4d"       # error text on dark panels, 5.4:1 (pure red text would be 4.4:1 on cards)
  danger-solid: "#ff0000"      # FILLED buttons/badges ("Error action", Delete): the SAME pure red, owner 2026-10-13. White text on it is 4.0:1 (bold, with the label shadow); dark text would be 5.1:1 but the system keeps white on red fills
  danger-pill: "#f00000"       # DECIDED (owner, 2026-10-15): the darkest pure red that still gives the header pill's small words 4.5:1 on its dark surface (4.52:1). The pill's words use this
  danger-edge: "#bc0000"       # the darkest pure red that still gives the pill's outline the 3:1 non-text minimum on that surface (3.01:1)
  on-danger: "#ffffff"         # light text on the dark red fill
  on-danger-soft: "#3d0000"    # dark RED (not black) text on the light red fill: 5.0:1 on danger-soft
  info: "#1dc1c7"              # CHANGED (owner, 2026-10-12): the SECONDARY ACCENT replaces sky for information. Informational text, "Up next", the Reload service plan control, info banners. 8.2:1 on panels, so it works as text AND as a fill
  info-text: "#77dadd"         # tint 40%: the accent as small text on dark (11.0:1 on panels). The pure accent is for fills, dots, borders and large glyphs
  info-hover: "#34c7cd"        # tint 10%: hover fill
  info-active: "#1aaeb3"       # shade 10%: pressed fill
  on-info: "#020617"           # DARK text on an accent fill (9.1:1). White would be only 2.2:1
  idle: "#64748b"              # disconnected / idle              (slate-500)
  # --- Integration identity (each connected service owns one hue) ---
  integration-services: "#6bb03c"        # DECIDED (owner, 2026-10-12): green. UI label "Services" (Planning Center today)
  integration-services-soft: "#6bb03c"   # dots, borders, 10-30% tints (same as the base)
  integration-services-text: "#72b446"   # small text, 7:1 on panels
  integration-presentation: "#fe7821"    # DECIDED (owner, 2026-10-12): bright orange. UI label "Presentation" (ProPresenter today)
  integration-presentation-soft: "#fe7821"
  integration-presentation-text: "#fe802e"
  integration-lighting: "#d53367"          # DECIDED (owner, 2026-10-12): pink-red. UI label "Lighting". 3.9:1 on panels: fill/border/dot only. NEAR the error rose: see "Colour collisions"
  integration-lighting-soft: "#d53367"
  integration-lighting-text: "#e687a5"
  integration-playback: "#4b73b0"        # DECIDED (owner, 2026-10-13): the slate-blue made clearly BLUE (hue 216, saturation 40%; was #56677b, hue 212, saturation 18%). 3.8:1 on panels and cards, so it works as a fill, dot, border and tint with no separate lifted copy. White text on it 4.8:1. MIDI has no colour of its own (configured under Playback)
  integration-playback-soft: "#4b73b0"   # same as the base
  integration-playback-text: "#8ba5cd"   # small text: 7.2:1 on panels, 5.8:1 on its own 20% tint
  integration-playback-midi: "#7761b8"        # DECIDED (owner, 2026-10-14): the MIDI method inside the Playback panel wears a blue-violet halfway between the Playback blue and magenta (OKLCH hue -101 to -67, same lightness)
  integration-playback-midi-soft: "#7761b8"   # same as the base
  integration-playback-midi-text: "#a69ad2"
  integration-lighting-osc: "#2f9e8f"         # DECIDED (owner, 2026-10-15): the OSC method inside the Lighting panel wears a teal, clearly apart from the MIDI blue-violet (integration-playback-midi) and the Lighting pink-red
  integration-lighting-osc-soft: "#2f9e8f"    # same as the base
  integration-lighting-osc-text: "#7fcdc1"
  # Lighting timeline cue types (DECIDED direction, owner 2026-11-01): >= 3:1 on the track background; never colour alone (bar style + glyph + name)
  cue-look-captured: "#1fb0e0"   # Lighting timeline: captured look (cyan bar, ◆)
  cue-look-edited: "#f0a030"   # edited captured look (amber, ◆✎)
  cue-look-manual: "#7c8cff"   # manual look (indigo, ◇)
  cue-effect: "#4fd1a5"   # effect loop (mint, striped band, ↻)
  cue-live: "#a8e830"   # Lightkey cue on (solid) and off (hollow) (lime, ▶ / ■)
  cue-fixture: "#f08ae8"   # single fixture message (magenta, ◐)
  cue-other: "#94a3b8"   # other OSC (grey, dashed, ○)
  cue-note-c: "#ff5d5d"   # MIDI note letters C D E F G A B repeat through every octave; sharps keep the letter colour with a striped bar and a ♯
  cue-note-d: "#ff9d3c"
  cue-note-e: "#f2d33c"
  cue-note-f: "#5fd36a"
  cue-note-g: "#27c9c0"
  cue-note-a: "#5b8cff"
  cue-note-b: "#d0b0ff"
  integration-backend: "#c7231d"          # DECIDED (owner, 2026-10-12): the StagePilot status card and its configuration panel. SAME HEX as primary: see "Colour collisions"
  integration-backend-soft: "#c7231d"
  integration-backend-text: "#e28d89"
  # DECIDED direction: a muted version of each service colour for the "Advanced" parts of its setup
  # (30% of the hue's -text tint + 70% text-muted). Recomputed for the 2026-10-12 palette: 5.5:1 or better on a panel (all AA).
  advanced-services: "#8aa896"
  advanced-presentation: "#b4988f"
  advanced-playback: "#7e95b6"
  advanced-playback-midi: "#938cbb"       # the MIDI method inside the Playback panel (owner, 2026-10-14): same recipe, hue moved halfway to magenta
  advanced-lighting: "#ad9bb2"
  advanced-backend: "#ab9caa"
  # --- Widget borders (white at low opacity, so they work on any surface). DECIDED: three ---
  border-subtle: "rgba(255, 255, 255, 0.06)"  # ~34 uses: quiet dividers, card edges (merges the old 5% and 7%)
  border: "rgba(255, 255, 255, 0.10)"         # 73 uses, THE default
  border-medium: "rgba(255, 255, 255, 0.18)"  # ~21 uses: secondary button outline, hover (merges the old 15% and 20%; 20% was the least used)
  # --- Live controls (Manual controls widget): outlined, fill on hover. Colour = how much a press changes the show ---
  live-go: "#31551d"           # Start next, Restart current: moves the show forward. Fill on hover/press with light text (7.8:1)
  live-go-soft: "#417227"      # the outline at rest (3.15:1)
  live-go-text: "#69b63e"      # the label at rest (7.2:1)
  live-back: "#2d487c"         # Previous: steps the show back. DARK BLUE (owner, 2026-10-13), same lightness as Start next green (OKLCH L 0.41), hue 262; Reload is teal, Stop/Reset rose, so it stays distinct
  live-back-soft: "#3c61a6"      # the outline at rest
  live-back-text: "#689cfe"
  live-reload: "#1dc1c7"       # Reload service plan: re-reads the plan, nothing on stage changes. CHANGED to the secondary accent (was blue-700). Hover fill with DARK text (9.1:1)
  live-reload-text: "#77dadd"
  live-stop: "#e11d48"         # Stop timer, Reset position: halts or rewinds the show. CHANGED: hover fill rose-600 (was rose-500, white text only 3.7:1)
  live-stop-text: "#fecdd3"
  scrim: "rgba(0, 0, 0, 0.70)"                # modal backdrop (3 dialogs; error boundary uses slate-950 @80%)
  # --- Retro loading bar (deliberately off-brand: blue pixel art) ---
  loader-track: "#07101f"
  loader-outline: "#dbeafe"
  loader-fill: "#38bdf8"
  # --- Film-flare palette sampled from the background photo (owner, 2026-10-11) ---
  # ONLY for live-item washes, glows and warm shadow tints. Never for text, buttons or status.
  flare-red: "#c7231d"         # same as primary: the brightest red in the photo
  flare-ember: "#b63e32"
  flare-brick: "#8d2620"
  flare-wine: "#71201b"
  flare-umber: "#471e1b"
  flare-night: "#2b1817"
  flare-orange: "#db7037"
  flare-gold: "#dcaf50"

typography:
  # DECIDED: Geist (self-hosted variable font via @fontsource-variable/geist, no CDN, so it works offline in the desktop app).
  # Geist replaced Inter by owner decision on 2026-10-11 (tabular numbers, 100-900 weight axis, OFL-1.1).
  brand:
    fontFamily: StagePilot Logo
    fontSize: 4.05rem            # dashboard header lock-up (leading 0.56)
    fontWeight: 400
    lineHeight: 0.56
  brand-hero:
    fontFamily: StagePilot Logo
    fontSize: 11.25rem           # startup screen at lg; 6xl (3.75rem) base, 8xl (6rem) at sm
    fontWeight: 400
    lineHeight: 1
  display-timer:
    fontFamily: Geist Mono        # Tailwind font-mono (bundled; falls back to ui-monospace)
    fontSize: 5.4rem             # max. Real CSS is clamp(3rem, 5vw, 5.4rem); the spec cannot express clamp()
    fontWeight: 300
    lineHeight: 1
    letterSpacing: "-0.06em"      # timer only. (The -0.045em seen once is the song title, below. No conflict.)
  song-title:
    fontFamily: Geist
    fontSize: 3.65rem            # real CSS clamp(2rem, 3.2vw, 3.65rem); live panel current-song heading
    fontWeight: 900
    lineHeight: 0.96
    letterSpacing: "-0.045em"
  text-strong:                   # DECIDED: same colour as `text` (#f8f4ec), just bolder. Use for emphasis inside body copy and field labels.
    fontFamily: Geist
    fontSize: 0.875rem
    fontWeight: 700
    lineHeight: 1.45
  h1:
    fontFamily: Geist
    fontSize: 1.5rem
    fontWeight: 700
    lineHeight: 1.2
  h2:
    fontFamily: Geist
    fontSize: 1.125rem
    fontWeight: 700
    lineHeight: 1.3
  body-md:
    fontFamily: Geist
    fontSize: 0.875rem
    fontWeight: 400
    lineHeight: 1.45
  body-sm:
    fontFamily: Geist
    fontSize: 0.75rem
    fontWeight: 400
    lineHeight: 1.4
  label:
    fontFamily: Geist
    fontSize: 0.875rem
    fontWeight: 600
    lineHeight: 1.2
  kicker:
    fontFamily: Geist
    fontSize: 0.6875rem          # CHANGED: 11px. One micro size replaces 0.6 / 0.65 / 0.68rem
    fontWeight: 800
    lineHeight: 1.2
    letterSpacing: "0.18em"      # tracking scale is now 3 steps: wider 0.05em (uppercase labels), 0.18em (kickers), 0.35em (display)
  badge:
    fontFamily: Geist
    fontSize: 0.75rem
    fontWeight: 700
    lineHeight: 1.2
    letterSpacing: "0.05em"
  micro:
    fontFamily: Geist
    fontSize: 0.6875rem          # CHANGED: 11px, same size as kicker (was 0.6 / 0.65 rem)
    fontWeight: 700
    lineHeight: 1.2
    letterSpacing: "0.05em"

rounded:
  sm: 4px      # rarely used
  md: 6px
  lg: 8px      # THE default: buttons, inputs, inline alerts (157 uses). CHANGED: the 12 bare `rounded` (4px) become this
  xl: 12px     # ALL panels and cards (setup panels move here from 16px)
  "2xl": 16px  # toasts only
  full: 9999px # pills, status dots, badges

spacing:
  xs: 4px
  sm: 8px
  md: 12px     # default gap and horizontal padding
  lg: 16px     # panel padding
  xl: 20px
  "2xl": 24px
  touch-target: 44px   # DECIDED: live controls (timer, cue, manual controls, dialogs) and ALL inputs
  control: 36px        # DECIDED: every other button

components:
  panel:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.text-body}"
    rounded: "{rounded.xl}"
    padding: "{spacing.lg}"
  panel-setup:                                  # variant of panel (DECIDED: one panel recipe, two variants)
    backgroundColor: "{colors.panel}"
    textColor: "{colors.text-body}"
    rounded: "{rounded.xl}"
    padding: "{spacing.xl}"
  status-card:
    backgroundColor: "{colors.card}"
    textColor: "{colors.text-muted}"
    rounded: "{rounded.xl}"
    padding: "{spacing.md}"
  live-panel:
    backgroundColor: "{colors.panel-strong}"
    textColor: "{colors.text}"
    rounded: "{rounded.xl}"
    padding: "{spacing.lg}"
  button-primary:                               # DECIDED: the ONE solid action colour. Save, Connect, Unlock, Update.
    backgroundColor: "{colors.primary}"
    textColor: "{colors.on-primary}"
    typography: "{typography.label}"
    rounded: "{rounded.lg}"
    padding: "8px 14px"
    height: "{spacing.control}"
  button-live:                                  # the neutral base of a live control; its colour is one of the four live-* tones
    backgroundColor: "{colors.surface-950}"
    textColor: "{colors.text-body}"
    typography: "{typography.label}"
    rounded: "{rounded.lg}"
    padding: "10px 16px"
    height: "{spacing.touch-target}"
  button-primary-hover:
    backgroundColor: "{colors.primary-hover}"
  button-secondary:
    backgroundColor: "{colors.surface-950}"
    textColor: "{colors.text-body}"
    typography: "{typography.label}"
    rounded: "{rounded.lg}"
    padding: "8px 14px"
    height: "{spacing.control}"
  button-danger:
    backgroundColor: "{colors.danger-solid}"
    textColor: "{colors.on-danger}"
    typography: "{typography.label}"
    rounded: "{rounded.lg}"
    padding: "8px 14px"
    height: "{spacing.control}"
  button-danger-soft:
    backgroundColor: "{colors.danger-soft}"
    textColor: "{colors.on-danger-soft}"
    typography: "{typography.label}"
    rounded: "{rounded.lg}"
    padding: "8px 14px"
    height: "{spacing.control}"
  input:
    backgroundColor: "{colors.surface-950}"   # DECIDED: surface-950 everywhere (half use surface-900 today)
    textColor: "{colors.text}"
    typography: "{typography.body-md}"
    rounded: "{rounded.lg}"
    padding: "10px 12px"
    height: "{spacing.touch-target}"
  badge-success:
    backgroundColor: "{colors.success}"
    textColor: "{colors.text}"
    typography: "{typography.badge}"
    rounded: "{rounded.full}"
    padding: "6px 12px"
  badge-warning:
    backgroundColor: "{colors.warning}"
    textColor: "{colors.on-light}"
    typography: "{typography.badge}"
    rounded: "{rounded.full}"
    padding: "6px 12px"
  badge-danger:
    backgroundColor: "{colors.danger-solid}"
    textColor: "{colors.on-danger}"
    typography: "{typography.badge}"
    rounded: "{rounded.full}"
    padding: "6px 12px"
  dialog:
    backgroundColor: "{colors.surface-950}"
    textColor: "{colors.text-body}"
    rounded: "{rounded.xl}"
    padding: "{spacing.xl}"
  toast:
    backgroundColor: "{colors.surface-950}"
    textColor: "{colors.text-body}"
    rounded: "{rounded.2xl}"
    padding: "{spacing.xl}"
  popover:
    backgroundColor: "{colors.surface-950}"   # drawn at 95% alpha
    textColor: "{colors.text-body}"
    rounded: "{rounded.lg}"
    padding: "{spacing.md}"
primary-scale:                   # custom section, REFERENCE ONLY (owner supplied, 2026-10-11). Only the five named primary-* tokens above are used by components
  shade-10: "#b3201a"
  shade-20: "#9f1c17"
  shade-30: "#8b1914"
  shade-40: "#771511"
  shade-50: "#64120f"
  shade-60: "#500e0c"
  shade-70: "#3c0a09"
  shade-80: "#280706"
  shade-90: "#140303"
  tint-10: "#cd3934"
  tint-20: "#d24f4a"
  tint-30: "#d86561"
  tint-40: "#dd7b77"
  tint-50: "#e3918e"
  tint-60: "#e9a7a5"
  tint-70: "#eebdbb"
  tint-80: "#f4d3d2"
  tint-90: "#f9e9e8"
text-shadow:                     # custom section. DECIDED: LIGHT label text only gets a shadow. Dark label text gets none.
  on-dark-text: "0 1px 2px rgb(10 10 12 / 0.55)"     # light text (on dark or tinted buttons): small very-dark-grey shadow (owner: looks great)
elevation-options:               # custom section: PICK ONE strength for elevation.default / overlay (levels 2-5 scale the three soft layers)
  level-1: "inset 1px 1px 0 rgb(255 190 150 / 0.07), 2px 3px 6px rgb(33 15 13 / 0.35), 6px 10px 24px rgb(22 13 12 / 0.42), 12px 28px 64px rgb(22 13 12 / 0.45)"
  level-2: "inset 1px 1px 0 rgb(255 190 150 / 0.07), 2px 3px 6px rgb(33 15 13 / 0.44), 6px 10px 24px rgb(22 13 12 / 0.53), 12px 28px 64px rgb(22 13 12 / 0.56)"
  level-3: "inset 1px 1px 0 rgb(255 190 150 / 0.09), 2px 3px 6px rgb(33 15 13 / 0.52), 6px 10px 24px rgb(22 13 12 / 0.63), 12px 28px 64px rgb(22 13 12 / 0.68)"
  level-4: "inset 1px 1px 0 rgb(255 190 150 / 0.09), 2px 3px 6px rgb(33 15 13 / 0.61), 6px 10px 24px rgb(22 13 12 / 0.73), 12px 28px 64px rgb(22 13 12 / 0.79)"
  level-5: "inset 1px 1px 0 rgb(255 190 150 / 0.09), 2px 3px 6px rgb(33 15 13 / 0.73), 6px 10px 24px rgb(22 13 12 / 0.88), 12px 28px 64px rgb(22 13 12 / 0.92)"
elevation:                       # custom section (not in the Google spec; the linter ignores it)
  # Light comes from the upper-left, like the background photo. Shadows are warm brown-red, never cool grey/black.
  default: "inset 1px 1px 0 rgb(255 190 150 / 0.07), 2px 3px 6px rgb(33 15 13 / 0.35), 6px 10px 24px rgb(22 13 12 / 0.42), 12px 28px 64px rgb(22 13 12 / 0.45)"
  overlay: "0 8px 24px rgb(22 13 12 / 0.45), 0 24px 80px rgb(22 13 12 / 0.60)"   # DECIDED (owner approved, 2026-10-11): dialogs, toasts, popovers (replaces shadow-2xl).
---

# StagePilot design rules

This file is the single source of truth for how StagePilot looks and behaves. **The YAML block above is machine-read**: `npm run tokens:generate` (in `frontend/`) turns it into `src/design-tokens.css` and `tailwind.config.js`, `npm run lint` fails if those are stale, and CI runs `npx @google/design.md@0.4.0 lint DESIGN.md`. **This prose is for you (the agent or developer): it tells you what to do, how the existing screens were built, and how to add to them without breaking cohesion.** A human-facing visual version lives in the style guide (served on the LAN at `http://192.168.4.21:8099/`; source in `~/AgentHub-data/projects/stagepilot-design/styleguide/`).

**How to use this file:** read §1 and §2 always. Then read the section for what you are touching (§3 tokens, §4 type, §5 components, §6 panels, §7 dashboard, §8 header, §9 saving, §10 words). Run the checklist in §11 before you open a PR. Where a rule says **MUST**, a test or lint enforces it or a reviewer will reject it. Where it names a file, that file is the reference implementation: copy its pattern, do not invent a parallel one.

---

## 1. What this product is

StagePilot is a **live-production control surface**. A tech runs a church service from it, in a dim booth, glancing at it while their hands are busy. Everything follows from that:

- **Calm when fine, impossible to miss when not.** Dark slate panels; colour appears only when it carries meaning.
- **Usable with no training.** Controls sit where people expect them, labels say what will happen, and only the choices that make sense are offered. If a feature needs a manual, redesign it.
- **Nothing surprising during a show.** Anything that could change what a running show sends or reads is held until the user presses **Save configuration** (§9).
- **One look: "8-bit meets film flare, with a modern touch."**
  - *Modern* is the baseline: Geist, clean panels, 8/12px corners, tokens only.
  - *8-bit* is the app's **voice** for *system state*: the Geist Pixel Square font (status words, pills, connection labels, the Manual controls buttons, the service-plan title), the pixel loading bar, the phone spinner, the outlined logo. It means "StagePilot is telling you something about itself".
  - *Film flare* is **light and warmth**: the photo background, the warm brown-red shadow lit from the upper left, the red rail and wash on the live item. The red light means **live or selected** and appears on at most one item per view.
  - Both signatures are accents. Never use pixel fonts for body copy or form labels, pixelated borders on panels, neon outlines, gradients on ordinary controls, or extra film effects (grain, vignette, lens flare).

## 2. The ten laws

1. **Tokens only.** Every colour, radius, shadow, font size and spacing comes from the YAML above. No raw hex, `rgb()`, `hsl()`, stock Tailwind palette classes (`red-500`), `text-white`/`bg-black`, or `transition-all` in `frontend/src` (enforced by `src/tokensOnlyAll.test.ts` and `components/ui/tokensOnly.test.ts`; `index.css` may hold `rgb()` shadows only). Need a value that does not exist? Add the token to the YAML first, run `npm run tokens:generate`, then use it.
2. **Colour means one thing.** State colours = state. Connection colours = identity. The primary red = the one primary action and the live item. Teal = information. Never repeat a colour for a second meaning (§3).
3. **A state is never colour alone.** Pair every state colour with a word or icon (`Connected`, `Error`, `Not saved yet`).
4. **Build from `components/ui/`.** `Panel`, `Button`, `Badge`/`StatusBadge`, `Field`/`Input`/`Select`, `ConnectionMethod`, `Disclosure`, `SaveBar`, `ScrollRegion`, `SectionLabel`, `InlineAlert`, `Dialog`, `Toast`. Extend a primitive (add a variant or prop) rather than writing a second look.
5. **One panel, one recipe.** Every connection's configuration panel follows the same anatomy in the same order (§6). New app-wide options go in the **StagePilot** panel, never into a connection.
6. **Containers decide layout, not the window.** Grids inside panels and widgets switch with `@container` queries on the component's own width (§7). Only the page shell and status cards use viewport breakpoints.
7. **Hold, don't surprise.** Disruptive settings are held until **Save configuration**; harmless or reversible ones save the moment they change and say so (§9).
8. **One word per thing.** Use the vocabulary in §10. A brand name (Planning Center, ProPresenter) appears only where the user signs in to it or an API is specific to it.
9. **Respect reduced motion and touch.** Every animation has a `prefers-reduced-motion` fallback; nothing important is hover-only; targets are 44px (36px for ordinary buttons with a mouse).
10. **Keep the file true.** If you change what the UI does, update this file in the same PR (an out-of-date rule is worse than none). Do not mix a visual change and a behaviour change in one PR.

---

## 3. Colour

### 3.1 Surfaces and text
| Token (Tailwind class) | Use |
| --- | --- |
| `background` | page, under the photo (the photo is `fixed`, behind scrolling content; keep it) |
| `surface-panel` (`panel`) | widgets and configuration panels (drawn at 94% alpha) |
| `surface-panel-strong` (`panel-strong`) | the live Now playing widget |
| `card` | status cards, sub-cards |
| `surface-950` | inputs, dialogs, toasts, popovers, scroll regions, the header pill: **anything recessed reads as a dark well** |
| `ink` `#f8f4ec` | headings, key figures, timers, text people type |
| `ink-body` `#cbd5e1` | default copy |
| `ink-muted` `#94a3b8` | labels, kickers, hints, placeholders (7:1 on every surface) |

`text-strong` is **not** a colour: it is `ink` set at weight 700. Never use a slate-500 grey for text (it fails contrast).

### 3.2 The primary red (`primary` `#c7231d`)
Marks the **live item** (Now playing rail, current song row) and the **one solid primary button** (Save configuration, Connect, Scan, Update). At 3.2:1 on panels it is for **fills, rails, borders and dots only**; as text or a thin icon use `primary-text`. States: `primary-hover`, `primary-active`, focus ring `primary-focus`. Text on a primary fill is `on-primary` (white). A destructive button never uses `primary`; `primary` never carries an error.

### 3.3 State colours (fixed app-wide)
| State | Fill | Dots / borders | Words |
| --- | --- | --- | --- |
| connected / ready / running | `success` `#31551d` | `success-soft` `#417227` | `success-text` `#69b63e` |
| connecting / delayed / attention | `warning` `#ff0000` | same | `warning-text` |
| error / destructive | `danger-solid` `#ff0000` | `danger` | `danger-text` `#ff4d4d` |
| not connected / idle | | `idle` `#64748b` | near-white word "Not connected" |
| information, "what comes next" | `info` teal `#1dc1c7` (dark text `on-info`) | `info` | `info-text` |

- **Warnings and errors are the same pure red** and are told apart by their words (`Connecting`, `Delayed`, `Not saved yet` vs `Error`, `Couldn't save`).
- `success` is too dark for a dot: it is a **fill**. Status cards and the header pill carry state by *word and outline*, not a dot. Dots appear only where a word sits beside them (Check system list, badges).
- **Teal (`info`) is never a state and never the primary action.** Uses: "Up next", info banners, links, the Reload service plan control, the Update available button.
- **The header pill** uses the darkest reds that still pass contrast on its dark surface: words `danger-pill` `#f00000` (4.5:1), outline `danger-edge` `#bc0000` (3:1). Do not reuse these for anything else.
- **One word, one colour, one table (decided 2026-11-04).** A connection state has exactly one colour, taken from `statusTone` in `components/ui/statusWord.ts`, wherever it is drawn: **Connected** green, **Ready** teal (`info`: output open, delivery not proven, which is *not* an error and not a warning), **Connecting** and **Lightkey is closed** amber (the warning family), **Not connected** and **Error** red where something that was set up cannot be reached (a Live Link pill), **Not connected** and **Off** grey where nothing is expected yet (a card or header pill for a connection that is not set up). **"Not connected" is the one word for that state everywhere (decided 2026-11-18); "Disconnected" is not shown to operators** (it is the enum name in code, and the backend's own sentences are mapped to it by `playbackWords`). The panel header's pill and the Lighting Live Link pill both read this table, so teal never means a fault and red never means "fine". They may show different *words* only because they describe different things (the header: the OSC output, `READY`; the Live Link pill: the live-link API, `Not connected`), and the Live Link pill's tooltip names the subject. Never colour alone: every pill and dot has words.

### 3.4 Connection colours (identity)
Each connection owns one hue. It tints that connection's status-card icon, its panel, its secondary buttons and badges, and is the **only accent inside its own panel**.

| Connection | Token family | Hex | Label colour on a solid fill |
| --- | --- | --- | --- |
| Services | `integration-services` | `#6bb03c` green | dark (`on-light`) |
| Presentation | `integration-presentation` | `#fe7821` orange | dark (`on-light`) |
| Playback | `integration-playback` | `#4b73b0` blue | white |
| Playback → MIDI method | `integration-playback-midi` | `#7761b8` blue-violet | white |
| Lighting | `integration-lighting` | `#d53367` pink-red | white |
| Lighting → OSC method | `integration-lighting-osc` | `#2f9e8f` teal-green | white |
| StagePilot (card titled Backend) | `integration-backend` | `#c7231d` (= primary) | white |

Each family has three roles: the base (solid fills, rails), `-soft` (dots, borders, 10-30% tints) and `-text` (small text, 7:1). `advanced-<name>` is the muted tone (30% hue / 70% grey) for that connection's Advanced section.
- **Inside a connection's panel** (via `Panel tone=…` and `AccentContext`): primary buttons, tinted buttons, links, checkboxes, badges, success and info messages and the Save bar all take the connection's colour. **Exceptions that keep their own colour:** errors and warnings (pure red), idle grey, destructive buttons, and everything outside the panel (header, dialogs, toasts).
- **Never use a connection colour to mean state.** The open status card wears its connection colour (70% border, 10% wash fading by 44%, 3px left bar); the StagePilot card is the exception (it is primary red) and keeps a neutral card with a 1px red border.
- A *method* inside a connection gets its own sub-hue in the `ConnectionMethod` control (`chip` prop). **MIDI is the one method that also recolours its panel's controls:** when the MIDI method is chosen under Playback (the `MidiSetupPanel`) or under Lighting (the `LightingSetupPanel`), the panel passes `accent="midi"` while `tone` stays the connection's own identity, so buttons, badges, switches, status dots, banners, the Save bar and feature cards wear the MIDI blue-violet. The title (`titleTone`), the 1px top border and the setup wash stay the connection's colour. Playback API and OSC keep their connection's own colour everywhere. Pad colours a user picks on the Lighting cue board are their own and never change with the method.

### 3.5 Colours that sit close together (keep apart by form, not hue)
- **StagePilot `#c7231d` = primary red.** A rail or wash means *live or selected* and appears once per view; a connection tint is a thin tile tint and 1px border, never a rail or wash.
- **Lighting `#d53367` is near the error red.** Lighting is tint and dot only. Errors are always words plus a red dot and icon; a Lighting card in error says `Error`, it does not change colour.
- **Services green and the success green share a hue.** Inside a Services panel a "connected" message uses the panel accent, never the success green.
- **Live controls (Manual controls):** `live-go` green (Start next, Restart current), `live-back` dark blue (Previous), `live-reload` teal (Reload service plan), `live-stop` rose `#e11d48` / `#fecdd3` (Stop timer, Reset position). They are outlined at rest and fill on hover/press: a **severity scale** (how much a press changes the show), not "actions", so they do not follow the primary-only rule. The Stop/Reset rose is **deliberately a different red** from the pure-red error colour and the header pill (owner chose rose over the pill's dark red on 2026-10-16: do not change it).

### 3.6 Contrast and text on fills
- **Minimum 4.5:1 for text, 3:1 for outlines/dots/large bold text.** `design.md lint` checks token pairs.
- **Light text on dark fills, dark text on light fills.** Dark label (`on-light`/`on-info`) on the Services green, Presentation orange and teal accent; white (`on-primary`/`on-danger`) on everything else. Where an error colour needs dark text on a light red fill use `on-danger-soft` (dark *red*), never black.
- **Label shadow:** light text on a dark/tinted button gets `text-shadow.on-dark-text` (`text-shadow-label`). **Dark text gets no shadow.**
- Photo-sampled `flare-*` colours are for washes and glows on the live item only, never text, buttons or status.

---

## 4. Typography

**Fonts (all bundled; never a CDN, so the desktop app works offline):**
| Voice | Font | Use |
| --- | --- | --- |
| Interface | **Geist** (variable, `font-sans`) | everything by default |
| Numbers | **Geist Mono** (`font-mono`) | timers, elapsed time, countdowns, any live-changing figures; always `tabular-nums` |
| System state | **Geist Pixel Square** (`.font-pixel`) | **status words** on status cards and the header pill, **info/error notification text**, **connection pills/badges**, **Manual controls buttons**, the **service-plan title**, the `N songs` count |
| Wordmark | **StagePilot Logo** (`.font-brand`) | the logo only, with a 1px black outline |

- **Geist Pixel Square has one weight.** "Bold" is a 0.05em `-webkit-text-stroke` in the text colour built into `.font-pixel` (the status-card word uses 0.07em because it is larger). **Always use the class**, never `font-bold` on pixel text, and never fake bold. Letters like O, D, e, a must keep open counters.
- **Scale (use the tokens):** `type-brand` 4.05rem; `type-display-timer` clamp(3rem, 5vw, 5.4rem) +8px on the Now playing countdown, weight 300, tracking -0.06em; `type-song-title` Geist 900; `type-h1` 24; `type-h2` 18; panel titles 20px bold; `type-text-strong`/`type-label` 14; `type-body-md` 14; `type-body-sm` 12; `type-kicker`/`type-micro` **11px, the floor**. The only text allowed below 11px is the status word on compact status cards (≤1000px).
- **Kickers** (`type-kicker`: 11px, 800, uppercase, 0.18em, `ink-muted`) name sections: `Connection configuration`, `Status`, `Advanced`, `Connection method`. Use `SectionLabel` (it renders a real heading).
- **Tracking has three steps:** 0.05em (uppercase labels), 0.18em (kickers), 0.35em (display).
- **Numbers and units:** `tabular-nums` on every timer/clock/count; non-breaking space between a number and its unit; single `…` character in placeholders and loading text.
- **Text colour matches state tint where it is a live readout:** the Now playing label and elapsed time take the rail's colour (`.now-playing-tint`).

---

## 5. Shape, depth, spacing, motion

- **Radius:** 8px (`rounded-ds-lg`) controls, inputs, inline alerts, popovers; **12px** (`rounded-ds-xl`) **every** panel, card and dialog; 16px toasts only; full for pills, dots, badges.
- **Borders (three, white at low opacity):** `edge-subtle` 6% (dividers, card edges), `edge` 10% (**default**), `edge-medium` 18% (secondary button outline, hover). Hover on cards raises border opacity; it never changes size.
- **Elevation:** one warm shadow, `elevation.default` (brown-red, lit from the upper left, with an inner top-left highlight); `elevation.overlay` for dialogs, toasts, popovers. Never cool grey or pure black shadows.
- **Spacing:** 4px base unit; `md` 12px default gap; `lg` 16px panel padding (setup panels 20px). Targets: **44px** (`touch-target`) for live controls, all inputs and every button on touch (`pointer: coarse`); **36px** (`control`) for other buttons with a mouse.
- **Page:** capped at 1700px, centred, supports 320px.
- **Z-index layers:** widget handles 10 · menus 20 · header chip 40 (the system-check list sits above widgets) · popovers and confirm dialogs 50 · title bar and update dialog 100 · fatal toasts 110 · error boundary 120. Use these; do not add layers.
- **Motion:** micro-interactions 140ms; status colour 180ms; status-card morph 275ms; expand/collapse 500ms; header-pill resize and loading-logo flight as in §8; easing `cubic-bezier(.2,.8,.2,1)`. Animate `transform`/`opacity` only; list transitioned properties (never `transition: all`); respect `prefers-reduced-motion` with a calm alternative that still shows the change; animation must never delay or block a control. Do not use a pulsing dot on anything that is not live.

---

## 6. Components (the `components/ui/` kit)

### 6.1 Buttons (`Button`)
| Variant | Look | Use |
| --- | --- | --- |
| `primary` | solid `primary` red (or the connection's colour inside a panel), white text (dark on Services/Presentation) | the **one** main action: Save configuration, Connect, Scan, Update |
| `tinted` | `hue @ 10%` fill, 20-30% border, light hue text | secondary actions in a panel; takes the connection hue |
| `secondary` | `surface-950` fill, `edge-medium` outline | neutral: Discard, Close, Copy log |
| `destructive` / `destructive-soft` | `danger-solid` red | irreversible actions only; label names what is lost (`Delete layout`) |
| `live` + `liveTone` | outlined, fill on hover/press | Manual controls only (§3.5) |
- Labels are **verb + object** (`Save configuration`, `Search`, `Connect`, `Scan Playback API`, `Test connection`), sentence case, never `OK`/`Submit`/`Yes`. The safe option sits on the left of a pair.
- Disabled = 40% opacity + `cursor-not-allowed` **and a reason shown nearby** (`No changes to save`). Never silently grey out.
- Only one primary button per panel. Focus ring: 2px `primary-focus`, 3px offset, global; never remove.

### 6.2 Inputs and choices
- `Field` = label **above** (never left), optional hint in `ink-muted`, error under in `danger-text` with a short fix ("Enter a number from 1 to 16"). `Input`/`Select`: `surface-950`, `edge`, 8px, **44px** tall, `ink-muted` placeholder. Validate on blur; keep the user's input on error.
- **Choose one value from a list → styled native `Select`.** **Choose an action → action menu** (button + chevron opening a `surface-950` card, 44px items, arrow keys + Escape). **Choose how a connection connects, or one of a few exclusive modes → `ConnectionMethod`** (below). A computer's address is never a select with an "Other computer…" option: it is the `AddressBox` (§6.6), a text box that is always editable with an arrow that lists the scanned and recent addresses.
- Short fixed-width controls stay short: a PIN input is `w-28` with **Save PIN** beside it on the same row.

### 6.3 `ConnectionMethod`: THE switch
A segmented radio group: two or more options side by side, each a bold name, optionally over a tiny uppercase `PRIMARY`/`ALTERNATE` (`role` prop; omit it for unranked options). The chosen option wears the connection's tint (outline + 45% fill) and **the blue box slides between options**. Native radios (arrow keys, 44px). Used for **Playback** (Playback API primary / MIDI alternate), **Lighting** (OSC primary / MIDI alternate) and **Backend Connection Access** (Host / Local network / Internet). `label` overrides the kicker (default `Connection method`). **Never build a second picker; add options to this one.** Changing method swaps the fields beneath it; fields belonging to the other method are hidden and never affect each other (OSC cues work only when OSC is saved, MIDI only when MIDI is).

`compact` is the one-line toolbar form (the kicker is read by screen readers only). `field` is the compact form for a row of labelled fields (Lighting's **Mode**): the kicker is visible in the small uppercase muted field-label style and the switch is 40 px (44 on touch). `description` is a one-line explanation carried by the switch's tooltip and accessible description instead of a standing paragraph.

### 6.4 `Disclosure`: three kinds of expandable thing (shape says what it is)
| Variant | Look | Means |
| --- | --- | --- |
| `advanced` | one tinted bordered row, chevron + label, **one-line summary of what is inside when closed** (`Port 18766 · Logging info`), 3px left bar in the connection's `advanced-*` tone when open, 44px | extra options. Always **after** General configuration. Opens by itself when a field inside is invalid |
| `activity` | flat, no box/fill, dashed top rule, neutral count pill (`12 events`) | read-only information, or a group of controls people may not use (Remote access). Pass `usePersistedFlag` so the open state survives restarts |
| `feature` | card with a **5px rail, 70% border and soft wash in the connection colour**, `text-lg` title, a **detail line** shown open or closed that says what is inside (`16 cues on 2 of 4 songs · 2 pads · Battle Belongs 04:41`), an `aside` tag at the right | a **major** optional feature that deserves weight (Lighting's Song lighting timeline). Everything in the feature, including its Test cue controls, folds away with it |
Expand/collapse 500ms; respect reduced motion. **Persist** collapse state with `usePersistedFlag(KEY, default)` whenever a section is optional for some users (Remote access, Song lighting timeline: both start open).

### Config panel reveal

A configuration panel emerges from the connection card it belongs to; closing is the exact reverse and the panel unmounts only afterwards (`usePanelPresence` + `PanelReveal`, `lib/panelEmerge.ts`, `.panel-reveal*` in index.css). **Only `transform` and `opacity` animate** (Web Animations, one group, so the dashboard is not laid out again on every frame, which is what made the old height/clip-path animation stutter on slow Macs). The panel's open layout (final size and position) applies immediately; nothing animates height, grid rows, margins, `top`/`left`, `clip-path` or filters.

- **Duration 500ms** open and close (`EMERGE_MS`). **Open:** outer wrapper `translateX(dx) scaleX(sx)` -> identity, `cubic-bezier(.65,0,.35,1)`, where `sx` = card width / panel width (floor 0.06) and `dx` = card centre - panel centre; inner wrapper `translateY(-gap) scaleY(0.02)` -> identity, `cubic-bezier(.4,0,.2,1)`; the whole group (outer wrapper) also fades in over the first 20% (opacity, linear), so an open that is turned round mid-flight also ends invisible. Both scale from `50% 0`, so the strip starts exactly the card's width, centred on the card, touching its bottom edge. `gap` is the real distance from the card's bottom edge to the panel's top, read from layout (never hard-coded).
- **FLIP for what is below**: everything after the reveal in the flow slides `translateY(-(0.98 H + gap))` -> 0 with the inner wrapper's duration and easing (H = the panel's measured height), so the gap under the panel stays constant. If the panel's height changes while it animates (data arriving) the offset is refitted in place. 
- **Close is not the open reversed (no lingering box):** it **accelerates into the card** (`CLOSE_EASING` = `cubic-bezier(.5,0,.75,.4)` for the horizontal narrowing, the vertical collapse and the content below, one easing for all three so the gap stays constant) and ends at **`scaleY(0)`** at the card's width on its bottom edge, with the content below at `translateY(-(H + gap))`, i.e. exactly its closed-layout position (the old end, `scaleY(0.02)` with an ease-out tail, left a 12-30px strip that sat under the card for the last ~20% and then popped away, and moved the content below by that 2% at unmount). The **whole panel group fades out** (outer wrapper opacity, linear, 1 until 65%, 0 at 93%), so what is still tall at the end is already invisible: measured on a real close, at 92% every panel (first, middle, last card, tall Lighting) is at 4% opacity or less, at 100% nothing is left, and no frame of a real close at 60 Hz with 4x CPU throttle shows a 1-30px strip at more than 10% opacity. The panel **unmounts the moment the close animation finishes** (`PanelReveal` `onCloseEnd` -> `usePanelPresence().closeEnded`), the presence timer only remains as a fallback (it can fire a frame earlier; the last frame is invisible either way). The content fade-out and the notch's sink (done at 35%) are unchanged and finish before the group starts to fade.
- **Content** fades in only over the last 55% of an open (0 until offset .45, then linear to 1) and out over the first 40% of a close, so scaled text is never visible. If a child of the panel is replaced while the panel opens (the Lighting panel swaps a section once its settings arrive) the new child takes over the fade at the same time, and the old one's animation is cancelled (a fade on a removed element keeps the page rendering every frame without painting anything).
- **Notch pointer**: an upright 45 degree triangle (28px wide, 14px tall: the 12px above the panel's top border plus the border row and the highlight row under it) sits on the panel's top border centred on the card's centre, so it reads as part of the border. Its 1px edges are the panel's own top-border colour (the connection colour) and it is filled with a copy of the panel's background (colour and wash, read from the panel's computed style, set from layout, continued 12px above it), so it is the panel's exact colour: **Colour match, measured:** the panel itself is drawn with the opaque panel colour (`.panel-reveal__panel > [data-variant] { background-color: var(--ds-surface-panel) }`; the normal fill is 94% translucent). A translucent notch can never equal a translucent panel: both show the page behind them, and the notch sits over a different patch of it (the photo backdrop is brighter and redder toward the top and left, and the cards' and widgets' drop shadows darken the strip under the cards, which a copy of the backdrop cannot reproduce: a replica of the page background under the notch was tried and did not help, 3.3/255 vs 3.8/255 before). With an opaque panel the notch's copy of the panel background is the panel's own pixels wherever it is. `frontend/scripts/panel-notch-pixels.mjs` (not run in CI) prints the notch-vs-panel difference for all five panels after a first open, a swap, a close and re-open, a window resize and the last frame of an open: at most 0.8/255 per channel, sampled at the notch centre and four positions across the triangle, 4 and 9px above the border (10px is the 1px apex of the 12px triangle). The diagonal legs end on the border row's bottom edge, so they do not overshoot the border at the base corners. It is a separate element tree (`.panel-reveal__notch-*`, `aria-hidden`, no pointer events, never scaled) and it **rises out of the panel's edge**: on an open it travels `translateY(+14px)` -> 0 (`cubic-bezier(.2,.8,.2,1)`) from offset .6 (a little later for a card near the panel's end, once the still narrow strip covers the notch, never after .8) so it is finished when the panel is, and on a close it sinks back the same way (`cubic-bezier(.8,0,.8,.2)`, finished by offset .35, before the panel shrinks away). It never fades. A static clip (`clip-path: inset(0)` on a strip from the notch's tip to just under the border row, riding the panel's top edge with the inner wrapper's own `translateY`) shows only the part above the border, so while it is still "inside" the panel it is not drawn at all, and never over the panel's content or the gap. Only `transform` animates (plus the accent colour during a slide); the fill is counter-translated with the same keyframes and easings so it stays lined up with the panel while the notch moves. It is decorative: the selected card's styling stays the cue, and it is hidden in forced-colors mode.
- **The notch slides, it never pops**: while a panel is open, picking another connection card swaps the panel in place (`usePanelPresence(..., { swap: true })`) and the notch glides along the top edge from the old card's centre to the new one (`translateX`, 280ms, `cubic-bezier(.4,0,.2,1)`) while its accent colour cross-fades. Picking again mid-slide retargets from the notch's current position. A window resize glides the notch to its re-measured position; a burst of resize events (a drag) snaps. It never slides during a close or on the first open (there it rises at the card). A swap never replays the rise.
- **Interruption**: asking for the opposite direction mid-flight calls `reverse()` on the running group, which continues from the current progress. An open removes all its animations at the end and leaves the final styles to plain CSS (`will-change` is set only while animating); a close keeps its last frame until the panel unmounts.
- Single column (<640px): no horizontal spread, the strip only grows down. While closing the panel is `inert` and `aria-hidden`, and focus inside it returns to the card. Reduced motion: a 120ms fade of the panel and notch, no travel, scale or notch rise.

**Inspecting the motion (dev hook):** `localStorage.setItem("spGenieMs", "6")` (or `?genieMs=6`) multiplies the 500ms duration (1..49 = times slower); 50..20000 sets it in milliseconds (`"3000"` = 3s). Applies on the next open, ignored under reduced motion; `localStorage.removeItem("spGenieMs")` restores 500ms. The notch slide keeps its 280ms. Nothing sets the key, so it is inert in normal use.

### 6.5 Other primitives
- **`Panel`** (`variant` default | setup | live; `tone` = connection): `surface-panel` fill, `edge` border, 12px, `elevation.default`. `setup` adds the connection-tinted wash and 20px padding; `live` adds the red rail and 135° wash.
- **`SaveBar`**: §9. **`Badge`/`StatusBadge`**: full radius, uppercase pixel text, hue at 10-15% fill. **`InlineAlert`** (warning/danger → `role="alert"`; success/info → `role="status"`): 8px, hue 10% fill, 20-30% border, under the thing it is about.
- **`ScrollRegion`**: any box that scrolls on its own is drawn as an obvious bordered region (1px `ink-muted` border, 12px, `surface-950`), with an 18px fade **over the content, inputs included** along any edge with more to reach. Keep it focusable and named (`role="region"`, `tabIndex=0`, `aria-label`). An empty list is not framed. Do not use a dark shadow (invisible on `surface-950`).
- **Borders inside a panel:** a panel is one box. Only **form fields, the Advanced/Activity/Feature rows, `ScrollRegion`, the Save bar, and the two Remote-access groups** get their own border. Group everything else with a heading, spacing or a single `edge-subtle` divider. Never a divider *and* a box around the same thing.
- **Dialogs** (`fixed inset-0`, `scrim`, centred `surface-950`, 12px, `elevation.overlay`, 44px actions) and **toasts** (bottom corner, 16px, red border) are for things the user did not just cause (crash, update) or for confirming something destructive. Anything about the current panel is an inline alert.

### 6.6 Section templates (`components/sections/`, decided 2026-11-18)
A config page is built from **section templates** plus the content a page passes in. A section type looks the same wherever it appears, with only wording and the connection's own accent differing, so a design change to a section type is made **once, in `components/sections/`**, and every page that uses it changes together. A template takes only props and slots (no page knowledge, no fetching, tokens only) and has its own test file that pins its DOM contract: order, roles, aria, test-id hooks, keyboard order, narrow/wide classes. **The rule is enforced, not hoped for:** a lint rule stops a page file from rebuilding a template (see "The guard" below), and a test per page fails when its sections change order.

**The templates** (all done; a page is built ONLY from them and the content it passes in):
1. **`ConnectionSection`** (and **`ConnectionRow`**, the one wrapping line of address, port and main buttons inside it): the Live Link block (Lighting's sACN DMX Live Link, Playback's Playback Live Link, Presentation's ProPresenter Live Link).
2. **`HelperSetupBox`**: the one helper setup box (the command, Copy, the 30-minute countdown, Cancel, auto-close when the helper answers, "Update ..."), replacing the three copies.
3. **`SettingsSection`**: a group of `Field`s in the standard responsive grid, by the width of the panel (a container, never the window). `columns`: `standard` 1, 2 or 4 across (28rem, 56rem; never 3 + 1), `inset` the same inside a padded row such as Advanced (so it switches where the summary cards above it do), `pair` two from 30rem, `triple` three from 40rem, `lead` three from 30rem with the first twice as wide, `single`. An optional uppercase kicker (`title`), `divided` (a rule above: "a divider and the fields", never a sub-heading), `width="narrow"` (a lone field), `boxed` (one bordered box: the two Remote-access groups), and one `note` under the group. The held marker (`Not saved yet`, a dot AND the words, `HeldMarker`), the hint and the error line belong to `Field`; `busy` disables every `Input` and `Select` inside, so no page passes `disabled` field by field (a control that is off for its own reason still says `disabled`).
4. **`ActionRow`** (and `ActionButton`): the Test / Refresh / Load buttons in one wrapping row of tinted buttons in the connection's colour, with the busy words (`Refreshing…`) and its one note ("Test connection and Refresh use the saved settings. Save configuration first to try the new ones."). Quick actions never save anything. `align="center"` keeps a button level with a `ReasonPill` beside it; `align="end"` sits the buttons level with inputs in the same row (Lighting's test cue: note, velocity, Send). **A control that is switched off says why (decided 2026-11-18, ideal "disabled controls say why"): give `ActionButton` / `ReasonButton` a `reason` (`Sign in to Planning Center first.`) and it is disabled, titled on a `<span>` wrapper (a disabled button shows no tooltip of its own, and a phone has no hover) and described to a screen reader by a hidden copy; `ReasonWrap` does the same for a select or an arrow. No page writes its own wrapper.**
5. **`StatusSummary`**: facts as a `dl`. `list` (under a rule, at the bottom of a page): four across while the words fit, then two, then one, in as many columns as there are facts (3 or 2). `cards` (each fact in its own recessed box, with an optional second line): 1, 2 or 4 across. A fact's `tone` is words AND colour: `ok` wears the connection's colour, `warn`, `danger`.
6. **`SectionNotice`**: one line of news in a tone. A **box** is an `InlineAlert` (warning and danger are `role="alert"`, info and success `role="status"`; info and success wear the connection's colour inside its panel, a `neutral` note does not), with an optional bold title and buttons; a box that asks a question the person just caused is a named **`group`** (not an interruption). A **line** is the same words without the box, in the tone's text colour (no role unless the page asks), for what belongs to a field or a button: why Save is off, what a restart does. A line may carry its button beside the words (`actions`: "Use the OSC host"); the row then holds the role and the test id.
7. **`OptionalSection`** (and `OptionalBand`): the collapsible optional block, the three `Disclosure`s the app has (`advanced` after the general settings, `activity` for read-only or rarely used things, `feature` for a major optional feature) with the persisted open flag: `persistKey` remembers the person's choice across restarts (Remote access, Song order, the Lighting timeline and cue board all start open), `forceOpen` opens it while something inside needs the person (an open question, an invalid field) without changing what they chose. `OptionalBand` wears the same card as the `feature` Disclosure (a rail and a wash in the connection's accent, the title in the accent with the one-line summary under it, a rule above the open body) and its header also carries buttons (Playback's Song order and its Scan button): toggle, heading, one-line summary and the actions on one row, the body folds in 300ms and is `inert` while closed. `forceOpen` opens a folded band while something needs the person without touching what they chose (their press is passed to `onOpenChange`), and `tone="presentation"` tints it in the Presentation colour (the Presenter Link and the Auto-Presenter). `wrapSummary` lets the summary wrap instead of being cut off beside the header's buttons and keeps the toggle at least 16rem wide, so on a phone the buttons drop to a row of their own (the Auto-Presenter, whose state must be read whole).
8. **`PanelLayout`** (and `PanelBody`): the order every configuration page follows, written once.
9. **`AddressBox`** (`addressGroups`, `scannedAddresses`): the ONE address box of every Live Link block (Lighting's Host, Playback's computer, ProPresenter's Host), decided 2026-11-18 (Hunter: "an IP address typed manually in the box, and a little arrow on the right that drops down the recent IP addresses chosen and the recently scanned IP addresses ... But I always want the open ability to edit the current address or write a new one in its place"). **The text is always editable**: an ordinary mono text box (`CONTROL_40`, 44 px on a coarse pointer) whose value the block owns; typing, pasting, editing the current address and replacing it work with the list closed or open, and no block has an "Other computer" choice or a second box. A small arrow sits INSIDE the right edge (the visible arrow 16 px; its hit area 32 px wide, 44 px on a coarse pointer, full height; not a tab stop: the box is, and ArrowDown there is the keyboard's way in). **The list** is the standard combobox: the box is `role="combobox"` with `aria-expanded`, `aria-controls` (the list's id), `aria-autocomplete="none"` and `aria-activedescendant`; the list is a `role="listbox"` as wide as the box, `max-h-60` and scrolling, over the content below it (`z-20`, `elevation-overlay`, `surface-950`), holding `role="option"`s in two named groups and nothing else: **Found** (the computers of the LAST search, while its result is the latest one: each reads `Name (address)` when the search knows a name, else the address; hidden when there is none) and **Recent** (`useRecentAddresses(scope)`: the addresses that worked, newest first, at most five). An address in both is shown once, under Found. The address already in the box has a check (and the words "(in the box now)" for a screen reader). With nothing in either group the arrow stays and the list says **No recent or found addresses yet.** (a calm line, not an error). **Keys:** ArrowDown / Alt+ArrowDown open the list (ArrowDown goes on to the option that is in the box, or the first; Alt+ArrowDown only opens); with it open ArrowUp / ArrowDown move through the options (wrapping), Enter chooses the active option, Escape closes it (and goes no further), and typing edits the box and closes the list, so the next Enter is Connect. **Closing:** Escape, choosing, a click outside, focus leaving; the list never traps focus (Tab goes on). **Choosing only puts the address in the box** (and the port a scan reported, through `onPick`, for ProPresenter): it never connects; **Connect** does, and **Enter in the box with the list closed** is the block's Connect (`onSubmit`; Playback's box sits in a form, so the form does it). `working` is the address that is connected right now: it is remembered, so it is in Recent next time. A held change is the warning border (`held`) with the caption's "Not saved yet"; an error is `error` plus the sentence under the row (`aria-describedby` names both); `disabled` while a request runs stops the box, the arrow and the list.
10. **`FactChips`** (decided 2026-11-19, Hunter: "change the format of 'Ab · 4:37', 'G · mtID 18191', 'Not captured · 65 BPM' in all the panels to use the style that I had you remove from the 'Key C' ... separated by that style instead of a '•'"): a song's small facts as a row of **square chips**, ONE chip per fact, and **no separator character** (no dot, no bullet: the box is the separator). Use it wherever a list of a song's facts was one dotted line: the Playback config's song cards (key, length), the Presenter Link's song cards (key, `mtID-23126`), Lighting's song cards (capture words, the loudness note, BPM) and the matchup ledger (key, BPM). Do NOT use it for a sentence, a status line or a count summary that merely has a dot between clauses (`1 to add · 2 in playlist`, the Live Link status line), nor for the order of a song's sections. **The look, written once in the template and never per page:** `inline-flex items-center border border-edge-medium bg-surface-950 px-1.5 py-0.5 font-mono text-[0.8125rem] tabular-nums text-ink`, square corners, a `gap-1` wrapping row (a second row only when the card is too narrow, 360 px), a long fact is cut inside its own chip with its full words as the hover text. The words are the facts' own and stay plain (`Ab`, `4:37`, `65 BPM`, `mtID-23126`; the key is the bare letter, no `Key`); each fact may carry a `name` for a screen reader (`Key Ab`) and the list is named by them (`Key Ab, length 4:37`). A `tone` (`success`, `warning`, `danger`; default neutral) changes only the outline and the words' colour, and only for words that already said it. It is `role="list"` / `listitem` on spans (valid inside the button that is a whole card). Zero facts draw nothing.
11. **`SectionButton`** (decided 2026-11-24, Hunter: "a nice big section button in the presentation section"; restyled the same day, Hunter: "I do want it to look a bit distinct from the Presenter Link, and I want it to have the versebridge logo and be under the presenter link section"): the section that opens a page of its own instead of changing anything on this one (VerseVision, from the Presentation panel). It is deliberately NOT the feature card of the `OptionalBand` above it (that one folds open here; this goes somewhere else), but it shares its height and rounding: a flat dark card (`rounded-ds-lg border bg-surface-900/60` with the neutral dark outline (`border-edge`, a lighter one on hover; Hunter: the orange outline was too much), `min-h-[4.25rem]`; no rail, no wash) laid out as the page's **logo** in a 48 px tile at the left with a soft glow of the connection's colour behind it (`blur-md`, 70% opacity, `data-part="glow"`; `logo` prop; VerseVision passes `assets/versevision-logo.svg`, the app icon of the VerseVision page, as an `<img alt="">`), the title in plain `text-lg font-bold text-ink`, ONE short line under it in `text-sm text-ink-body`, and an **Open** pill with an up-right arrow at the right (the connection's tinted colours from `accentClasses`; the word hides below `sm`, the arrow stays), all one real link (`<a target="_blank" rel="noopener noreferrer">`, so it can be copied, middle-clicked and opened in a new tab) with a screen-reader-only `(opens in a new window)`. It sits in the page's own content AFTER the Presenter Link section and before the notices and the Save bar. It has no busy or disabled state: it never saves anything. In the desktop app the page is not on the server's address, so the page passes `onOpen` and the click goes to the system browser through `openExternalUrl` (the same call the other "open in the browser" buttons make). It is not drawn for a remote session (the server answers 404 there).

**Song cards (`SongCardShell` around `SongCardBody`).** A flex column inside a `<button>` always writes its `items-*`: WebKit's button default is `flex-start`, so the colored arrangement bar was a quarter wide in the Mac app until `items-stretch` was written down. `frontend/guard/buttonItems.guard.test.ts` fails a `flex-col` button in `components/` that does not. A song card always has a cover slot: the picture, or the placeholder (`CoverSlot`: a 28 px `bg-surface-900` tile with a quiet music note, `aria-hidden`, the same rounding), when the song has no picture or the picture fails to load, so the titles line up in every list.

**The order of a configuration page (`PanelLayout`), top to bottom, never changes:** (1) the **header** (`SetupPanelHeader`); (2) the **alert lines** (loading, connecting, a failure or message: one `mt-4 space-y-3` group that hides itself when empty); then the body, one column with one gap (`mt-6 space-y-4`, so no page sets margins on a section): (3) the **connection** (`ConnectionSection`, or the page's own connection row); (4) the **settings** (`SettingsSection`s); (5) the **actions** (`ActionRow`); (6) the **optional** sections (`OptionalSection`); (7) the page's own **content** (the song list, the MIDI monitor, Connection Access); (8) the **notices** (why Save is off, the last failure or message of the page); (9) the **Save bar**; (10) the **summary** (`StatusSummary`), which a page may open with instead (`summaryAt="top"`: Backend's Status cards are the first thing read there); (11) what sits **below** the body (dialogs, the support-log buttons). A page leaves a slot empty to skip it. A page whose body is itself a `PanelBody` (the Playback panel inside the MIDI / Playback page) hands it over as `children` and puts the Save bar in `below`. The Playback panel's load and message lines sit in the alert lines; the answer to Refresh inputs stays right under the input row.

**The order of a `ConnectionSection`, top to bottom, never changes:** (1) the **title and the state pills** (a dot AND words, one pill per connection); (2) the **connection row**: the address input(s), the port and **the same two buttons at its end, always, in this order: `Search`, then `Connect`** (decided 2026-11-19: Search finds a computer, the word Scan is kept for reading the songs) (same height, same place; props `onScan`, `onCancelScan`, `onConnect`, `scanning`, `connecting`, `scanDisabled`, `connectDisabled`; while searching Search reads `Searching… (cancel)` and cancels, while connecting Connect reads `Connecting…`; a block with nothing to search still draws both, Search off with a hint; `Connect` on a block that is already connected only checks again and the status line says so; no block writes its own `Test connection`, `Check again`, `Scan Network` or `Scan again`); **the pair has no page-specific look** (decided 2026-11-19): `ScanConnectButtons` takes no class prop, every block draws `variant="tinted"` in its own `tone` (`lighting`, `playback`, `presentation`), and the disabled and busy states are the template's, a test pins that the three blocks render identical button classes apart from the tone, with what belongs right under it in the `ScanArea` (field errors, and the shared scan pieces `ScanProgress`, `ScanChooser` and `ScanHelp`, which read ONE `ScanState` that each block's controller adapts its own backend into); (3) **ONE status line**: each connection's words joined by ` · ` (stacked on a narrow block, inline from 640px), one polite live region; (4) the **actions row**: any quick actions (Lighting: Release lights, Read fixtures, Send a test cue, How to find a cue's address) and then the **Details button** with the arrow (right when closed, down when open; `aria-expanded`, `aria-controls`); (5) the boxes a quick action opens; (6) the **Details area**, closed by default: the helper setup box, the update line, notes. The connection row never moves and the details never push it. The Details open **by themselves only when something needs the person** (the helper is not set up, or is behind and StagePilot cannot update it by itself, or a note warns); they open again whenever that turns true again, and the person's own choice (the button) wins until then. A Details button exists only when there is something behind it. Anything that must not hide (an error, a saving line, a chooser the person has to answer) sits outside the Details area. The wording is props: the Details button reads `Presenter Link setup` (`Update the Presenter Link`), `Library Link setup` (`Update the Library Link`) or `Lighting Link setup`.

**Each template's props, in one line** (the types in `components/sections/` are the truth; every template also takes `className` and a test id):

| Template | Props |
|---|---|
| `ConnectionSection` | `title`, `pills`, `connection`, `status[]` (`{ text, testId, tone }`), `actions`, `panels`, `details` (a node, or `(state) => node`), `detailsLabel`, `attention`, `tone`, `testId`, `testIdPrefix`, `statusTestId`, `footer` |
| `ConnectionRow` | `children`, `as` (`div` or `form`: Enter presses Connect), `id`, `nested`, `after`, `testId`, `tone`, and the fixed pair: `onScan`, `onCancelScan`, `onConnect`, `scanning`, `connecting`, `scanDisabled`, `connectDisabled`, `scanHint`, `connectHint`, `buttonClassName` |
| `ScanArea`, `ScanProgress`, `ScanChooser`, `ScanHelp`, `ScanConnectButtons` (+ `SCAN_WORDS`, `NO_SCAN`) | the scan's room under the row, its one progress line, the computer chooser (several found), and the "nothing found" / macOS "local network blocked" help; all read one `ScanState` (`scan`) and one `ScanWording` (`wording`) |
| `HelperSetupBox` | `wording` (`testId`, `headings`, `why`, `waitingFor`, `done`, `before`, `paste`, `label`, `permissions`), `controller` (`start`, `end`, `watch`), `state`, `answered`, `quiet`, `quietNote`, `lead`, `autoStart`, `hasComputer`, `needComputer`, `endOnClose`, `tone` |
| `SettingsSection` | `children` (`Field`s; a `Field` may carry an `action`: its own button, right under its control), `title`, `columns` (`standard`, `inset`, `pair`, `triple`, `lead`, `single`), `width`, `divided`, `boxed`, `busy`, `note`, `noteId`, `aria-label` |
| `ActionRow` | `children` (`ActionButton`s), `note`, `noteId`, `align`, `aria-label` |
| `ActionButton` | `label`, `busyLabel`, `busy`, `disabled`, `reason`, `alwaysWrap`, `wrapProps`, `onClick`, `tone`, `variant` (`tinted`, `primary`, `secondary`), `title`, `aria-describedby` |
| `ReasonButton`, `ReasonWrap` | `reason` (why the control is off: with one the button is disabled, wrapped in `<span title>` and described by a hidden copy), `alwaysWrap`, `wrapProps` (`data-*` hooks); `ReasonWrap` takes any control as `children(describedBy)` (a select, an arrow) |
| `ReasonPill` | `children` (the short reason in words), `id` (what the button's `aria-describedby` points at), `testId`: the reason drawn BESIDE a button as a small neutral pill (info icon and words, never red), put inside the button's `ActionRow align="center"` so it wraps under the button on a phone |
| `FactChips` | `facts[]` (`text`, `name`, `content`, `tone` (`neutral`, `success`, `warning`, `danger`), `title`, `testId`), `label`, `testId`, `className` |
| `StatusSummary` | `items[]` (`label`, `value`, `tone`, `detail`, `mono`, `tabular`, `testId`), `variant` (`list`, `cards`), `label` |
| `SectionNotice` | `tone`, `children`, `variant` (`box`, `line`), `title`, `actions`, `role`, `group`, `quiet`, `live`, `neutral`, `strong`, `size`, `id` |
| `OptionalSection` | `title`, `summary`, `detail`, `aside`, `count`, `countNoun`, `tone`, `variant` (`advanced`, `activity`, `feature`), `persistKey`, `defaultOpen`, `open`, `onOpenChange`, `forceOpen`, `children` |
| `OptionalBand` | `title`, `summary`, `summaryTestId`, `actions`, `persistKey`, `defaultOpen`, `forceOpen`, `onOpenChange`, `tone`, `headingId`, `children` |
| `SectionButton` | `title`, `description`, `href`, `onOpen`, `className`, `testId` |
| `PanelLayout` | `id`, `tone`, `accent`, `header`, `alerts`, the slots `connection`, `settings`, `actions`, `optional`, `content`, `notices`, `saveBar`, `summary` (+ `summaryAt`), `bodyId`, `children`, `below` |
| `PanelBody` | the slots of `PanelLayout`, alone (a page whose body is shown inside another page) |
| `AddressBox` | `value`, `onChange`, `scope` (`playback`, `lighting`, `presentation`), `scanned[]` (`{ host, label, port }`: `scannedAddresses(scan)` makes it from a `ScanState`), `working`, `onPick`, `onSubmit`, `disabled`, `error`, `held`, `placeholder`, `title`, `aria-label`, `aria-describedby`, `inputRef`, `className`, plus `defaultOpen` and `recent` for the gallery and tests; `NO_ADDRESSES` is the empty list's sentence |

`LiveLinkPill` (a dot and words per connection) and `LiveLinkField` (the caption over an `AddressBox` or another field) fill a `ConnectionSection`; `LiveLinkShell` and `DisclosureButton` are what it is made of and are never used by a page. The order of a panel is written once, in `panelOrder.ts` (`BODY_SLOTS` for the body, `PANEL_ORDER` with the header and alert lines), and `PanelBody` draws from it; the page tests read the same list.

**How to add a connection** (a new configuration page, in about 35 lines; this is `frontend/src/gallery/AcmeSetupPanel.tsx`, drawn in the gallery under "A new connection page", and a test fails if this text and the file differ):
1. Give the connection a colour in `connectionPalette` and a `PanelIdentity` (§3.4); never a colour of its own on the page. The example borrows Presentation's.
2. Name the file `<Name>SetupPanel.tsx` in `components/` (the guard finds pages by name). Build it from `PanelLayout`: the header and alert lines, then each slot filled with a template and the words and state that are the connection's own.
3. If it has a Live Link block with a helper, write its `...BoxProps` function in `components/helperBoxProps.tsx` and put a `HelperSetupBox` in `details`.
4. Add its states to `src/gallery/` (one `SampleState` per state: clean, held, error, busy, failed) and its test file; the page-order test (`sections/pageOrder.test.tsx`) gets one `it` that pins the page's sections.

<!-- example: frontend/src/gallery/AcmeSetupPanel.tsx -->
```tsx
import { useState } from "react";

import { SetupPanelHeader } from "../components/SetupPanelHeader";
import { AddressBox, ConnectionRow, ConnectionSection, LiveLinkField, LiveLinkPill, PanelLayout, SectionNotice, SettingsSection, StatusSummary } from "../components/sections";
import { Field } from "../components/ui/Field";
import { Input } from "../components/ui/Input";
import { SaveBar } from "../components/ui/SaveBar";

/** A new connection page: only templates, plus the words and the state that are its own. */
export function AcmeSetupPanel({ saved, onTest, onSave }: { saved: { host: string; port: string }; onTest: (host: string) => Promise<boolean>; onSave: (next: { host: string; port: string }) => void }) {
  const [host, setHost] = useState(saved.host);
  const [port, setPort] = useState(saved.port);
  const [testing, setTesting] = useState(false);
  const [reachable, setReachable] = useState<boolean | null>(null);
  const changes = Number(host !== saved.host) + Number(port !== saved.port);
  const test = async () => { setTesting(true); setReachable(await onTest(host)); setTesting(false); };
  const row = (
    <ConnectionRow connecting={testing} onConnect={() => void test()} scanHint="Acme cannot be found by scanning: type its host." tone="presentation">
      <LiveLinkField caption="Host"><AddressBox aria-label="Host" onChange={setHost} onSubmit={() => void test()} scope="presentation" value={host} /></LiveLinkField>
    </ConnectionRow>
  );
  return (
    <PanelLayout
      header={<SetupPanelHeader closeLabel="Close Acme configuration" status={reachable ? "connected" : "disconnected"} title="Acme console" />}
      id="acme-configuration"
      tone="presentation"
      connection={<ConnectionSection connection={row} pills={<LiveLinkPill name="" text={reachable ? "Connected" : "Not connected"} tone={reachable ? "success" : "idle"} />} status={[{ text: reachable === null ? "Not tested yet." : reachable ? "Acme answered." : "Acme did not answer." }]} title="Acme Live Link" />}
      settings={<SettingsSection busy={testing} columns="pair"><Field heldChange={port !== saved.port} label="Port"><Input onChange={(event) => setPort(event.target.value)} value={port} /></Field></SettingsSection>}
      notices={reachable === false ? <SectionNotice tone="warning">Check the host and the port, then test again.</SectionNotice> : undefined}
      saveBar={<SaveBar onDiscard={() => { setHost(saved.host); setPort(saved.port); }} onSave={() => onSave({ host, port })} state={changes ? { kind: "held", count: changes } : { kind: "clean" }} />}
      summary={<StatusSummary items={[{ label: "Host", value: saved.host }, { label: "Port", value: saved.port, tabular: true }]} />}
    />
  );
}
```

What no template fits (the song list, the MIDI monitor, a cue table) goes in the `content` slot of the page. If two pages need the same thing, make a template or give a template a prop, never a second copy.

**How to make a design change to one section type.** Say it is the Details button's label weight. (1) Find the one file: the template's own (`DisclosureButton.tsx`), or `index.css` for `.section-fields` and `.section-summary`. (2) Edit it; nothing in a page changes. (3) Run `npm test`: a template's own test pins its DOM contract, so only a test that pinned the old look fails, and that is edited with the change. (4) Build the gallery (`npm run build:gallery`), look at the section type on every page that uses it, at 360 px and 1280 px, side by side, and check nothing else moved. (5) State in the PR which pages carry the change. If a change is wanted on one page only, it is a prop or a slot of the template (wording and the connection's accent already are), never an edit of a page's copy: there is none. The first change made this way is recorded in the pull request that introduced the guard (a Details label weight, applied and reverted in two commits, with before and after pictures).

**The guard.** `frontend/eslint.sections.js` applies to every page file (`*SetupPanel.tsx`, `*ApiPanel.tsx`, `RemoteAccessPanel.tsx`, the Live Link blocks and `LightingOscParts.tsx`; tests excepted) and makes these an **error** whose message names the template to use: a `dl` (`StatusSummary`), an `Input` labelled Host or computer (`AddressBox`), a `grid-cols-` class (a field grid: `SettingsSection`), a `live-link` shell class or the panel-strong fill (`ConnectionSection`), the connection row's classes (`ConnectionRow`), a Details button or any `aria-expanded` button (`ConnectionSection` `detailsLabel`, `OptionalSection`), a Test / Refresh / Reload / Load `Button` (`ActionButton` in an `ActionRow`); and importing `Panel` (`PanelLayout`), `Disclosure` (`OptionalSection`), `InlineAlert` (`SectionNotice`), a template from one of its own files instead of `./sections`, or a Live Link piece from `LightingOscParts`. What is truly the page's own (a table of cues, a numbered list of songs, a row's own Edit toggle) takes `// eslint-disable-next-line no-restricted-syntax -- page-specific: <why no template fits>` on that line, so every exception is visible and has its reason. `frontend/guard/sections.guard.test.ts` runs the real lint configuration over made-up pages (each wrong thing is flagged and names its template, a page built from templates passes, the worked example above passes) and fails when a file that uses `PanelLayout`, `ConnectionSection` or `SettingsSection` is not covered by the guard.

**The section gallery** (`npm run gallery` for the dev server, `npm run build:gallery` then `python3 tools/preview_sections.py --bind 0.0.0.0 --port 8102`) draws the real blocks in every state for every connection with made-up data and no backend. It has: a **view switch** (one column, or **side by side**: one frame per connection, Presentation, Playback / MIDI, Lighting, Services, StagePilot, each with its Live Link block and its page, so the same section type can be compared across pages); a **width switch** (360 px, 1280 px, Fit) that sizes the framed page itself, so every breakpoint answers; and a visible **"Last built from <commit>"** line (the commit hash, "+ uncommitted changes" when the tree is dirty, and the time) so nobody judges a stale build. Its parts: the Connection section per connection; every configuration page (Presentation, Services, StagePilot, Playback / MIDI, Lighting, Remote access) as the REAL page, where a held change or an error is reached by doing what a person does (typing, pressing Save) and not painted on; each template by itself in a connection's colour (the address box in all three blocks at once: empty, typed, Found and Recent, only one of them, nothing yet, the keyboard in the list, a long list, error, held change, disabled; every column set, held, error, hint and busy fields, the action row idle, busy and disabled, the summary with ok, warn and danger words, every notice tone, the optional sections open, closed and remembered); and the panel order (every slot, and with the summary first) with the new-connection example. A new state is one more `SampleState` in `src/gallery/`. It is a review tool: it is not part of the web bundle or the desktop app (`dev-editor/productionBuild.test.ts` proves it).

---

## 7. The dashboard

### 7.1 Header (above the widgets)
- **Logo first, never moving.** One row at every width: the StagePilot logo sits **32px in from the left edge of the background image** (the image is 1700px wide and centred, so on a window wider than it the logo moves with the image, never into the black beside it). Everything else is one status pill to its right.
- **Status pill** (`HeaderStatusChip`): always the widgets' dark `surface-950` with a thin outline and bold-pixel words. Ready = green (`success-soft` outline, `success-text` words). Check system / Error = red (`danger-edge` outline, `danger-pill` words). **The pill is the notification:** on an info message the *same pill* grows to the message size, keeps the dark surface and takes the Previous blue (`live-back-soft` outline, `live-back-text` words); an error message takes the red; then it shrinks back. New messages show immediately (words slide, pill resizes); each lasts **5 seconds max**; simultaneous messages play in turn. **A message is always one line:** if it is longer than the room it has it is cut and ends in `...`. On a compact window (below 1024px) the message may grow leftward over the logo, because messages are short-lived, but never past the header's left margin, which is the same inset the header keeps on its right (16px under 640px, 24px from 640px). **The status words are one line too:** a status longer than the room beside the logo (`Error: Saved settings could not be read`, or `Record <song> again` with a long song name) is cut with `...` to that room and stays whole in the pill's tooltip and in the list (decided 2026-11-10: a problem must stay visible on a phone). Only when the room is narrower than the `Ready` pill (66px) does the status pill disappear (never a dot); a message still shows the same way. Announce via a visually hidden live region (the drawn copy is `aria-hidden`).
- **System check list** (the pill's hover/focus panel, above the widgets): lists only what the show cannot run without: Services, Service plan, Song durations, Playback, Presentation (and its timer), StagePilot. **Lighting is not in it** (optional). A problem that does not stop the show (Remote access offline, a song to record again) is listed only while it is a problem, with no "Optional" tag. **Row looks (decided 2026-11-18):** a row that blocks a service and is not ready is red with a **×**; an optional row that is not ready (the Playback library, Remote access offline, a song to record again) is grey with a **!**; a row that is fine is green with a **✓**. The mark carries it, never the colour alone, and the pill's own verdict is not read from these looks (`readinessRowTone`). The Playback entry names the saved method ("Playback API connected" / "Playback MIDI disconnected"). Words match the status cards.
- **System problems live in the pill and its list; banners are not used for them (decided 2026-11-10, owner: "the notices should show up in the header alerts like the rest of the alerts").** The status pill is the app's one place for what is wrong with the system, so a new kind of problem is a **readiness check** (`buildReadinessChecks`), never a banner, dialog or toast of its own. A check that is only a problem when it is one is **listed only then** (Remote access offline, the two below). Two kinds: *blocking* (`required`, `severity: "blocking"`, status `error`) turns the pill red and reads `Error: <label>`; *optional* (`required: false`, `severity: "warning"`) never blocks Ready, and when everything required is fine the pill reads its label in the same alarm colours (the first one leads; the list shows all). Labels are plain words that say what to do (`Record <song> again`, `Saved settings could not be read`), the sentence is the row's detail, and a row has **no dismiss button: it clears when the problem clears** (the dashboard checks the data it already holds, so no restart). The two settings checks: **settings file not read** (blocking, `Saved settings could not be read`, or `Settings are from a newer StagePilot`) is listed first; **a song's lighting set aside** (optional, one row per song, `Record <song> again`, gone once the song has lighting data again). **Scan Playback again** (optional, label `Scan Playback again`) is one more such row: listed only in the hour before the next service while Playback has not been scanned in that hour, gone when a scan completes or when the service starts, and **never listed while the loaded plan is going** (a service in progress: from its start until the last song has ended, between songs too; a run-through that is not part of a service, such as a sound check: only while a song plays and for 5 minutes after the last song activity; owner 2026-11-11: no scan error while the service plan is going) (decided 2026-11-11; no pop-up, no dismiss button, no sound). Only the full dashboard shows them; the read-only Live status view does not. A long label is cut with `...` like any pill message, and stays whole in the row and in the pill's tooltip. Progress that belongs to one card (the Lighting card's `Saving the recording of "song"...` line) is not an alert and stays on the card.

### 7.2 Startup and the hand-off
- Desktop startup: centred `brand-hero` wordmark, the blue pixel loading bar, one headline, one detail line. Phone/remote: a circular pixel spinner *instead of* the bar. Blue = loading, red = slow, red and stopped = failed. Remote sign-in: one centred card, one primary button, errors under the field in `danger-text`.
- **The loading logo glides into the header** (`startup/logoLanding.ts`): the dashboard's own header logo starts at the loading logo's centre and size and moves home (translate + scale, 720ms) while the rest of the dashboard is hidden from the first frame, then the whole dashboard fades in over 200ms (borders and contents together; a border-first stagger was tried and dropped). It never plays on a reload of the loaded app or in tests; reduced motion skips it.
- "Can't reach StagePilot" retries by itself when the dashboard loads and whenever that error occurs; the manual **Try again** remains.

### 7.3 Status cards (five, in this order: Services, Playback, Presentation, Lighting, Backend)
The fifth card is titled **Backend** while collapsed (Hunter's decision, 2026-11-18); its screen-reader name and the open panel say StagePilot. This is the one place the word is allowed in operator text (see section 10).
Icon tile + title + **status word** (Connected / Connecting / Not connected / Error) + one detail line. No status dot. The word is bold-pixel, uppercase, and **grows with the card** (about 12px up to a 22px maximum) so NOT CONNECTED always fits on one line; the title is 13px. Card hover/open tints the title and description whenever the *card* is hovered or open (not only the text). At **≤1000px** viewport width the cards switch to **compact** (icon tile, word on one line beneath it; never wraps, ellipsizes or overlaps). The open card wears its connection colour (§3.4).

### 7.4 Widgets (GridStack, 12 columns desktop / 6 tablet, 5px gutters)
Four widgets: **Service plan**, **Now playing**, **Manual controls**, **Recent event stream** (`dashboardWidgetRegistry.ts`).
- **The Event stream is in plain words (decided 2026-11-18).** Each row is a time and the words (`Playback connected`, `Presentation not connected`, `Service plan loaded`, `Timer stopped`), never the developer's dotted name; the raw `type · source` is the row's hover text, for support. `plugin.status_changed` rows are not drawn. A type the table does not know reads as plain words (`Plugin something new`). The table is `lib/eventWords.ts`.
- **Default (and "Reset layout") arrangement:** Service plan on the left (x 0, 7 wide); on the right, in one 5-wide column: Now playing, then Manual controls, then Event stream, all the same width as Now playing. First-time users get exactly this.
- **Free placement:** widgets can go anywhere they do not overlap. There is no spacer tool. A light **upward gravity** keeps gaps from forming at the top unless a widget cannot fit beside what is there; the startup pass settles by gravity only (no compaction).
- **No clipped content, no dead space:** every widget has min sizes; **Now playing and Manual controls heights lock to their content** (`dashboard/contentFit.ts`), so a user cannot resize them into cut-off controls or empty space. Service plan and Event stream scroll internally.
- **Now playing:** `live` panel with a left rail. At rest (idle/paused/stopped/ended) the rail is the dark Previous blue; when the countdown starts the live red **sweeps down from the top-left** and then a lighter band keeps travelling down it ("alive to the music"); on stop the blue sweeps back the same way. The label, elapsed time and glow wait for the sweep and follow it. Every colour change starts at the top; none fades the whole bar. Countdown: Geist Mono 300, +8px; elapsed time matches **Time remaining**'s style; `Up next` is teal, `Current · live` is the song actually started (`playing_song`), `Up next` is what **Start next** will play (`up_next_song`).
- **Service plan rows:** hover tints **dark** on non-song rows and a subtle red-orange "live" tint (`can-hover:bg-primary/[0.08]`) on song rows. The plan title uses bold pixel.
- **Manual controls:** six live-control buttons in **even rows**: 2 columns (default), 3 columns from 37rem, 6 columns from 74.5rem container width (`.manual-controls-grid`), never a ragged last row, and only more per row when the label fits on one line. Labels use bold pixel.
- Edit mode: **Edit layout** → drag the labelled **Move** handle, resize from the lower-right, **Compact layout**, **Reset layout**, **Done**. Normal mode locks every widget so a live press cannot move one. Layout data never leaves the browser.

---

## 8. Configuration panels: the shared anatomy

Every connection opens a panel under the status cards, in this order, which `PanelLayout` (§6.6) draws. **A new connection uses it and copies nothing.**
1. **Header** (`SetupPanelHeader`): kicker `Connection configuration` (the StagePilot panel says `StagePilot configuration`), a 20px bold **title in the connection's own colour** (`-soft`; the StagePilot title uses its pale `-text` tint because its red is only 3.2:1), a one-sentence description in body grey, the **status pill** and the 44px **Close** button. The pill sits in a **fixed slot immediately left of Close on every panel**, uses the real state colours (green/red/idle), **not** the connection tint, and does not change the title position. Below 480px the pill moves to its own row under the title.
2. **`ConnectionMethod`** (only if the connection has more than one way).
3. **General configuration** fields, ordered by how often they change.
4. **`Disclosure advanced`** (optional).
5. **Feature section(s)** (optional, `Disclosure feature`).
6. **`SaveBar`**, always last, always visible (§9).
Rules:
- Description is one short sentence of **what this does for the user**. Never explain the UI in the UI.
- Do not add sub-headings like "Service plan configuration" over a form that is obviously the form. Use a divider and the fields.
- The header subtitle describing a connection is **fixed and short**; it must not change when the method changes.
- A held field shows a red dot and `Not saved yet`. Unsaved edits give the Save bar a red edge **without moving anything below it** (reserve room for the border).
- Narrow: the Save bar's buttons stack full width below 480px and it is sticky at the bottom on phones/tablets.

### 8.1 Panel-by-panel reference (what exists today)
- **Services** (green): sign-in row, then service-plan fields, then the four **system checks**.
  - Sign-in row: **Planning Center OAuth** (primary, takes ~2/3) and a **Manual API Connection** `advanced` Disclosure (~1/3) side by side from 40rem container width, stacked below it.
  - Fields (a divided `SettingsSection`): Service type, Timezone, Service plan title preference, Preferred service time in **1, 2 or 4 columns** (28rem / 56rem container), never 3+1. Then `Load service types` / `Load today's plan` (an `ActionRow`). The sign-in states are `SectionNotice` boxes (info, success, warning) and the Manual API Connection is an `OptionalSection` whose Application ID, Secret and checkbox are a `pair` group with a Test connection `ActionRow`. A failure or message sits just above the Save bar.
  - The status checks at the bottom (`StatusSummary`) are four across while their words fit (panel ≥ 30rem), then two, then one.
- **Presentation** (orange): the **ProPresenter Live Link** block FIRST under the header and the panel's alert lines (the same `mt-6 space-y-4` wrapper and `!mt-0` block as the Playback panel; below), holding the connection (Host, API, `Search`, `Connect`) on TOP of its one status line and its collapsed Details; then Detected timer, Timeout (seconds), ProPresenter Look (first option reads **Current Look**, meaning "leave the Look alone"), `Refresh timers and Looks`; then the page's own content: the big **VerseVision** `SectionButton` (opens `/versevision/` in a new tab, or the system browser in the desktop app), then the Presenter Link section, then the **Auto-Presenter** section (own card, below); the Save bar. 
  - **ProPresenter Live Link block (decided 2026-11-18, copied from the Playback Live Link block).** A `ConnectionSection` (§6.6), the same bordered card as Playback's and Lighting's, top to bottom: a title and two pills (`ProPresenter API: Connected`, `Presenter Link: Connected`); **the address row first** (Host with recent addresses, **API** (the port of ProPresenter's network API: its caption says only API, its accessible name is `ProPresenter API port`), `Search`, `Connect`; a held change says `Not saved yet` in words and with the warning border, a field problem is a sentence under the row); **one polite status line** that says both in words (`ProPresenter API: Connected to ProPresenter at <host>. · Presenter Link: Ready, version 0.1.0.`); the **Details button** (`Presenter Link setup`); and the **Details, collapsed**, holding the local line, the import-blocker sentence, the update line and the setup box. A dot AND words. The Link is optional: `Not installed` is grey and never a warning, and it never changes the header's verdict. The words are `Ready, on this computer`, `Ready, version N`, `Update available, version N`, `Needs the command again`, `Not answering`, `Not installed`, and `Can add songs, cannot import missing songs (macOS Accessibility not allowed)` (teal, with the backend's one-sentence fix under the strip in body text, not a warning colour: it is something you may want, not a fault). ProPresenter on THIS computer needs no setup and says so inside the Details (`ProPresenter runs on this computer, so there is nothing to set up.`); another computer gets the Details button **Presenter Link setup** (**Update the Presenter Link** when behind and StagePilot cannot update it itself), open by itself while the Link needs setup, that holds the `HelperSetupBox` it shares with the Library Link and the Lightkey helper: nothing is made until its button is pressed, then the command, **Copy**, the 30-minute countdown, **Cancel**, and a polite `The ProPresenter Link is ready.` when it closes by itself. The block sits above the Save bar and changes nothing the Save bar saves.
- **Presenter Link (decided 2026-11-18; cards, sections and wording 2026-11-19; plain cards, one status chip and short statuses 2026-11-19).** A section of the Presentation panel, after the settings and before the Save bar, drawn only when the Presenter Link is ready; otherwise nothing at all (the feature is optional, so no warning). **It is an `OptionalBand` that starts folded (decided 2026-11-21): the heading `Presenter Link` is the toggle and ONE short line beside it says whether the songs can be imported (`4 songs ready to import`, `3 of 4 songs can be imported`, `No songs can be imported`, `Not while a song plays`, `No songs in the plan`; the playlist is never mentioned); a preview being made, a preview that stopped, a running or finished import and a problem open it whatever the person chose (the line then says `Importing…`, `Done: 4 added`, `Stopped: nothing changed` or `Problem`) and it stays open until they press the toggle; the body stays mounted while folded, so nothing inside is lost.** It is built from the templates (a settings group for the **Playlist** and **Section** pickers, `SectionNotice` lines, `ActionRow`s for **Select all / Select none** and for the one **Import songs to ProPresenter** button, `Importing…` while it works) and has no dialog: the preview, the progress, the result and **Try again** appear in place under the button, and the new part takes the focus. **Section** lists the playlist's header names in its order and nothing else (never `After "..."`): **End of Playlist** exists only for a playlist with no header, because the last section is the same place; a section is everything from its header to the next one and the songs go at its end; the default is the section last USED FOR AN IMPORT in that playlist (kept in this browser) while the playlist has it, else the LAST section. Two headers of one name are both listed and choosing it shows the engine's plain line under the box (the button stays off; nothing is guessed). The songs are the **same song cards as the Lighting tab's strip** (`SongCardShell` in check mode around `SongCardBody`: the `SONG_GRID` auto-fill grid, cover, title, a line of facts, the arrangement bar, 44 px targets): each card is a `role="checkbox"` button (Space/Enter), a selected card has Lighting's selected look (a solid border in the connection's colour, a tinted fill and the outline), an unselected one a dashed, dimmed border and fill, and **no mark in the corner**, so the colored arrangement bar runs the whole width along the bottom of the card exactly like Lighting's (decided 2026-11-18; the card reads title, fact chips, the bar, then the arrangement text under it), all selected at first, a polite count `3 of 4 selected`. **Card facts are square chips (`FactChips`, §6.6), nothing to open:** a Playback card is the title, then one chip per fact, `C` and `mtID-23126` (key letters, `Ab (was G)` when changed in the last day; the chip is `mtID-` and the number with no spaces, and `mtID—` with no id, decided 2026-11-18; the number is the MultiTracks Song Master ID, the helper's `content_id`, never Playback's setlist song number), then the order Playback plays written out (`Intro · Verse 1 · Chorus ...`, wrapping, full names also as the card's hover text), then the thin bar. A Planning Center card is the title and a small `Planning Center` chip; a song that cannot be used is the grey dashed card with its chip. **Statuses are ONE chip per song** (`StatusChip`: an icon and a word in a square outline; the word is the status, never colour alone) **and at most one short phrase** (about six words, no sentence, no full stop): `✓ In playlist` (green), `+ Ready`, `+ New copy`, `↓ Will import`, `? Check` (teal, informational, never red), `? Pick one` (its files as small mono chips, one per line in the preview; on the card one `SongFilePicker` button per file, below), `! Can't import`, `! Can't map` (`Missing: Bridge 2`), `! No mtID` (red: they stop the song), `− Skipped` (grey); steps are one word each (`Backup`, `Library`, `Importing`, `Copy`, `Loading`, `Playlist`, `Check`) with a spinner, check or cross. A row is aligned columns on a wide screen (title, a chip column of one width, the phrase) and stacks title, chip, phrase at 360 px. **Pick one is chosen on the card (2026-11-19):** under the title, inside the card's border and never inside its checkbox button, `SongFilePicker` is a `role="radiogroup"` (`aria-label` `<Song>: use file`) of small tinted `Button`s (`size="touch"`, 44 px, wrapping), one per file, labelled with the file name without `.pro` (the library too only for two files of one name), one tab stop, arrow keys, Home and End move and Space or Enter picks, the file in use `aria-checked` with a check mark; the pressed button takes `Button loading` and nothing else moves. A song that used a file without being asked (a pick or the name rule) shows its real status chip, the quiet phrase `Using “Firm Foundation (Live)”` and a small `Change` button (`SongCardShell`'s `under` slot) that brings the buttons back. The pick is kept in this browser under `stagepilot.presenter-song-file.<mtID>`. The long sentences the engine sends live only in the chip's hover text and in `sr-only` text. The preview's heading is the place (`Sunday Morning Service › Worship`) with ONE neutral line of counts (`1 to add · 2 in playlist · 1 can't import · 1 pick one`, zero counts left out; red only when the playlist blocks the whole import); the result's heading is `Done: 3 added`, `3 added, 1 differs` or `Stopped: Can't import "Battle Belongs"`, its differences short lines (`Item 7: duration 80 → 0`), the backup's file name and the ProPresenter version small lines (the version only in the result). Whole-job lines are one line: `Song playing: wait`, `Live on screen: clear it, then try again`. The section explains as little as possible: no sentence about what the button does, where the songs come from or the key (docs/propresenter.md explains, with the status words table); only what a person must act on. The button that is off says why in a few words beside it, never only by dimming; one polite status line announces the newest thing. The Playback panel has no import control.
- **Auto-Presenter (decided 2026-11-26, Hunter: "make it look nice in the StagePilot Presenter config front end as an Auto-Presenter section, a toggle that defaults to off and can only be turned on when there is a Playback Library Link (with current data) ... and a Presenter Link").** Its own section of the Presentation panel, under the Presenter Link and the VerseVision card and above the Save bar (Hunter, 2026-11-26): an `OptionalBand` in the **Presentation** colour (`tone="presentation"`, `wrapSummary`), the same card as the Presenter Link, so the two read as one family beside each other. The Presentation colour is kept on purpose: the feature drives ProPresenter, and a colour of its own would make it look like a fourth connection. Header row: chevron, the title **Auto-Presenter**, ONE summary line under it (a mark and words, never colour alone: `○ Off`, `✓ Following: Chorus, slide 3`, `! Paused: someone changed the slide in ProPresenter`, `○ Needs the Playback Library Link`) and the **switch on the row itself** (the shared `Switch`, its word `On` / `Off`, named Auto-Presenter), never inside a nested box; on a phone the switch drops to its own right-aligned row under the summary rather than squeezing it. It folds and remembers (`stagepilot.presentation.auto-presenter-open`); it starts folded while the switch is off; it opens by itself while it needs the person (paused by failures, a song that cannot be followed, a saved-on switch that is locked) until they fold it. **Always drawn; locked, never hidden:** the switch can be turned on only while the Presenter Link is ready AND Playback's Library Link is connected with current data; otherwise the switch is disabled (it can still be turned OFF when it was saved as on), everything inside is greyed, and ONE box says which link is missing and where to set it up (`Needs the Playback Library Link` / `Set it up in Configuration → Playback (Library Link setup).`; while the Library Link waits for the keychain prompt (helper state `needs_key`, right after an install) the box says `Allow the keychain prompt on the StagePilot computer ...` instead); a saved-on switch that is locked says `Waiting for ...` and nothing (not the line, not a row) says it is following. Inside, top to bottom: the reason box (locked only), the full sentence when the summary had to cut it (`Paused: ProPresenter did not take 5 slide changes in a row (...)`), **Try again** (only when paused by failures), the **song list** and the folded **Timing** group. The song list is one plain row per plan song (`rounded-ds-lg` box, cover slot, title, then ONE `StatusChip` in the backend's words with its mark, the quiet phrase, and for a song with no cues the ONE chip `~ No cues: following by bars` (the info look of the other fallback chips, never red: it is a notice, not an error; never a second chip that says the same) with one sentence naming the Lyrics Bus template its file needs); these chips live here only, the Presenter Link's song cards carry none. **Polish (2026-11-26, FL6):** the summary line is body `ink-body` with a hollow-circle mark (`○`) for off, ready and needs: never a lone dash; only `Following:`, `Paused:` and similar words and their mark take the success, warning or danger tone. The switch's `On`/`Off` is the `Switch` label at full `ink`, semibold; while locked a small padlock and the word `Locked` stand beside the switch (a locked Off never reads as an ordinary Off). Locked, the reason box is said once and the rows are only cover and title, greyed, with no chip. A row's help sentence is `text-sm` in `ink`. The folded **Timing** row is a normal setting (usable while locked; its title in `ink`, `Offset N ms` in `ink-body`, and a quiet `changed` mark when N is not 0). The gallery frame (never the app) keeps a normal gap between a state's annotation and its card.

- **Playback** (blue; accessible name "MIDI / Playback"): `ConnectionMethod` (Playback API primary / MIDI alternate). *Playback API view:* the **Playback Live Link** block (a `ConnectionSection`, §6.6) first: the **Playback computer** address box (`AddressBox`: type any address, or open its arrow for the found and recent ones), `Search` and `Connect` on top, with the scan's progress, the macOS permission help and the chooser right under the row (they stay in view); then ONE status line (`Playback API: Connected to Playback at <host>. · Library Link: Ready, version 0.4.0.`; while MIDI stands in the line is MIDI's own sentence and carries no `Playback API:` prefix) and the **Details button `Library Link setup`** (`Update the Library Link` when behind), closed unless the Library Link needs setup, holding the setup box and notes; network scan and automatic connecting are **always on and have no switch**; **Song order**: ONE song-scan button in its header row, naming the best scan there is (`Scan Playback Library` when the Library Link works and its order holds, else `Scan Playback API`; never hidden: greyed out with its reason as title and screen-reader text when no scan can run); **the section says each fact once** (decided 2026-11-18): the header summary is THE order fact (`Order from a scan (4 songs)`, `Order from the Library Link (4 songs)`, `No order yet`), while the order is being learned it is the neutral chip `Learning order (0 of 4)` (never a red sentence), and the explanations (what a scan does, what Check order means, `Songs never change places`) are in `docs/playback-api.md`, not on the page; Playback's songs and the service plan's songs as linked pairs (`Song arrangement`, below), with `Update Services` and the category menu; `Recent activity` (`activity`). *MIDI view (blue-violet accent)*: Available input (select, `Refresh inputs`, `Connect`/`Disconnect`); **Playback song velocities** (an `advanced` Disclosure) holds MIDI channel, Fixed note, Debounce (ms) and the six action velocities (unique, 100-127) and sits just above the **live MIDI note monitor**.
- **Song arrangement (Playback, decided 2026-10-14; redesigned with the Playback library 2026-11-17).** One **matchup ledger**: Playback songs and service-plan songs are **linked pairs, and nothing can be reordered**: each side can only **skip** a song, and the Nth song left on one side pairs with the Nth left on the other, so a crossed pairing cannot exist. Left column **Service plan**, right column **Playback**, a **join mark** between them (a square numbered by the pair: solid line for a pair that is fine, dashed warning line for a pair to check, no line when nothing joins). The look is the project's retro one: square 2px-bordered boxes, ruled rows (dashed between rows, solid top and bottom), `font-mono` `tabular-nums` for every length, key and BPM, and one **stamp** per row in `font-pixel` capitals with a mark so state is never colour alone: `✓ Matched` (green), `! Check key`, `! Check length`, `? Check title`, `! Check order`, `− Only in plan` (warning), `+ Only in Playback` (teal, information) and `⌀ Skipped` (grey, dashed). A "close match" reads `✓ Close match`. A row is ONE list item (`ol` named "Song pairs") that reads as ONE sentence for a screen reader ("Plan song 3, Easy To Worship goes with Playback song 3, Easy To Worship. Check key. Plan says key A, Playback plays Ab."); the drawn copy is `aria-hidden` and only the two Skip switches stay as controls. Under the pair the **differences are plain sentences**, never colour: `Plan says key A, Playback plays Ab.`, `Playback counts this song down to 7:22; the plan says 6:48.` (only when over 15 s apart), a close or uncertain title match with both titles, an order note. A skipped plan song reads `No track`; a skipped Playback song reads `Not in this service`; both are dashed, struck through and keep their switch. The only control is the shared `Switch` (a real `role="switch"`, 44px row) with the single word **Skip**, bottom right of every box. Skips are a **held change** saved by `Save configuration` (order: method, arrangement, MIDI). If a skipped plan song disappears from the loaded plan, cues are held back and the panel offers `Forget it`. A new scan clears Playback skips (a new setlist) but keeps plan skips. The group decides by **its own width**: under 35rem each pair stacks (stamp, plan box, vertical join, Playback box), the plan box kept left and the Playback box kept right. A skipped Playback song never touches the service timer.
  - **Without the Playback library** (no Library Link, or it does not answer): the Playback side is the saved scan alone, `Playback song N` and the scan's length, and the rows say `N s apart` (green within 30 s, `Probably a different song` past 120 s: a hint only, never an automatic change; that row is stamped `Check length`). Nothing about the library is drawn, and nothing is lost.
  - **With the library** (the Library Link answering): a **Playback setlist** line names the setlist the matchup reads (the one on the loaded plan's date, the newest of that date if there are several; never today's date) and is a polite live region. When Playback has none for that date it says so plainly and offers a small chooser of its other setlists (name, date, song count). Nothing is lined up until one is chosen; the choice lasts for that plan while StagePilot runs, is kept in memory only and writes nothing to Playback, Planning Center or the settings. Songs are paired **by title** (case, punctuation, accents, "&", "(Live)", brackets and "feat." tails do not matter) with a stated **confidence**: exact, close (same but for a bracket or "feat."), or uncertain (alike, not the same: both titles are shown and the row is stamped `Check title`; an uncertain pair is never used for the cover, key or waveform). A song only in the plan, or only in Playback, is its own row, in the place Playback has it. Each Playback box shows the 28px cover, the Playback title, the key Playback plays, BPM, the countdown length (hover: where the countdown ends) and the coloured section bar. **Order differences are shown and explained, never fixed**: with a saved scan tied to the setlist (by Playback's own song numbers; by place only when the scan shares no number with the library, and then the panel says so), each row says which Playback song will really start its timer with the skips as they are, and a skip on the song that is only on one side lines the rest up. With no scan the library alone is a comparison (no Skip switches, one sentence on how to turn them on). A scan that cannot be tied to the setlist leaves the plain scan view with a one-line note. The saved song order is still only ever written by a scan: the library is read-only.
- **Lighting** (pink-red): `ConnectionMethod` (OSC primary / MIDI alternate; the choice is saved and only that method's cues fire). Connection fields (OSC: Host (full name *Host or IP (OSC output + Live Link)*: the box's hint and the first line of the Details), UDP port; MIDI: MIDI Output, MIDI channel, Note length) fit one row when the panel is wide. Then the **Song lighting timeline** `feature` Disclosure, which holds: Song select, ♪# toggle and `Add Cue` beside it, **Test cue** (note/OSC address, velocity/argument, `Send test cue`, kept compact on one line and justified right of the song select, moving above it when narrow), the **Cue board** (saved reusable pads; press one while a song plays to capture a cue at the current time; `Preview only`, `Undo last capture`, `Edit pads`), the **Timeline** (marker labels stack on up to 3 rows; zoom), and the cue list in a `ScrollRegion` with Clear / Clear all. Lighting is optional, so it never turns the header pill to Check system. **Settings problems are not drawn here:** a damaged song set aside or an unreadable settings file shows in the header status pill and its list (§7, "System problems live in the pill"), never as a banner on this screen. **Saving line** (`LightkeySavingLine`, on the Lighting card): `Saving the recording of “song”…` in `ink` with its explanation in `ink-body` (never the dimmer `ink-muted`), on an opaque `surface-950` base under the Lighting 10% tint, from the end of a recording until its looks are saved. The dot is the solid Lighting colour (4.1:1 on the line, never faded, because a pulsing opacity dropped it under 3:1); the pulse is a soft ring around it, hidden under reduced motion so the still dot remains.
  - **sACN DMX Live Link block (decided 2026-11-04, matches the approved style-guide mock "Lighting: connection compacted to one titled line").** One bordered block (radius 12, deep fill, 10 x 12 padding) with, top to bottom: (1) a **header**: the title and the **two state pills** (dot + words, `aria-live="polite"`) right after it: `sACN: Connected` (the link to Lightkey; `Not connected`, `Connecting…`, `Off`, `Lightkey is closed`) and `DMX: 66 fixtures identified` (the fixture stream; `1 fixture identified`, `Connecting…`, `Not receiving`, and a grey `DMX: —` while sACN is not connected: both pills are always drawn); (2) the **field row**, bottom-aligned and wrapping by the block's own width: `HOST` (flex, mono, min 180 px; the box's hint and the Details say *Host or IP (OSC output + Live Link)*), `OSC UDP` (84 px, centred, tabular numerals), `API` (84 px), then the pair **Search** and **Connect** (the fixed pair of the ConnectionRow, 40 px; Connect on a connected block only checks again; never dimmed unless Lighting is off; `Connecting…` while a request runs; `Searching… (cancel)` while the search runs) as the **same tinted pair** Playback and Presentation draw (`variant="tinted"`, `tone="lighting"`; only the accent colour differs, decided 2026-11-19). It is an action, so it never uses the crimson fill: **the selected segment of a toggle is the only filled crimson in the block**. Then at the far right **Mode** (Auto | Replay | Record, `ConnectionMethod field`) alone (recordings are fitted, not thinned, so there is no thinning toggle); narrow, the host takes a full row, then the ports and Connect, then Mode; (3) a quiet **strip**: one muted status line (statements are separate sentences joined with ` · `, for example `Auto: replaying (no connection). · Lightkey API did not answer.`), then ONE group of actions that wraps as a unit (so `How to find a cue's address` is never alone on a row): `Release lights`, `Read fixtures`, `Send a test cue ▸/▾` (right when closed, down when open), `How to find a cue's address ▸/▾` and the Details button **`Lighting Link setup ▸/▾`**; the first two nested boxes open under the strip (the test cue: OSC address, argument rows, `Send test cue`), the Details hold the helper setup box and open by themselves while the helper is not set up or needs the command. The whole block is a `ConnectionSection` (§6.6): the test cue and the address help stay their own disclosures beside the Details, not inside them.
  - **Single-source status text.** Each fact is said once. The connection state lives in the pill (the reason, for example "Lightkey API did not answer.", is its tooltip and accessible name and appears once more in the strip's state line). The neutral "OSC ready for HOST:PORT. UDP does not confirm delivery." note is **one short info-toned line** above the block (the plain `info` look, not the Lighting pink-red banner; a real send or port error keeps the warning/error look) and is not repeated in a footer. The Detail explanation is the Detail switch's tooltip. The cue-address help is behind its disclosure and is never truncated. Pill colours follow "One word, one colour" in 3.3.
- **StagePilot / backend** (red): top to bottom: **Status** (four recessed cards, 1/2/4 across: Version, State revision, API endpoint, WebSocket), **Advanced** (Timezone first, then Server port, Log level, Release channel, 1/2/4 across; the restart-needed note sits at its bottom), **Connection Access** (§9), then the Save bar and support-log buttons (`Copy log`, `Send all logs to developer`) below a divider.
  - **Connection Access** is a `ConnectionMethod` titled "Connection Access": **Host** (this computer only), **Local network**, **Internet**. It **autosaves the moment you choose** (no Save needed). The dashboard PIN is always on (default `1234`) and its small input + `Save PIN` appear under Local network and Internet. Choosing Internet reveals **Remote access** (an `activity` Disclosure, persisted, starting open) with two bordered groups: the **Remote link** (address, `Copy link`, `Regenerate Remote link`) and **People** (viewers are read-only; operators can control StagePilot; `Add a person` opens its fields only when pressed).
  - New app-wide options go in this panel, never into a connection.

---

## 9. Saving

- **The `SaveBar` is never hidden**, even when nothing changed, and always says one of five things (same wording in every panel):
  1. clean: `All changes saved` + dimmed button with the reason beside it (`No changes to save`);
  2. auto: green check, `Saved automatically at 9:41 AM`;
  3. saving: `Saving…`;
  4. held: red, `2 changes waiting. Not saved yet.` + `Discard` + primary `Save configuration`;
  5. error: red, what failed + `Try again`; the field keeps its value.
  The bar is a deep tint of the connection colour (**below `lg`, where it sticks to the bottom of the screen, it sits on the opaque `panel` colour with a heavier top border, so the cards that scroll under it never show through the words and the button; decided 2026-11-18**); a *clean* bar is quiet, a *held* or *major* one is not. The Lighting feature card and Save bar sit side by side and must read as different things in one family (card has a rail; Save bar is a flat tint).
- **Held until Save configuration:** connection method, which computer, velocities, MIDI network/channel/note length, OSC host/port, Presentation timer/Look, Services type/plan choice, StagePilot port/timezone/log level/release channel. Reason: saving most of these reconfigures or reconnects something a running show may use.
- **Saved immediately (and the bar says "Saved automatically"):** Connection Access choice, the PIN via its own `Save PIN`, Lighting cue-list edits (add/remove/edit cues, cue board captures), `Search`/`Connect` host saves, Remote-access actions, UI preferences (collapse state, note-name display). Rule of thumb: **if pressing it during a show could break the show, hold it; if operators must do it live or it only changes what *you* see, apply it at once.**
- Held changes need **Save configuration** whether or not a show is running. Leaving a panel with held changes asks `Discard unsaved changes?` (focus lands on Discard; Escape and backdrop do not dismiss).
- **Send/Test actions** (Send test cue, Test) run when pressed using the *saved* configuration, and are disabled with a reason while a different output is selected but unsaved.

---

## 10. Words

| Say | Never say | Meaning |
| --- | --- | --- |
| **Connection** | integration, plugin, connector | something StagePilot talks to |
| **Services** | Planning Center (except sign-in/OAuth/API screens) | supplies the service plan |
| **Presentation** | ProPresenter (except where the product is the subject: "ProPresenter Look") | shows the countdown |
| **Playback** | MultiTracks | tells StagePilot a song started. Status card accessible name: "MIDI / Playback" |
| **Lighting** | Lightkey | receives cues |
| **StagePilot** (the app), **the engine** (the part that runs on this computer, in startup and crash messages) | backend, local backend, local API, server | the fifth card; app-wide configuration. Operators never see "backend" except the collapsed fifth card's title, **Backend**, which Hunter decided to keep (2026-11-18; the one allowed string in `noBackendWords.test.ts`); it stays in code, routes, logs and developer docs |
| **Connection method**, **Connection Access** | connection type, mode | how a connection is reached; who can reach StagePilot |
| **Host / Local network / Internet** | This Computer only, Remote access (as the option names) | the three Connection Access options |
| **OSC**, **MIDI** | | protocol methods; MIDI is Playback's alternate and Lighting's alternate |
| **Configuration**, **General configuration**, **Advanced** | settings, setup | what you edit; the two parts of a panel |
| **Service plan** | plan | the day's song list |
| **Cue** (a message sent to Lighting), **Note** (MIDI note and its length) | pulse | |
| **Cue board / pad / Timeline** | | Lighting's saved reusable cues, their buttons, and the visual song line |
| **Remote access / People** | remote users | the Internet section and who may sign in |
| **Connected · Connecting · Not connected · Error** | Disconnected (lower case inside a sentence is fine: `Services not connected`) | the four status words (sentence case in text, uppercase pixel in badges and cards) |
| **Current · live / Up next** | | the song actually started / the song Start next will play |
- **One date and time style (decided 2026-11-18):** dates read `Thu 19 Nov`, times read `2:31 PM` (12-hour, no leading zero; `2:31:42 PM` only in the Event stream), service times `9:00 AM`. Everything goes through `lib/dateWords.ts` (`dateWords`, `timeWords`, `clockWords`, `whenWords`); nothing formats its own, and no ISO date (`2026-11-19`) or `11/19/2026` is shown to an operator. The Services summary reads `Account: Signed in`, `Plan date`, `Last loaded`; the sign-in button is `Planning Center`.
- Sentence case everywhere except badges and kickers. Plain verb-and-object labels. Short: delete a sentence if the control already says it ("Required to open the dashboard from another device." was removed; so was "Sends cues by MIDI").
- Copy states what **will happen**, never how the UI works. Do not add text that explains a toggle the toggle already shows.

---

## 11. Checklist before you ship

1. **Tokens only?** No raw colours, no new radius/size/shadow/z-index. `npm run tokens:generate` run if you touched the YAML; `npm run lint` clean (0 errors; the baseline is 6 warnings).
2. **Right primitive?** You used `components/ui/`, not a fresh look. If you added a variant, you documented it here.
3. **Colour check:** each colour you used has exactly one meaning from §3. State is also a word. Contrast ≥ 4.5:1 for text, 3:1 for outlines.
4. **Where does it go?** Belongs to a connection → that panel's General configuration, Advanced or a feature Disclosure. Otherwise → the StagePilot panel. Same order as §8.
5. **Held or immediate?** Applied the §9 rule and the Save bar tells the truth.
6. **Responsive by container:** it works at 320px; multi-column groups use 1/2/4 (never 3+1) via `@container`; nothing wraps a status word or overlaps a neighbour; targets are 44px.
7. **Motion:** has a reduced-motion fallback, animates `transform`/`opacity`, does not delay a control.
8. **Words:** names come from §10; sentence case; verb-and-object buttons.
9. **Looked at it:** verified in a real browser at ≥3 widths (e.g. 1400, 900, 390), plus `npx tsc --noEmit`, `npm run lint`, `npx vitest run`, `npm run build`. Tests assert intended behaviour, not just "it renders".
10. **Docs:** this file (and the style guide if the visual rule is new) updated in the same PR. Run `npx --yes @google/design.md@0.4.0 lint DESIGN.md`.

### Do and don't, in one place
- **Do** let the connection colour be the only accent in its panel; **don't** use it for state.
- **Do** put the major optional feature in a `feature` Disclosure with a useful detail line; **don't** add a fourth kind of expandable row.
- **Do** persist the collapse state of optional sections; **don't** persist anything that changes what the show sends.
- **Do** pair a state colour with a word; **don't** rely on colour or on hover alone.
- **Don't** use the primary red for decoration, for text, or for more than one live item per view; **don't** let it carry an error.
- **Don't** put a bordered box inside a panel except the listed ones; **don't** nest cards.
- **Don't** hardcode brand names, widths of text containers, or pixel-perfect positions that depend on the window width; **don't** use `transition: all`.

---

## 12. Reference

### 12.1 Where things live
| What | Where |
| --- | --- |
| Tokens → CSS/Tailwind | `frontend/scripts/design-tokens.mjs`, generated `src/design-tokens.css`, `tailwind.config.js` |
| Global styles, container queries, keyframes | `frontend/src/index.css` (each block carries a comment with the decision) |
| UI kit | `frontend/src/components/ui/` (`gallery/` has a visual dev gallery) |
| Panels | `components/{PlanningCenter,ProPresenter,PlaybackApi,Midi,Lighting,Backend}SetupPanel.tsx`, `RemoteAccessPanel.tsx`, `SetupPanelHeader.tsx`, `PlaybackSaveFooter.tsx`/`usePlaybackSave.ts` |
| Dashboard | `components/Dashboard.tsx`, `components/dashboard/*` (layout, registry, `contentFit.ts`), `StatusCard.tsx`, `HeaderStatusChip.tsx` |
| Startup / hand-off | `src/startup/*` |
| Tests that guard the design | `tokensOnlyAll.test.ts`, `components/ui/tokensOnly.test.ts`, `connectionPalette.test.ts`, `statusCardHue.test.ts`, `narrowZoom.test.ts` |
| Dev visual editor | `docs/visual-editor.md` (dev-only click-to-edit side panel; changes still go through tokens) |

### 12.2 Decision log (why things are the way they are)
Keep these in mind before you "improve" them; each was an explicit owner choice.
- **2026-10-09 to 13:** Geist replaced Inter (bundled). The primary is a deep red (`#c7231d`); coral is retired. Errors and warnings are one pure red. Connected is a deep green (fill) with a lifted copy for dots. Teal is the secondary accent for information. Three borders (6/10/18%). One warm layered shadow (the hard "printed" shadow is gone). Light button labels carry a small dark text-shadow; dark labels carry none. Services/Presentation use generic names.
- **2026-10-14:** panels follow the style-guide look (kicker + coloured title + section kickers). `ConnectionMethod` replaced the select and the "alternate" row. Playback MIDI is blue-violet. Manual controls use bold pixel and always form even rows. The Now playing rail pulses live and settles blue. Backend status became four cards; Advanced moved under them with Timezone first.
- **2026-10-15:** header pill is the notification (dark surface, coloured outline, grows to the message, 5s). Lighting gained OSC (primary), a Cue board and a visual Timeline; MIDI Output cue lists are independent of OSC. `ScrollRegion`, `Disclosure feature`. Default dashboard layout stacks Manual controls and Event stream under Now playing. Free widget placement replaced the spacer.
- **2026-10-16:** the loading logo glides into the header. Connection Access (Host / Local network / Internet) autosaves; PIN always on. Playback "Automatically Connect" removed (always on). Services lost its "Service plan configuration" heading. ProPresenter Look default reads "Current Look". Stop timer / Reset position stay **rose** (the pill's dark red was tried and rejected). Dashboard borders do not appear before contents.
- **2026-11-04:** the Lighting Live Link block follows the approved mock (header pill, one field row with Mode at the right, a quiet strip). Connection state is stated once, in the pill, and every state colour comes from `statusTone` (teal Ready is never a fault, red Not connected is never fine).

- **2026-11-18 (section templates):** Hunter asked for the Presentation Live Link in the same format as the others (the address row at the top, the details below, collapsed behind a button) and for each part of a config page to be a template, so one design change updates every connection page. The three Live Link blocks are now one `ConnectionSection` and the three helper install boxes one `HelperSetupBox` (§6.6); a gallery shows every state of every connection. **Later the same day (guard):** a lint rule and a page-order test per page keep it that way (§6.6 "The guard"), and a change to one section type was drilled end to end (applied to the Details button label, shown on every page that uses it, reverted).
- **2026-11-18:** the ProPresenter Live Link block and the Presenter Link section follow the Playback Live Link block and the matchup ledger (see §8.1): the optional Link is grey when absent, stamps carry a mark and a word, and the section keeps the same list rows from either source so a swap never moves the button.

### 12.3 Still open
- Real-device touch, screen-reader and 200% browser-zoom checks are emulated only.
- A confirmation audit for every destructive action.
- Backend split for true per-field save-as-you-go.
- Decided exceptions: the compact status-card word may be below 11px; startup/error text over the brightest patch of the photo may measure 1.1:1.
- **2026-11-05 (smooth capture UI):** the three-way thinning control and the Simplify action are removed from Lighting (the capture fits ramps and loops instead). Scan and Connect are a neutral outlined pair (dark fill, accent border); the crimson fill belongs to the selected segment only. Timeline: a **ramp** (a row with `fade_seconds`) is a bar from its start to start + duration (at least 4 px) at the foot of its marker line, in the cue-type colour, with the marker as its handle; an **effect loop** is a striped band (`.effect-band`, colour `cue-effect` `#4fd1a5`, glyph `↻`, label `4.0 s loop x 16 fixtures`) on its own lane under the track, at most three lanes (loops that overlap beyond that share the lane that frees up first), 24 px tall (44 px on touch). The legend gains **Effect loop** (striped chip). Loops are read only: they are never moved or edited from the rows, and removing one asks \"Remove this loop? The motion it plays is lost until you record again.\" Rows that cannot be merged (a ramp with an instant row, ramps of different length or start, the same property twice) disable Merge with the reason beside it.
- **2026-11-10:** system problems (settings file not read, a song set aside) are header-pill readiness checks, not banners; `SettingsNoticeBanner` is removed (see §7, "System problems live in the pill").
- **2026-11-19:** Search, not Scan, in every Live Link block (`Search` / `Searching… (cancel)`, the address list's `Found` group); Scan is kept for reading songs, and the Playback Song order has ONE song-scan button (`Scan Playback Library` or `Scan Playback API`, greyed out with its reason, never hidden). The Presenter Link's songs are the Lighting song cards in check mode, its Section box lists section names (`End of Playlist` only for a playlist with none; see 2026-11-19 below), and its button is `Import songs to ProPresenter`.
- **2026-11-19:** the Presenter Link's statuses are short: ONE chip (icon + word) and at most one short phrase per song, a counts line instead of sentences, cards in plain text (`C · mtID-23126` and the written-out order, no Sections button, no key chip), the Playback Live Link's address box captioned `Host` like the other two, and the Section box without `End of Playlist` when the playlist has sections.
- **2026-11-19:** a song's facts are square chips, never a dotted line (`FactChips`, §6.6): the Playback config's key and length, the Presenter Link's key and `mtID`, Lighting's capture words, loudness note and BPM, and the matchup ledger's key and BPM. The key is the bare letter everywhere. This replaces the plain-text `C · mtID-23126` line of the entry above; sentences and counts that merely have a dot between clauses are unchanged.
- **2026-11-24:** the Presentation panel gets a `SectionButton` band labelled VerseVision (a flat card with the VerseVision logo and an Open pill, under the Presenter Link) that opens VerseVision, StagePilot's built-in verse-slide page, in a tab of its own (docs/versevision.md). A new section type, so a template (§6.6 item 11), not a one-off button.
- **2026-11-21:** the Presenter Link section folds and starts folded (`OptionalBand`, tone `presentation`, remembered under its own key); its header line says whether the songs can be imported, and work in progress (`forceOpen`) is never hidden.
- **2026-11-11:** "Scan Playback again" is a header-pill readiness check (a problem row, shown only in the hour before a service while Playback has not been scanned in it, and not while the plan is going: a service in progress, or a sound check that is still playing or stopped under 5 minutes ago); see §7, \"System problems live in the pill\".
- **2026-11-26:** the Auto-Presenter (the lyrics-following switch, earlier "Follow Playback in ProPresenter") is a section of its own in the Presentation panel under the Presenter Link and the VerseVision card, above the Save bar: the same `OptionalBand` card, the switch on the header row, one summary line, locked with one reason until both links work, and the per-song chips move out of the Presenter Link's cards into its list.
- **2026-11-26 (FL6):** Auto-Presenter polish: the status line is body ink with a hollow-circle mark instead of a dash, the locked switch wears a padlock and `Locked`, a song with no cues has ONE chip (`No cues: following by bars`), locked rows show only cover and title, row help and the Timing row are one step brighter (Timing a real folded setting with a `changed` mark), and the gallery has the section gap above each annotation's card.
