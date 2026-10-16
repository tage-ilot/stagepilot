---
version: alpha
name: StagePilot
description: >-
  A dark, high-contrast live-production control surface. Calm slate panels,
  one deep red accent (#c7231d) for "live / primary", and unmistakable status colors,
  readable at arm's length in a dim booth. Values are AS RENDERED today
  unless marked CHANGED (decided) or OPEN (still needs an answer).

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
  integration-lighting: "#d53367"          # DECIDED (owner, 2026-10-12): pink-red. UI label "Lighting" (was "Lighting"). 3.9:1 on panels: fill/border/dot only. NEAR the error rose: see "Colour collisions"
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

# StagePilot design reference

> **Status: DRAFT v3 for owner review. Not yet wired into the app.**
> Extracted from `frontend/src/index.css`, `frontend/tailwind.config.js` and
> `frontend/src/components/**` of `tage-ilot/stagepilot-beta` @ `cfb439f`
> (1.1.104-beta.31). Edit the values in the YAML block above; prose below
> explains intent. Markers used in comments: **CHANGED** = differs from what
> renders today, on purpose; **OPEN** = needs your decision; **UNUSED** = defined
> in code but never used by a component.

## Overview

StagePilot is used by a tech running a live service, often in a dim booth,
glancing at a screen while their hands are on something else. The interface
is a **control surface, not a marketing page**: dark and low-glare, with large
legible numbers, generous hit targets, and colour that carries meaning. It
should feel calm when everything is fine and become impossible to miss when it
is not.

1. **Calm slate, one hot accent.** Everything is a dark blue-grey panel except
   the one thing that matters *right now*, which gets the primary red.
2. **Colour means state.** Green, red (errors and warnings) and the connection colours are reserved for status, never
   decoration.
3. **Each service has an identity hue** so a user can tell Playback from
   Services without reading.

## Our look

**DECIDED (owner accepted, 2026-10-11), drawn from what the app already does.** The style in
one line: **8-bit meets film flare, with a modern touch.** The app is a calm,
modern control surface (Geist, clean panels, clear status colours) with two
signatures that make it ours: a little pixel-art character and a warm,
cinematic glow. Both are accents. Neither takes over.

### 8-bit: where it lives (system moments only)
- **Yes:** the startup/loading bar (its fill KEEPS the pixel stripes, decided:
  the stripes must not be flattened or stretched) and the phone spinner (stepped pixel
  stripes, hard 5px offset shadow, `image-rendering: pixelated`, animation in
  `steps()`), and the outlined arcade wordmark (StagePilot Logo with its 1px
  black outline), and the generic pixel-art service icons
  (checklist, monitor, audio tracks, lighting).
- **Rule:** pixel styling marks *the app doing something itself* (starting,
  loading, connecting, failing). It is blue for progress, red when slow, red
  when failed.
- **No:** pixel fonts for body text or labels, pixelated borders or corners on
  panels and buttons, retro "game" wording, 8-bit sound or icons everywhere.
  Controls stay modern: 8px corners, Geist, clean borders.

### Film flare: where it lives (light and warmth)
- **Yes:** the photo background, the warm brown-red soft shadow lit from the
  upper left, the red wash and left rail on the live item, and the faint
  warm glow behind the timer.
- **Rule:** the red light means *live or selected*. Glow is earned: it shows on
  at most one live item per view. Everything else is quiet slate.
- **No:** glow, neon outlines or gradients on ordinary panels, buttons or text;
  cool grey or pure black shadows; extra film effects (grain overlays, lens
  flares, vignettes) stacked on top of the photo.

### Modern: the baseline
Geist (bundled), 8px/12px corners, the three widget borders, tokens only,
44/36px targets, plain verb-and-object labels, visible status with words.

### Avoid (generic defaults that would blur our identity)
Purple-to-blue gradients; glassmorphism on every panel (blur only on the
header and popovers, as today); the same icon + heading + text card repeated
for everything; a pulsing dot on things that are not live; invented numbers or
metrics; decorative animation that moves or delays a control; gradient text;
emoji as icons; nested rounded boxes that add no hierarchy; and any new colour
that is not a status, a service, or the primary red.

### Check before shipping a screen
1. **Does the 8-bit or film touch appear only where the rules above allow?**
2. **Remove each decorative layer in your head.** If meaning, state or action
   survive, delete it.
3. Run `audit-ai-design-slop` (evidence-based review of generic patterns and
   real UI defects) and `web-design-guidelines` (accessibility and UX review)
   on the changed files; run `design.md lint` on this file.

## Colors
- **Surfaces.** The page is `background` over the film-flare photo (decided: keep the photo). Cards sit on
  `panel` / `panel-strong` / `card`. Inputs, dialogs and toasts use
  `surface-950` (near black), so they read as wells.
- **Text ladder (decided).** Three colours: `text` (`#f8f4ec`, the warm
  off-white: headings, key figures, timers, text you type) > `text-body`
  (`#cbd5e1`, default copy) > `text-muted` (`#94a3b8`, labels, kickers, hints,
  placeholders). **`text-strong` is not a colour:** it is `text` set bolder
  (weight 700) for emphasis. The old cool-white `#f1f5f9` is gone. The old
  `text-quiet` (slate-500, 72 uses, only 3.4:1) is replaced by `text-muted`
  (7:1 or better on every surface).
- **Primary (`#c7231d`, deep red; replaced coral on 2026-10-11)** marks the live
  item (now-playing rail, current song row) and the layout-save action. (The
  open status card is NOT primary any more: it wears its own connection colour,
  see Connection colours.) Its family: `primary-hover` `#cd3934` (tint 10%),
  `primary-focus` `#dd7b77` (tint 40%, focus ring), `primary-active` `#b3201a`
  (shade 10%, pressed), `primary-text` `#e3918e` (tint 50%, for red used as
  text). The full 10% to 90% shade and tint scale is kept under
  `primary-scale` as a reference. **The red alone is 3.2:1 on panels, so it is
  for fills, rails, borders and dots (3:1 is enough for those); text and thin
  icons use `primary-text`.**
- **Red now has two jobs, so they are told apart by form (decided default):**
  primary is a *solid fill with white text* or a *rail*; errors are pale
  `danger-text` words with a red dot and icon; destructive confirms use
  `danger-solid` (red `#ff0000`) with a verb that says what is lost. A
  destructive button never uses `primary`, and `primary` never carries an error.
- **Text on fills (DECIDED rule): light text on dark fills, dark text on light
  fills.** Most of the app is dark, so light text stays the norm.
  - Light fills (the Services green, the Presentation orange): dark text (`on-light`, `#020617`). The Playback blue takes white (4.8:1), and so do the deep success green (7.8:1), Lighting and the StagePilot red.
  - **Identity fills (solid):** Services green 7.6:1 and Presentation orange 7.6:1 take dark text (`on-light`); Playback purple 9.2:1, Lighting pink 4.7:1 and the StagePilot red 5.7:1 take white (`on-primary`).
  - The **teal accent fill** `info`: dark text (`on-info`, 9.1:1); white would be 2.2:1.
  - The **primary red** is a mid-dark fill: **white text** (`on-primary`, 5.7:1).
  - Dark red fill (`danger-solid` `#e11d48`, 4.7:1 with white): white text
    (`on-danger`); it is the same pure red as the error colour (4.0:1).
  - Where an **error** colour needs dark text (a light red fill such as
    red-300/400), use dark **red** `on-danger-soft` `#4c0519`, never black.
  - **Minimum 4.5:1** for any text, and this file is linted for it.
- **One solid action colour (DECIDED): the primary red is the only primary button.**
  Every panel's main action (Save, Connect, Unlock, Update and Restart) is
  primary red with white text. Integration hues are for identity only: tinted
  borders, tinted secondary buttons, panel accents. Consequences in code:
  - "Save general settings" (StagePilot panel, red) and the MIDI save (magenta) become
    the primary red. Red now also means primary, so the form rule above (solid + white text = primary; pale text + dot = error) keeps them apart.
  - "Update and Restart" (sky solid) and the two PIN/unlock buttons (sky) become primary red.
  - "Use <plan>" in the Services plan picker (warning solid) becomes a
    tinted Services (green) button. The warning red is only for warnings.
  - `danger-solid` stays for destructive confirms only.
- **Status colours** are fixed app-wide: `success` = connected/running/ready,
  `warning` = connecting/delayed/needs attention, `danger` = error or
  destructive, `idle` = disconnected/idle. Use `*-text` for words and
  `*-soft` for dots on dark panels. `info` is the **secondary accent teal
  `#1dc1c7`** (owner, 2026-10-12): information, "what comes next", and
  non-disruptive actions; it is never a state.
- **Connection colours (decided 2026-10-12)** tint a connection's status card
  icon, setup panel, secondary buttons and badges. **Services is green
  `#6bb03c`, Presentation is orange `#fe7821`, Playback is blue
  `#4b73b0`, Lighting is pink-red `#d53367`, and the StagePilot (backend) card
  and panel are `#c7231d`.** MIDI is configured under Playback and has no colour
  of its own. Each has three roles: the base (solid fills, rails), `-soft` (dots,
  borders and 10-30% tints on dark surfaces) and `-text` (small text, 7:1).
  Playback is a clearly blue `#4b73b0` (it began as the owner's slate-blue
  `#3a4e66`; the first lift to `#56677b` still read as grey), 3.8:1 on dark
  surfaces, so one colour serves as fill, dot, border and tint. **The open status card wears its connection's colour (decided
  2026-10-13):** 70% border, a 10% wash fading out by 44%, and a 3px left bar in
  the base colour. The StagePilot card is the one exception: it is the same red as
  the live item, so it keeps the neutral card with a 1px `#c7231d` border and no wash
  or bar (see Colour collisions). Never reuse these for status. **Accent inside a configuration panel (decided
  2026-10-13):** inside a connection's configuration panel its colour is the only
  accent. The panel's solid action buttons (*Save configuration*, *Scan*, *Scan
  again*, *Connect*) are filled with it, tinted buttons, links, checkboxes and
  badges use it, a "connected" or "saved" message uses it instead of green or
  teal, the Save bar is a deep tint of it (the accent at 20% over the panel),
  and the panel wash comes from it. Label colour: dark on Services and
  Presentation, white on the rest. **Exceptions that keep their own colour:**
  errors (pure red), warnings (pure red, like errors), idle grey, the destructive buttons,
  and everything outside the panel (the header, dialogs, toasts). **Advanced** parts of a service's setup use a muted version of
  the service colour (`advanced-*`, the hue mixed half-way with `text-muted`)
  so they read as secondary. Decided: mixed 30% hue / 70% grey so the difference from the main colour is easy to see.
- **Dashboard names (decided): generic, not brand names.** The dashboard says
  **Services** and **Presentation**, not Planning Center and ProPresenter, so
  the app reads as usable with many tools and avoids implying an endorsement.
  A brand name appears only where the user is signing in to, or an API is
  specific to, that company's product (for example the Planning Center OAuth
  screen). Today the code still uses the brand names in the setup checklist,
  accessible labels and setup panels; those move to the generic names in the
  migration.
- **Live controls (decided: keep four colours).** The Manual controls widget
  uses colour to show how much a press changes a running show: **green**
  (Start next, Restart current: moves the show forward), **orange**
  (Previous: steps back), **teal accent** (Reload service plan: nothing on stage changes),
  **red** (Stop timer, Reset position: halts or rewinds). They are outlined
  at rest and fill on hover, with dark text on green and orange fills, white
  on blue and red. The red hover fill is the pure red (4.0:1 with white). These do not follow the
  primary-only rule on purpose: they are a severity scale, not "actions".
- **Widget borders (decided: three)** are white at 6/10/18% opacity so they sit right on any
  surface. `border` (10%) is the default. The old 5% and 7% merged into `border-subtle`; the old 15% and 20% (the least used) merged into `border-medium`.
- The **retro loading bar** is blue pixel-art, deliberately not red
  (decided: keep). Use it lightly; its contrast against the red/slate UI is
  the point. **Decided: remove the white "scan" bar that sweeps along its
  leading edge** (`.loading-progress-scan` in `index.css`, a `<span>` in
  `App.tsx`). About 20 hardcoded hex values in `index.css`.

## Typography
- **Geist, bundled** (decided): self-hosted variable font
  (`@fontsource-variable/geist`, OFL-1.1), no CDN, so the desktop app looks the
  same offline and on every OS. Geist replaced Inter by owner decision on
  2026-10-11. It is safe because it keeps tabular numbers (`tnum`), has the same
  100-900 weight axis and the same Latin glyph coverage, and its licence permits
  bundling (notice in `THIRD_PARTY_NOTICES.md`).
- **StagePilot Logo** (`assets/logo-font.ttf`) is for the wordmark only, with a
  1px black outline (`.font-brand`).
- **Sizes (decided):** `text-sm` 14px (197 uses), `text-xs` 12px (134),
  `text-lg` (13), `text-base` (5), `text-xl` (3), `text-2xl` (3), `text-4xl`
  (2), and **one micro size, 11px**, replacing `0.6rem` / `0.65rem` / `0.68rem`.
- **Weights:** 600 labels and buttons (90), 700 headings and badges (84),
  900 small chips (13), 800 kickers.
- **Timer:** light-weight **monospace**, tabular numerals,
  `clamp(3rem, 5vw, 5.4rem)`, tracking `-0.06em`. **Song title** (live
  panel heading) is a separate style: Geist 900,
  `clamp(2rem, 3.2vw, 3.65rem)`, tracking `-0.045em`. These are two different
  elements, not a conflict.
- **Tracking (decided): three steps.** `0.05em` (Tailwind `tracking-wider`,
  uppercase labels, 56 uses), `0.18em` (kickers; absorbs the 0.14 / 0.2 / 0.22
  one-offs), `0.35em` (display).
- **Placeholders (decided):** `text-muted`. No placeholder style exists today.

## Layout
- 4px base unit. Most common: padding `px-3` (118) / `py-2` (85) / `py-2.5`
  (45); gaps `gap-3` (49) / `gap-2` (35); panel padding `p-4` (18) / `p-5` (12).
- Page capped at 1700px and centred; minimum supported width 320px.
- **Breakpoints:** Tailwind defaults. `sm:` 35 uses, `lg:` 12, `xl:` 10,
  `md:` 7. `lg` (1024px) is the phone/tablet vs desktop split. Components that
  resize inside the dashboard use **container queries** (`@container`,
  8.5/10/18/36/46rem), not viewport queries.
- Dashboard is a drag/resize grid (GridStack) with 5px gutters; see
  `docs/dashboard-layout.md`.
- **Tap targets (decided):** **44px** for live controls (timer, cue and manual
  controls, dialog actions) and for every input; **36px** for all other
  buttons. Today only 5 files use `min-h-11`; most buttons are about 38px, so
  the 36px tier mostly matches what exists.
- **Stacking order** (`z-index`):

  | Layer | z | Used by |
  | --- | --- | --- |
  | In-widget controls | 10 | widget handles, header |
  | Menus | 20 | Services plan menu |
  | Header chip | 40 | system-status chip |
  | Popovers, confirm dialogs | 50 | discard-changes, Playback discover confirm, Services length preview, tooltips |
  | Title bar | 100 | desktop title bar (sticky) |
  | Update dialog | 100 | same level as the title bar (decided: stays) |
  | Fatal toasts | 110 | crash-loop and global-error toasts |
  | Error boundary | 120 | full-screen crash screen |

  Above the title bar: both toasts and the error boundary. Level with it: the
  update dialog. Below it: the three confirm dialogs at 50 and all popovers.
  **Decided to keep as is.** The title bar holds the window's drag area and
  close/minimise controls; leaving it reachable under a routine confirm means
  a user is never trapped in a dialog, which is how native apps behave. Only
  fatal states (crash toasts, error screen) and the update dialog, which is
  about the app itself, cover it.

## Elevation & Depth
- **Default shadow (decided; the old "printed" offset shadow is deleted):** soft, layered and warm. Light
  comes from the upper-left, like the film-flare photo, so shadows fall down
  and to the right in a brown-red (`rgb(22 13 12)`), not cool black, plus a
  faint warm inner highlight on the top-left edge. Value is in the
  `elevation.default` token. Rendered over the real background next to the old
  one: it separates well and looks more physical; its weakest spot is the
  right edge over the darkest part of the image, which the 10% white border
  covers.
- `elevation.overlay` (dialogs, toasts, popovers) is a stronger version of
  the same idea and replaces `shadow-2xl`. **Decided** (owner approved after review).
- The old "printed" offset shadow is deleted (decided); `elevation.default` is the only panel shadow.
- **Strength:** `elevation.default` is level 1 until you pick a stronger level from `elevation-options` (5 levels, each darker). The dialog and toast shadow scales with it.
- Live panel: 5px red left rail with a faint glow. Hover on cards raises
  border opacity, not size.
- Dragging a widget: opacity ~0.78 plus a large drop shadow.
- Glass: `backdrop-blur-xl` (5 uses) on header/popover chrome.

## Shapes
- **8px** (`rounded-lg`, 157 uses) is the control radius, and the bare
  `rounded` (4px, 12 uses) becomes 8px (decided).
- **12px** for **all** panels and cards (decided; setup panels move from 16px).
- **16px** for toasts only. **Full** for pills and dots.

### Borders inside panels (decided)
A panel is one box. Inside it, **only these get their own border**: form
fields (inputs, selects, text areas), the Advanced disclosure row, the Activity
row, and the save bar. Everything else is grouped with spacing, a heading, or a
single thin divider (`border-subtle`), never another bordered box.
- A group of related fields is a heading plus spacing, not a bordered card.
- Status cards, live controls and dialogs are their own surfaces, not boxes
  inside a panel.
- A thin divider separates sections of one panel; do not put a divider and a
  box around the same thing.
- Nested boxes only when the inner one is a different kind of thing the user can
  act on (for example the held-change field marker or an alert).
Reason: the design check flagged "card inside card" repeatedly; extra borders
add noise without adding hierarchy.

## Motion

- Micro-interactions 140ms; live reveals 420ms; status colour 180ms;
  expand/collapse 500ms; status-card morph 275ms linear; loading bar steps.
  Easing `cubic-bezier(.2,.8,.2,1)`.
- **Everything must respect `prefers-reduced-motion`** (a global override
  already exists in `index.css`; keep it).

## Components
- **Panel (one recipe, two variants; decided):** `panel` fill, widget border
  border, 12px radius, `elevation.default`. Variant `panel-setup` adds the
  red-to-clear 120° wash and 20px padding. The `!important` overrides on
  `.setup-panel` go away when they merge.
- **Status card:** icon tile + title + status word + detail line. No status dot
  (removed 2026-10-13). The word is **capitalized** (CONNECTED, CONNECTING, NOT
  CONNECTED, ERROR), semibold, and grows with the card: it is sized from the card
  width so the longest phrase, DISCONNECTED, always fits beside the icon, from
  11px on a 181px card up to 18px on a 266px one. In compact mode (1000px and
  below) the word sits under the icon on ONE line, always (owner, 2026-10-13): it never
  wraps, never ellipsizes and never overlaps the icon or a neighbour. Its size is the
  card width minus padding divided by 8.3, at most 11px, so DISCONNECTED (the widest
  phrase) fits; that is about 10px on tablets, 8.6px at 480px and 6.5px at 390px, and
  the owner-accepted exception to the 11px floor. Collapses to an icon-only tile in compact mode.
- **Live (now-playing) panel:** red rail, 135° red wash fading out by 42%,
  oversize tabular timer.
- **Buttons.** Height: 44px for live controls, 36px for everything else.
  - *Solid* (primary): `primary` red, white text; hover `primary-hover`, pressed `primary-active`, focus ring `primary-focus`.
  - *Tinted* (most of the app): `hue @ 10%` fill, `hue @ 20-30%` border, light
    `hue-200` text, `hue @ 20%` on hover.
  - Disabled: 40% opacity (a few use 50%; normalise to 40%), `cursor-not-allowed`.
  - **Label shadow (decided):** light text on a dark or tinted button gets a small very-dark-grey shadow (`text-shadow.on-dark-text`). **Dark text gets no shadow.** A light shadow under dark text was tried and rejected by the owner because it looks bad (it reads as a blurry halo). If dark labels ever need more punch, change the fill or the text colour, not add a shadow.
  - Primary = red (decided). Secondary = tinted in the panel's hue.
- **Inputs (decided):** `surface-950`, widget border, 8px radius, 44px
  tall, `text-muted` placeholder. Native checkboxes (7), `accent-rose-500` on 3.
- **Badges/pills:** full radius, uppercase, 700 weight, hue at 10-15% fill with
  matching light text.
- **Inline alerts:** 8px radius, hue at 10% fill, 20-30% border, `role="alert"`
  (11) or `role="status"` (7).
- **Dialogs:** `fixed inset-0`, `scrim`, centred `surface-950` card, 12px
  radius, `elevation.overlay`. Actions are 44px.
- **Toasts:** fixed bottom corner (left = global error, right = crash-loop),
  16px radius, red border, `elevation.overlay`.
- **Focus:** 2px `primary-focus` outline, 3px offset, global
  (`:focus-visible`). Never remove.
- **Icons:** no icon library; service icons are PNGs in `assets/`.
- **Scrollbar:** 9px, `scrollbar-track`, slate-700 (`#334155`) thumb.

## Patterns

Everything here is **DECIDED** (owner accepted the recommendations on 2026-10-11, with the specific answers marked **(decided)** below). Principle behind all of it: **anyone can use
the app with no training.** No hunting and no manuals: each control is where
the user expects it, labels say exactly what will happen, and the choices
offered are the ones that make sense at that moment. Nothing is kept just
because the current code does it.

### Responsive behaviour
- The status cards switch design at **1000px** viewport width (not just
  shrink): **Wide** = icon tile + title + status dot + status word + detail
  line; **Compact** = icon tile only, with the status dot and word beneath it
  (title and detail stay in the accessible label and the opened panel). Five
  cards always fit in one row.
- `lg` (1024px) is the phone/tablet vs desktop split for everything else.
- Phone layout is a first-class design, not a squeezed desktop: single column,
  44px targets everywhere, dialogs full-width with actions stacked.

### Startup and sign-in screens
- **Desktop startup:** centred wordmark (`brand-hero`), the blue pixel loading
  **bar**, one headline, one detail line.
- **Remote / phone loading (decided): a circular pixel spinner _instead of_
  the bar**, with the same headline and detail text. (Today the code shows the
  spinner **and** the bar together on phones: that is a bug against this rule.)
  The spinner follows the bar's states: blue = loading, red = taking longer
  than usual, red and stopped = failed.
- The white lead-in line on the bar is removed.
- **Remote sign-in (PIN or login):** a single centred card with the wordmark,
  one sentence of purpose, the field(s), and **one primary red button**. Errors appear
  under the field, in `danger-text`, announced to screen readers.

### Choice fields and menus
- **Choosing one value from a list = a styled native select** (input look,
  44px, chevron on the right). Works on phones, keyboard and screen readers for
  free. There are 15 today in 3 different looks; they become one.
- **Choosing an action = an action menu** (button with a chevron that opens a
  card): `surface-950`, `border-medium`, 8px radius, `elevation.overlay`,
  items 44px tall, current item checked, arrow keys + Escape. Only for several
  variants of one action, such as "Update service plan".

### Connection method (decided: MIDI is a method, not a brand)
A connection may be reachable in more than one way. The ways are shown as a
**Connection method** select at the top of General configuration, so alternates are where the
user looks, not hidden behind a toggle:
- **Playback:** `Playback API` (main) or `MIDI` (alternate).
- **Lighting:** `MIDI` (the main method today; a method select appears only when
  a second method exists).
- When only one method exists, show it as a line of text ("Sends cues by MIDI"),
  not a select with one item.
Changing the method swaps the fields beneath it. MIDI is a protocol, not a
company, so the word stays in the UI.

### Collapsible sections ("Advanced")
- **One pattern for every collapsed section** (today there are five: a ▸ text
  button, `<details>`, a checkbox row, a bordered button, a plain toggle).
- A full-width 44px row: chevron + label on the left, and on the right **a
  one-line summary of what is inside** when closed (for example
  `Port 8080 · Channel 1`), so a user rarely has to open it.
- Colour: the service's muted `advanced-*` tone for chevron and label; the open
  area gets a 3px left bar in the same tone. General configuration is never inside a collapsed
  section; Advanced always comes after General configuration. Motion: 500ms expand, honouring
  reduced motion.

### Telling expandable things apart (decided)
In beta 31, "Connect a specific computer", "Recent activity" and
"Alternate connection: MIDI settings" are three near-identical grey rows that
do three different jobs. Rule: **a row's shape says what kind of thing it is.**
1. **A way of connecting (Playback API vs MIDI) is never a collapsed row, and it is never a
   dropdown.** It is the **Connection method** control at the top of the panel, built once as
   `ConnectionMethod` and **used by every connection that has more than one way to connect, now
   and in future** (decided 2026-10-14, replacing the earlier select): two or more options
   side by side, each a bold name over a tiny uppercase **PRIMARY** or **ALTERNATE**
   (`Playback API / PRIMARY`, `MIDI / ALTERNATE`). **Whichever option is chosen, primary or
   alternate, wears the connection's tint** (an outline and a 45% fill; the primary red outside a
   panel), so the toggle itself says which is active. **There is no "how to switch back"
   notice**: the toggle is that. The header badge still says `Connected · MIDI` when the
   alternate is in use. It is a radio group (arrow keys, one label, 44px targets). Do not build a
   second method picker; add options to this one. A change is held until Save configuration.
2. **Extra options = Advanced:** the one tinted row with a one-line summary.
3. **Read-only information (Recent activity) = Activity:** flat and neutral, with a
   dashed top rule and a neutral grey count pill (`12 events`) (the pill takes the connection colour inside a connection panel), no fill, no box.
4. **A one-off value (a specific computer address) is a choice in the field it
   belongs to** ("Other computer…" at the end of the computer select), not a
   separate collapsed row.
Words: "Alternate connection: MIDI settings" becomes **MIDI** under
**Connection method**, and "Main" / "Alternate" are labels on the options.

### Vocabulary (one word for one thing, everywhere)
| Say | Never say | Meaning |
| --- | --- | --- |
| **Connection** | integration, plugin, connector | one of the things StagePilot talks to |
| **Services** | Planning Center (unless signing in to it) | the connection that supplies the service plan |
| **Presentation** | ProPresenter (unless an API screen needs it) | the connection that shows the countdown |
| **Playback** | MultiTracks | the connection that tells StagePilot a song started |
| **Lighting** | Lighting (until 2026-10-12), Lightkey | the connection that receives cues. Renamed from "Lighting" by the owner on 2026-10-12. Describe its output as **MIDI Output to Lighting Controller** (decided) |
| **MIDI** | (stays) | a connection method: alternate for Playback, main for Lighting |
| **Configuration** | settings, setup, production setup | what you edit for a connection (decided) |
| **General configuration / Advanced** | alternate, specific, manual | the two parts of Configuration |
| **StagePilot** | backend, local API, plugin health | the fifth status card; its panel holds the app's own status and all app-wide configuration |
| **Service plan** | plan | the day's list of songs (decided) |
| **Cue** | pulse | a message sent to Lighting (decided) |
| **Note** | pulse | the MIDI note a cue uses, and how long it lasts. Use **Note** wherever the app said "pulse": "Note length (ms)", "Test cue" (decided 2026-10-11) |
| **People** | remote users | who can sign in remotely |
| **Connected / Connecting / Disconnected / Error** | disconnected | the four status words (sentence case in text, UPPERCASE in badges) |

### Labels and buttons
- Button labels are **verb + object**: `Save configuration`, `Scan for devices`,
  `Test connection`, `Discard changes`. Never `OK`, `Submit`, `Yes`.
- Sentence case everywhere, except badges and kickers (uppercase).
- Destructive buttons name what is lost (`Delete layout`); the safe option is
  on the left.
- A control is **disabled with a reason shown nearby**, never silently greyed.

### The loading screen hands over to the header (decided 2026-10-16)
The big StagePilot logo on the loading screen is not replaced by the dashboard: the dashboard's own header logo starts at the loading logo's centre and size and glides to its place in the header (720ms), while the rest of the dashboard is held back (hidden from the first frame), then fades in (400ms). `startup/logoLanding.ts` does it with the `translate` and `scale` properties, so nothing is cloned and the logo's own transform is untouched. People who prefer reduced motion get the dashboard at once, and a page that did not come from the loading screen (a reload of the loaded app, a test) never plays it. The Backend panel's "Timezone, logging and port changes take effect after a backend restart." now sits at the bottom of the Advanced section, under the four settings it is about.

### The dashboard header (decided 2026-10-15)
The logo is **32px in from the left edge of the background image** (decided 2026-10-15), at every width. The image is 1700px wide and centred, so on a window wider than it the logo moves with the image and never lands in the black beside it; on a narrower window the image fills the window. The status checks at the bottom of Services (and any panel using `.status-checks`) are four across while their words fit (the panel is 30rem or wider), then two, then one; they follow the panel width, not the window.

One row at every window width, with the **logo first and never moving**: no grid, nothing re-flows around it. Everything else is one status pill (`HeaderStatusChip`) in a slot to its right:
- **The pill is the notification.** The pill is always the widgets' dark surface (`surface-950`) with a thin outline and pixel text in the status colour (decided 2026-10-15; it was a solid fill before): at rest Ready is green (`success-soft` outline, `success-text` words) and Check system / Error is red (`danger-edge` outline, `danger-pill` words: the darkest reds that still pass 3:1 and 4.5:1 on the dark surface). When a message arrives the same pill grows to the message's size, keeps the dark surface and takes the blue of the Previous button (`live-back-soft` outline, `live-back-text` words; not the light teal `info`) for an info message, or the same red for an error, and swaps its words; when the message ends it shrinks back. Both the status words and the message use the bold Geist Pixel Square (`font-pixel`).
- **A new message shows right away**, replacing the one on screen (the words slide in and the pill resizes if the new text is a different length). Messages that arrive together play in turn. Each lasts 5 seconds at most.
- **Too small for even the pill:** the pill disappears (never a dot) and the message appears just under the header on a solid surface.
- The message is announced through a visually hidden live region; the drawn copy is `aria-hidden`. Reduced motion turns the animation off.

### The system check list (the pill's hover panel, decided 2026-10-15)
- It lists what the show cannot run without: Services, Service plan, Song durations, Playback, Presentation (and its timer) and StagePilot. **Lighting is not in it**: it is optional and has its own card, so a Lighting output going away never turns the pill to Check system.
- The Playback entry names the saved connection method: "Playback API connected" or "Playback MIDI disconnected".
- The panel sits above the dashboard widgets (the header has its own stacking level), and every word in it matches the cards.

### Scrolling boxes (decided 2026-10-15)

A box that scrolls on its own (the Lighting cue list, the timeline) is drawn as an obvious region so people know there is more to see:
- Use `ScrollRegion` (`components/ui/ScrollRegion.tsx`). It draws a 1px `ink-muted` border, rounded corners and `surface-950` fill around the scroller.
- A light fade (`ink-muted` to transparent, 18px) lies **on top of the content, inputs included**, along any edge that has more to reach, and is hidden at the ends. It is an overlay that ignores the pointer. A dark shadow is not used: it cannot be seen on `surface-950`.
- The scroller stays focusable and named when it has content (`role="region"`, `tabIndex=0`, an `aria-label`). An empty list is not framed.
- Collapsing a group of controls that people may not use (Remote access) uses `Disclosure variant="activity"` with its open state saved by `usePersistedFlag`, starting open.
- A **major** optional section (the Lighting song timeline) uses `Disclosure variant="feature"` (decided 2026-10-15): a card with a 5px rail, a 70% border and a soft wash in the connection's colour that fades to the right; a two-line header (bold title, then a detail line that is shown open or closed and says what is inside, for example `16 cues on 2 of 4 songs · 2 pads · Battle Belongs 04:41`); a small tag at the right (the active method); and the body under a rule. Everything in the section, including its Test cue controls, folds away with it. It sits next to the Save bar, which is a flat full tint with no rail, so the two read as different things in one family.

### Panels read like the style guide (decided 2026-10-14)
The owner preferred the guide's StagePilot and Playback panels to the app's. The app now follows them:
- **Header:** the kicker `Connection configuration` (11px, 800, uppercase, 0.18em, muted); the **title is
  larger (20px) and bold, in the connection's own colour** (Services, Presentation, Playback and Lighting
  use their `-soft` colour, which is 3:1 or more for large bold text; **StagePilot uses its pale `-text`
  tint**, because its red is the primary red at only 3.2:1); the one-sentence description in body grey.
- **Section names** (Status, General configuration, People, Advanced, Connection method) are **kickers**:
  11px, uppercase, tracked, muted, not bold sentences. They are still real headings. One shared
  `SectionLabel` draws them.
- **StagePilot Status facts** are **small recessed bordered cards** (`border-subtle`, a 40% darker well,
  8px radius), four across, each a kicker label over a bold value. This is the one place Status facts
  are boxed, because each is a separate value to scan.
- **Recent activity** is **flat**: no box and no fill, a dashed rule above it, a neutral grey count pill
  (`12 events`). Advanced stays the one tinted, bordered row, so the two still read as different jobs.

### StagePilot panel layout (decided 2026-10-14, from the guide the owner liked)
Header, then **Status**: three cards (**Version**, **Running since**, **Connections: N of 5
connected**) and one quiet **Connection details** row beneath them holding the API endpoint, the
WebSocket and the state revision (it is for support, not for operating). **General configuration**:
**Timezone** and **Network access** are full-width selects (`This computer only` / `Other devices on
this local network`), then the PIN. **People**: one sentence, then Remote Access; once it is on, the signed-in people are listed and **Add a person** opens the email, password and role fields only when pressed (Cancel folds them away again; the very first Operator keeps its form open because it is the only step). **Advanced** is a
collapsed row with a one-line summary (`Port 8765 · Logging info`); it holds Server port, Log level
and Release channel, opens by itself when its port is invalid, and says `N changes waiting` while
one is held. The save bar follows; support logs sit below it.

### Configuration panel anatomy (every connection looks the same)
1. **Header:** kicker `Connection configuration` (muted), title = the
   connection name, status badge, close button (44px).
2. **One sentence** saying what this connection does for the user.
3. **Connection method** (when more than one), then **General configuration**, ordered by how
   often they change.
4. **Advanced** (collapsed, see above).
5. **Save bar, always last and always in the same place** (see below).

**Narrow screens (320px, found when the guide was checked):** the header's
status badge and close button are 160px + 44px and don't fit beside the title.
Below 480px the header wraps: the title takes the first row with the 44px close
button at the right; the status badge moves to its own row beneath the title.
The close button never shrinks below 44px. The save bar's buttons stack full
width below 480px.

### Saving (decided: save as you go, plus an always-visible Save configuration button)
- **Default (option A, decided 2026-10-11): every field is explicit-save.** A
  change is held, marked, and saved only by `Save configuration`. Reason: today
  saving any field in a panel reconnects or restarts something that a running
  show may be using. True save-as-you-go (option C) is planned for later, once
  the backend can apply non-disruptive fields without reconnecting; until then
  nothing saves on change, on leaving a field, or on Enter.
- **The `Save configuration` button is always visible** at the bottom of every
  connection panel, for every kind of change, including ones that already saved
  by themselves. Reason: an operator can't be expected to know which controls
  auto-apply, and a missing button looks like a glitch. Pressing it when
  everything is already saved is harmless and just confirms (`All changes saved
  at 9:41 AM`). It is **never hidden**.
- **The save bar shows one of five states** (same wording in every panel):
  1. **Nothing changed:** `All changes saved`, button dimmed with the reason
     beside it ("No changes to save").
  2. **Saved automatically:** green check, `Saved automatically at 9:41 AM`,
     button ready to press to confirm.
  3. **Saving:** `Saving…`.
  4. **Waiting (held changes):** red, `2 changes waiting. Not saved yet.`
     with `Discard` and a primary red **`Save configuration`**. Held changes only take
     effect when this is pressed, **whether or not a show is running** (decided).
  5. **Couldn't save:** red, what failed and `Try again`; the field keeps its
     value.
- **Held until `Save configuration` (decided):** every field in every
  connection panel and in the StagePilot panel.
  - **Services:** service type, service plan choice, `Reload service plan`.
  - **Presentation:** which timer or target receives the countdown.
  - **Playback:** connection method, which computer, velocities.
  - **Lighting:** the **MIDI network / output, the MIDI channel and the note
    pulse** (any Lighting save reconfigures the plugin: it cancels the running
    lighting timeline and reopens the MIDI port).
  - **StagePilot:** port, network access, timezone, log level, release channel,
    PIN requirement and the new PIN (the PIN is never saved per keystroke).
- **Saved immediately, even during a show (decided):** only Lighting cue list
  edits (cue notes, velocities, labels, elapsed times, add and remove), which
  already use their own endpoints, because operators need to add cues quickly
  during a show. Tests and display preferences (note names vs numbers) change no
  saved configuration.
- A held field is marked with a red dot and `Not saved yet` until saved, so
  it's clear which controls wait and which don't.
- On phones the save bar is sticky at the bottom of the screen.
- The StagePilot panel uses the same bar.

### The StagePilot panel (decided)
The fifth card stays in the connections row, as the quick look at the app's
own health, and its panel replaces a separate app-settings page. **All
app-wide configuration that is not for one connection lives here.**
Sections, in order: **Status** (version, running state, connection health:
read-only), **General configuration** (timezone, network access, PIN), **People** (remote
access and who can sign in), **Advanced** (port, logging, diagnostics).
New app-wide options go here, never into a connection.

### Form fields
- Label above the field (`text-strong`), hint under it in `text-muted`, error
  under that in `danger-text` with a short fix ("Enter a number from 1 to 16").
- Field height 44px; the label never sits to the left of a field.
- Validate when the user leaves the field, not on every keystroke.

### Feedback
- **Inline alert** for anything about the current panel (red, error
  red, success green), under the thing it is about.
- **Toast** only for things the user did not just cause (crash, update).
- **Empty state:** one sentence saying what to do next plus one button.
- **Progress in a panel** uses the blue pixel bar; on phones, the spinner.

### Build rules (from the Web Interface Guidelines and hardening checks)
These are the mechanical rules every component must follow. Each one came from
a skill check against beta 31 or against this guide.
- **Motion:** list the transitioned properties (never `transition: all`);
  animate `transform` and `opacity` only (the loader fill should scale, not
  animate `width`); honour `prefers-reduced-motion` with a calm alternative that
  still shows the state change; animations must not block input.
- **Touch:** `touch-action: manipulation` on buttons and controls;
  `overscroll-behavior: contain` inside dialogs, panels and menus; 44px targets
  on `pointer: coarse`; nothing important only on hover.
- **Text:** `…` (single character) in placeholders and loading text, and
  placeholders show an example ("For example, Sunday Morning…"); `tabular-nums`
  on every timer, clock, count and duration; non-breaking space between a number
  and its unit; 11px is the smallest text anywhere (no 10px), with ONE owner-decided
  exception: the status word on compact status cards (1000px and below).
- **Forms:** every field has a visible label above it; correct `type`,
  `inputmode` and `autocomplete`; never block paste; validate on blur and keep
  the user's input on error; focus the first error after a failed save; the Save
  button is disabled only while a save is in flight (no double submit).
- **Semantics (keep what exists; hover and touch come first):** `<button>` for actions, `<a>` for navigation; icon-only buttons
  have an `aria-label`; async results (saved, failed, connecting) are announced
  with `aria-live="polite"`; status is never colour alone; headings go in order
  (no skipped levels).
- **Long and odd content:** text containers use `min-w-0` with truncate or wrap;
  labels allow 40% longer translations (no fixed widths on text); long names
  (songs, devices, computers) truncate with the full text available on focus or
  tap; empty lists show an empty state, never a blank gap; layouts survive 200%
  zoom and 320px width.
- **Destructive or live-affecting actions** need a confirm or an undo window.
- **Images:** explicit width and height; decorative images `alt=""`.

### Rules for adding anything new
1. Does it belong to a connection? Then it goes in that connection's
   Configuration, General configuration or Advanced. If not, it goes in the StagePilot panel.
2. Could it change everything a running show sends or reads? Then it is a held change, saved with `Save configuration`. Everything else saves as you go.
3. Use an existing pattern from this file. If none fits, add the new pattern
   here first, then build it.
4. Name it from the Vocabulary table. Add any new word to the table.
5. It must work at 320px wide, with mouse hover and with touch (the priority). Do not regress the keyboard and screen-reader support that exists today, but do not gold-plate it.
6. A new colour is never needed: status colours for state, the service colour
   for identity, the primary red for the one primary action.

## Do's and Don'ts
- Do use status colours only for state, integration hues only for identity.
- Do put light text on dark fills and dark text on light fills; check any new
  pair at 4.5:1 (use the linter).
- Do keep error colours reddish, including text on a light error fill.
- Do put new colours here first, then use the token. No raw hex in components.
- Do pair state colour with a word or icon (status cards already do).
- Don't use the primary red for decoration, for text (use `primary-text`), or for more than one "live" item per view.
- Don't add radii, font sizes, shadows or z-indexes not listed here.
- Don't put a bordered box inside a panel except fields, the Advanced / Activity
  rows and the save bar (see Borders inside panels).

## How this drives the app

Status (final audit, 2026-10-11): the three systems below are one system once
the Phase 5 and 6 PRs (#132 to #142) are merged. Merged at the final audit:
#132, #137, #138, #139, #142. Still draft: #133 to #136, #140, #141. The table records what
main had before them and where each went; the plan steps after it are done
except where noted.

| Layer | Where | Reach |
| --- | --- | --- |
| `--sp-*` CSS variables | `index.css` `:root` | deleted by PR #139 (merged) |
| `stage-*` Tailwind colours | `tailwind.config.js` | deleted in PR #139 |
| Stock Tailwind palette | components | none left in `src` non-test files (checked at the final audit; guard in PR #142) |

Wiring plan (each step was a small PR; status at the final audit):

1. `npx @google/design.md export` to generate the Tailwind theme and CSS
   variables from this file; replace both `:root` and `stage-*` with it. Done
   (plumbing in Phase 2; the old `--sp-*` and `stage-*` are deleted in PR #139).
2. Add semantic Tailwind names (`text-muted`, `bg-success`, `border`)
   and migrate components screen by screen; add a CI grep that blocks new
   raw hex and new stock-palette colours. Done: the migration landed in
   Phases 3 to 4 and the tokens-only guard over all of `src` is PR #142.
3. Add `design.md lint` to CI and a line in `CONTRIBUTING.md`. Done in PR #142 (CI step; the
   `CONTRIBUTING.md` line is in the same PR).

### Audit ledger: discrepancies found and how each was handled

| # | Finding | Handling |
| --- | --- | --- |
| 1 | Draft v1 said timer is Inter 900 (historical), 4rem. Real: mono, weight 300, clamp(3rem, 5vw, 5.4rem) | Fixed in tokens |
| 2 | Draft v1 listed `brand` at 1.5rem; real sizes are 4.05rem header and up to 11.25rem splash | Fixed; added `brand-hero` |
| 3 | Draft v1 claimed text pairs pass AA. `text-muted` (slate-500) is 3.4-4.2:1; `danger` as text 4.4:1; white on coral 3.0, rose-500 3.7, fuchsia-500 3.5 | `text-muted` CHANGED to `#7b8ba1`; others documented as rules |
| 4 | `--sp-*` vs rendered values differ (success `#42dda4` vs `#34d399`, warning `#f6c453` vs `#fbbf24`, info `#7dd3fc` vs `#38bdf8`, muted `#9aa9bb` vs `#94a3b8`, quiet `#718096`) | Rendered values used; `--sp-*` values listed as OPEN Q1 |
| 5 | `stage-800` / `stage-700` defined but never used; `stage-900` only in scrollbar CSS | Decided: delete both unused; `stage-850` -> `card`, `stage-900` -> `scrollbar-track` |
| 6 | Missing `panel`, `panel-strong`, dialog/toast/popover, scrim, widget borders | Added |
| 7 | Missing z-index scale (7 layers) with two apparent stacking conflicts | Added table; layering decided: keep as is |
| 8 | Missing: Inter not loaded (now Geist, bundled); tap-target rule mostly unenforced; no placeholder style; two shadow languages; 3 button recipes | Decided: bundle a font (Geist since 2026-10-11), 44/36px tap targets, placeholder style, new shadow, one panel recipe; coral is the only solid primary |
| 9 | Draft v1 listed `rounded` sm/md as in use; bare `rounded` (4px) is the real odd one | Documented |

| 10 | I said one timer used -0.045em vs -0.06em. Wrong: -0.045em is the song title heading, a different element | Corrected; `song-title` token added |
| 11 | Draft v2 had no text-colour rule for coloured fills; white on rose-500 (3.7), fuchsia-500 (3.5), sky-500 (2.8) fail | Rule decided; darker fills added |
| 12 | Live controls (4 severity colours) were not documented, and the red hover fill fails AA with white text | Documented as a deliberate exception; red hover fill darkened |
| 13 | Dashboard and docs used vendor brand names | Generic names decided; brand names only for OAuth/API-specific screens |

## Decisions made (owner, 2026-10-09)

**Principle:** the app must be usable by anyone without training. Nothing is
kept because the current code does it.

**Foundations**
- Three text steps (`text` warm, `text-body`, `text-muted`); `text-strong` is
  the same warm colour set bolder (700), not a separate colour.
- Three widget borders (6 / 10 / 18%). One warm layered default shadow (old
  shadow deleted); stronger levels 2 to 5 exist for the owner to choose.
- Light button labels carry a small very-dark-grey text shadow. Dark labels
  (on red and Services-green fills) carry none: the light shadow under dark
  text was tried and rejected.
- Advanced tones are 30% service colour mixed with 70% `text-muted`.
- Light text on dark fills, dark text on light fills; error text keeps its
  reddish colour. Contrast minimum 4.5:1 for text.
- The primary red is the only solid primary button. Playback is fuchsia `#e879f9`;
  MIDI is a connection method, not a service colour. Live controls keep four
  severity colours.
- Geist is bundled (replaced Inter, 2026-10-11). Three tracking steps; one 11px micro size; bare `rounded`
  becomes 8px. Inputs on `surface-950`.
- Photo background stays, fixed behind scrolling content. Blue loading bar
  stays; its white lead-in line is removed.
- Service icons are generic pixel-art illustrations (checklist, monitor with
  cursor, audio tracks), not vendor logos, and count as 8-bit moments.

**Sizes**
- 44px: live controls, inputs, and **every button on touch devices**
  (`@media (pointer: coarse)`). 36px buttons are for mouse pointers only.

**Patterns (see Patterns section)**
- Wide status cards above 1000px, compact below. Phone/remote loading is a
  spinner instead of the bar.
- Connection method select (Playback API main / MIDI alternate; Lighting shows
  MIDI as text). Activity and Advanced rows look different; "Other computer…"
  replaces "Connect a specific computer".
- Vocabulary: Connection, Services, Presentation, Playback, Lighting, MIDI,
  Configuration, General configuration / Advanced, StagePilot, Service plan, Cue (not pulse),
  Note (kept), People.
- StagePilot card and panel hold all app-wide configuration.
- Save as you go, plus an always-visible `Save configuration` button and a
  five-state save bar. Held until saved: service type / plan choice / Reload
  service plan, Presentation timer target, Playback method / computer /
  velocities, Lighting **MIDI network (output) and channel only**, StagePilot
  restart-needed items. Everything else applies immediately, including the
  rest of Lighting.
- **Send test cue** runs when pressed, using the saved output. It is disabled
  (greyed, with the reason beside it) while a new MIDI output is selected but
  not saved.

**Process**
- Documentation-only PR first, then component migrations in small PRs; styling
  and behaviour changes are never mixed in one PR.

## Primary colour change (owner, 2026-10-11)
Coral `#ff6238` is retired everywhere. The primary is the deep red `#c7231d`,
with the supplied shade and tint scales under `primary-scale`. Mapping:
- `primary` coral -> `#c7231d`; `primary-hover` -> tint 10% `#cd3934`;
  `primary-focus` -> tint 40% `#dd7b77`; new `primary-active` shade 10%
  `#b3201a`; new `primary-text` tint 50% `#e3918e`.
- Text on a primary fill is now **white** (5.7:1), not dark (dark would be 3.5:1).
- The red is 3.2:1 on panels: fill, rail, border and dot only. As text it is
  `primary-text`.
- Wash tints that were coral (`rgb(255 98 56 / x)`) become the primary at the
  same opacity.
- Red is now both "live/primary" and the error family. Told apart by form:
  primary = solid fill + white text or a rail; error = pale text + dot + icon;
  destructive = `danger-solid` red. Hover/focus/active never swap between them.
- Photo-sampled `flare-*` colours are for washes and glows on the live item
  only, never for text, buttons or status.

## Secondary accent (owner, 2026-10-12): teal `#1dc1c7`
The secondary accent is **information and "what comes next"**, never state and
never an action that changes the show. It is bright (8.2:1 on panels), so unlike
the red it works as text.
- **Tokens:** `info` `#1dc1c7` (fills, dots, borders, large glyphs), `info-text`
  tint 40% `#77dadd` (small text, 11.0:1), `info-hover` tint 10% `#34c7cd`,
  `info-active` shade 10% `#1aaeb3`, `on-info` `#020617` (dark text on an accent
  fill). It replaces sky (`#38bdf8`) as `info` and blue-700 as `live-reload`.
- **Uses (decided):** the **Up next** song label (dashboard song list and the
  Now-playing "Up next" caption), informational text and banners (`InlineAlert`
  and `PanelAlert` tone info, Remote Access status lines, first-launch
  configuration kicker, "Update available" kicker and version number), the
  **Reload service plan** live control, the info badge and status dot.
- **Extra uses (decided 2026-10-12, owner: make those real):** the **next song's number
  chip** beside "Up next"; the **"Send test cue"** result line; the **Activity**
  row's count badge (neutral grey today); links (`linkButton`, e.g. Playback
  "Other computer" and learn-more links); the **Update available** button; the
  note-display toggle glyphs in Lighting; the dashboard **intentional spacer**
  outline; and the **progress** of an update download.
- **Never:** state (that is green, red for warning and error, slate), the one primary action
  (that is the red), service identity (that is the integration hues), or the
  retro loading bar (stays blue pixel art).
- **Collision resolved (owner, 2026-10-12):** Lighting's identity colour is no
  longer sky; the teal accent is distinct from every connection colour now.

## Colour collisions to know about (owner palette, 2026-10-12)
The owner chose these five connection colours. Two of them sit very close to
other colours that already mean something. They are kept apart **by form and
placement**, never by hue alone. Contrast between two colours is 1.0 when
identical.
- **StagePilot `#c7231d` is the primary red (1.00).** The StagePilot card and
  panel therefore carry the live-red. Rule: the red *rail and wash* on a card or
  panel means **live or selected** and appears once per view; a connection tint
  is a *thin icon-tile tint and a 1px top border*, never a rail or a wash. A
  StagePilot panel keeps its single solid primary button (`Save configuration`),
  which is the same red as its tint; that is by design.
- **Lighting `#d53367` is next to the error red (`danger-solid` `#ff0000`,
  1.01) and close to the primary red (1.22).** Rule: Lighting is *tint and dot*
  only. Error is always `danger-text` words with a red dot and an icon;
  `Stop timer`/`Reset position` stay rose-coloured live controls (a different red from the error red). A Lighting card in an
  error state shows the status word `Error`, not a colour change.
- **Playback `#4b73b0` is no longer near the idle grey** (`idle` `#64748b`):
  hue 216 and saturation 40% against hue 215 and 17%. It is still only 1.01:1 in
  luminance from idle, so idle stays a dot plus the words *Disconnected*, and
  Playback stays a tile tint, top border and tinted buttons. Its **Advanced**
  tone (`#7e95b6`) is 5.9:1 on panels and clearly blue-grey.
- **Services green `#6bb03c` and the deep success green `#31551d`** are the same
  hue (96 vs 98) at very different brightness (2.3:1 apart as fills; the lifted
  `success-soft` `#417227` is 1.9:1 from Services). Rule: Services is identity only
  (icon tile, panel border, accent buttons). The success green is never used
  inside a Services panel at all: there, a "connected" message uses the panel
  accent (see "Accent inside a configuration panel"). So the two never touch.
- **Errors and warnings are pure red `#ff0000`, one family (decided 2026-10-13).** Both use
  `danger` / `danger-text`, so a warning is never mistaken for the primary red `#c7231d`
  (Backend, Start), Lighting `#d53367` or the Presentation orange. They are told apart by words
  and icon, not hue: warning is *Connecting*, *Delayed*, *Needs attention*, *Not saved yet*; error
  is *Error* and the failed-action message. Filled destructive buttons use `danger-solid`
  `#ff0000`, the same red, with white text (4.0:1, bold, label shadow). **Previous** (`live-back` `#2d487c`) is dark blue, the same lightness as Start next's green, so it is
  not mistaken for the red Stop controls.

## Decided (owner, 2026-10-13): "Current · live" and "Up next" describe the show
**Previous** and **Next** only move the navigation cursor: they select a song and start nothing (the
timer, ProPresenter and Lighting do not change). The labels used to follow the cursor, so after
Previous the previous song read "Current · live" while another song was still playing, and the real
effect (the next Start next plays it) was hidden. Now:
- **Current · live** (and the Now playing title, position and elapsed) is the song that was actually
  started, `playing_song`.
- **Up next** (service plan row and the Now playing caption) is the song the next **Start next** will
  play, `up_next_song`. After Previous that is the previous song.
- Before anything starts nothing is live and Up next is the first song. **Reset position** clears the
  live song. **Restart current** restarts the cursor song and makes it the live one.
- Playback, MIDI and the Lighting cue editor keep using the cursor (`current_song`,
  `current_song_index`), unchanged.
- A service plan reload keeps both the live song and the Start next target by song id.

## Decided (owner, 2026-10-13): a darker "connected" green
`success` is now **`#31551d`**. Measured on panels (`#0f1720`), cards and the live panel:
| Role | Value | Contrast | Used for |
| --- | --- | --- | --- |
| `success` | `#31551d` | 2.1:1 on panels; light text on it 7.8:1 | fills only: Ready pill, solid badge, Start next / Restart current on press |
| `success-soft` | `#417227` | 3.15:1 | dots and borders, 10-30% tints |
| `success-text` | `#69b63e` | 7.2:1 (6.1:1 on its own 20% tint) | the words *Connected*, *Ready*, the live control label at rest |
Because the base is too dark for a dot, **status dots are removed from the five
status cards and the header Ready pill** (decided 2026-10-13): the word carries
the state, with the pill outlined in `success-soft`. Dots stay in the Check system
list, badges and panels where a word sits beside them.

## Still open

Resolved (owner, 2026-10-12): Lighting moves to `#d53367`; see "Colour collisions".

Nothing blocking. Decisions recorded 2026-10-11 (owner):
- Release channel lives in **Advanced** in the StagePilot panel.
- "Pulse" becomes **Note** everywhere ("Note length (ms)", "Test cue").
- The Lighting output reads **MIDI Output to Lighting Controller**; no vendor name.
- Compact status words may go **below the 11px floor**. This is the one
  documented exception (status word, compact cards, 1000px and below). Nothing
  else may.
- The loading-bar fill keeps its **pixel stripes**.
- `elevation.overlay` is approved.
- Save behaviour: explicit save with held markers everywhere (option A); the
  backend split for true save-as-you-go (option C) is later work.

Still not done:
- Real-device touch, screen-reader and real 200% browser zoom checks (only emulated).
- Exhaustive destructive-action confirmation audit.
- Option C (backend split for true save-as-you-go).

Decided exception: startup/error screen text over the photo may measure as low as 1.1:1 at the brightest patch; owner decided to leave it (2026-10-11).
