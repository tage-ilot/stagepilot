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
| disconnected / idle | | `idle` `#64748b` | near-white word "Disconnected" |
| information, "what comes next" | `info` teal `#1dc1c7` (dark text `on-info`) | `info` | `info-text` |

- **Warnings and errors are the same pure red** and are told apart by their words (`Connecting`, `Delayed`, `Not saved yet` vs `Error`, `Couldn't save`).
- `success` is too dark for a dot: it is a **fill**. Status cards and the header pill carry state by *word and outline*, not a dot. Dots appear only where a word sits beside them (Check system list, badges).
- **Teal (`info`) is never a state and never the primary action.** Uses: "Up next", info banners, links, the Reload service plan control, the Update available button.
- **The header pill** uses the darkest reds that still pass contrast on its dark surface: words `danger-pill` `#f00000` (4.5:1), outline `danger-edge` `#bc0000` (3:1). Do not reuse these for anything else.

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
| StagePilot (backend) | `integration-backend` | `#c7231d` (= primary) | white |

Each family has three roles: the base (solid fills, rails), `-soft` (dots, borders, 10-30% tints) and `-text` (small text, 7:1). `advanced-<name>` is the muted tone (30% hue / 70% grey) for that connection's Advanced section.
- **Inside a connection's panel** (via `Panel tone=…` and `AccentContext`): primary buttons, tinted buttons, links, checkboxes, badges, success and info messages and the Save bar all take the connection's colour. **Exceptions that keep their own colour:** errors and warnings (pure red), idle grey, destructive buttons, and everything outside the panel (header, dialogs, toasts).
- **Never use a connection colour to mean state.** The open status card wears its connection colour (70% border, 10% wash fading by 44%, 3px left bar); the StagePilot card is the exception (it is primary red) and keeps a neutral card with a 1px red border.
- A *method* inside a connection (MIDI under Playback, OSC under Lighting) gets its own sub-hue **only for the chosen option in the `ConnectionMethod` control** (`chip` prop). Do not tint whole panels with method colours.

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
| `secondary` | `surface-950` fill, `edge-medium` outline | neutral: Discard, Close, Copy Log |
| `destructive` / `destructive-soft` | `danger-solid` red | irreversible actions only; label names what is lost (`Delete layout`) |
| `live` + `liveTone` | outlined, fill on hover/press | Manual controls only (§3.5) |
- Labels are **verb + object** (`Save configuration`, `Scan Network`, `Test connection`), sentence case, never `OK`/`Submit`/`Yes`. The safe option sits on the left of a pair.
- Disabled = 40% opacity + `cursor-not-allowed` **and a reason shown nearby** (`No changes to save`). Never silently grey out.
- Only one primary button per panel. Focus ring: 2px `primary-focus`, 3px offset, global; never remove.

### 6.2 Inputs and choices
- `Field` = label **above** (never left), optional hint in `ink-muted`, error under in `danger-text` with a short fix ("Enter a number from 1 to 16"). `Input`/`Select`: `surface-950`, `edge`, 8px, **44px** tall, `ink-muted` placeholder. Validate on blur; keep the user's input on error.
- **Choose one value from a list → styled native `Select`.** **Choose an action → action menu** (button + chevron opening a `surface-950` card, 44px items, arrow keys + Escape). **Choose how a connection connects, or one of a few exclusive modes → `ConnectionMethod`** (below). A one-off value (a specific computer) is the last option of the select it belongs to ("Other computer…"), never a separate collapsed row.
- Short fixed-width controls stay short: a PIN input is `w-28` with **Save PIN** beside it on the same row.

### 6.3 `ConnectionMethod`: THE switch
A segmented radio group: two or more options side by side, each a bold name, optionally over a tiny uppercase `PRIMARY`/`ALTERNATE` (`role` prop; omit it for unranked options). The chosen option wears the connection's tint (outline + 45% fill) and **the blue box slides between options**. Native radios (arrow keys, 44px). Used for **Playback** (Playback API primary / MIDI alternate), **Lighting** (OSC primary / MIDI alternate) and **Backend Connection Access** (Host / Local network / Internet). `label` overrides the kicker (default `Connection method`). **Never build a second picker; add options to this one.** Changing method swaps the fields beneath it; fields belonging to the other method are hidden and never affect each other (OSC cues work only when OSC is saved, MIDI only when MIDI is).

### 6.4 `Disclosure`: three kinds of expandable thing (shape says what it is)
| Variant | Look | Means |
| --- | --- | --- |
| `advanced` | one tinted bordered row, chevron + label, **one-line summary of what is inside when closed** (`Port 18766 · Logging info`), 3px left bar in the connection's `advanced-*` tone when open, 44px | extra options. Always **after** General configuration. Opens by itself when a field inside is invalid |
| `activity` | flat, no box/fill, dashed top rule, neutral count pill (`12 events`) | read-only information, or a group of controls people may not use (Remote access). Pass `usePersistedFlag` so the open state survives restarts |
| `feature` | card with a **5px rail, 70% border and soft wash in the connection colour**, `text-lg` title, a **detail line** shown open or closed that says what is inside (`16 cues on 2 of 4 songs · 2 pads · Battle Belongs 04:41`), an `aside` tag at the right | a **major** optional feature that deserves weight (Lighting's Song lighting timeline). Everything in the feature, including its Test cue controls, folds away with it |
Expand/collapse 500ms; respect reduced motion. **Persist** collapse state with `usePersistedFlag(KEY, default)` whenever a section is optional for some users (Remote access, Song lighting timeline: both start open).

### 6.5 Other primitives
- **`Panel`** (`variant` default | setup | live; `tone` = connection): `surface-panel` fill, `edge` border, 12px, `elevation.default`. `setup` adds the connection-tinted wash and 20px padding; `live` adds the red rail and 135° wash.
- **`SaveBar`**: §9. **`Badge`/`StatusBadge`**: full radius, uppercase pixel text, hue at 10-15% fill. **`InlineAlert`** (warning/danger → `role="alert"`; success/info → `role="status"`): 8px, hue 10% fill, 20-30% border, under the thing it is about.
- **`ScrollRegion`**: any box that scrolls on its own is drawn as an obvious bordered region (1px `ink-muted` border, 12px, `surface-950`), with an 18px fade **over the content, inputs included** along any edge with more to reach. Keep it focusable and named (`role="region"`, `tabIndex=0`, `aria-label`). An empty list is not framed. Do not use a dark shadow (invisible on `surface-950`).
- **Borders inside a panel:** a panel is one box. Only **form fields, the Advanced/Activity/Feature rows, `ScrollRegion`, the Save bar, and the two Remote-access groups** get their own border. Group everything else with a heading, spacing or a single `edge-subtle` divider. Never a divider *and* a box around the same thing.
- **Dialogs** (`fixed inset-0`, `scrim`, centred `surface-950`, 12px, `elevation.overlay`, 44px actions) and **toasts** (bottom corner, 16px, red border) are for things the user did not just cause (crash, update) or for confirming something destructive. Anything about the current panel is an inline alert.

---

## 7. The dashboard

### 7.1 Header (above the widgets)
- **Logo first, never moving.** One row at every width: the StagePilot logo sits **32px in from the left edge of the background image** (the image is 1700px wide and centred, so on a window wider than it the logo moves with the image, never into the black beside it). Everything else is one status pill to its right.
- **Status pill** (`HeaderStatusChip`): always the widgets' dark `surface-950` with a thin outline and bold-pixel words. Ready = green (`success-soft` outline, `success-text` words). Check system / Error = red (`danger-edge` outline, `danger-pill` words). **The pill is the notification:** on an info message the *same pill* grows to the message size, keeps the dark surface and takes the Previous blue (`live-back-soft` outline, `live-back-text` words); an error message takes the red; then it shrinks back. New messages show immediately (words slide, pill resizes); each lasts **5 seconds max**; simultaneous messages play in turn. If the header is too narrow even for the pill, the pill disappears (never a dot) and the message appears just under the header on a solid surface. Announce via a visually hidden live region (the drawn copy is `aria-hidden`).
- **System check list** (the pill's hover/focus panel, above the widgets): lists only what the show cannot run without: Services, Service plan, Song durations, Playback, Presentation (and its timer), StagePilot. **Lighting is not in it** (optional). The Playback entry names the saved method ("Playback API connected" / "Playback MIDI disconnected"). Words match the status cards.

### 7.2 Startup and the hand-off
- Desktop startup: centred `brand-hero` wordmark, the blue pixel loading bar, one headline, one detail line. Phone/remote: a circular pixel spinner *instead of* the bar. Blue = loading, red = slow, red and stopped = failed. Remote sign-in: one centred card, one primary button, errors under the field in `danger-text`.
- **The loading logo glides into the header** (`startup/logoLanding.ts`): the dashboard's own header logo starts at the loading logo's centre and size and moves home (translate + scale, 720ms) while the rest of the dashboard is hidden from the first frame, then the whole dashboard fades in over 200ms (borders and contents together; a border-first stagger was tried and dropped). It never plays on a reload of the loaded app or in tests; reduced motion skips it.
- "Can't reach StagePilot" retries by itself when the dashboard loads and whenever that error occurs; the manual **Try again** remains.

### 7.3 Status cards (five, in this order: Services, Playback, Presentation, Lighting, StagePilot)
Icon tile + title + **status word** (Connected / Connecting / Disconnected / Error) + one detail line. No status dot. The word is bold-pixel, uppercase, and **grows with the card** (about 12px up to a 22px maximum) so DISCONNECTED always fits on one line; the title is 13px. Card hover/open tints the title and description whenever the *card* is hovered or open (not only the text). At **≤1000px** viewport width the cards switch to **compact** (icon tile, word on one line beneath it; never wraps, ellipsizes or overlaps). The open card wears its connection colour (§3.4).

### 7.4 Widgets (GridStack, 12 columns desktop / 6 tablet, 5px gutters)
Four widgets: **Service plan**, **Now playing**, **Manual controls**, **Recent event stream** (`dashboardWidgetRegistry.ts`).
- **Default (and "Reset layout") arrangement:** Service plan on the left (x 0, 7 wide); on the right, in one 5-wide column: Now playing, then Manual controls, then Event stream, all the same width as Now playing. First-time users get exactly this.
- **Free placement:** widgets can go anywhere they do not overlap. There is no spacer tool. A light **upward gravity** keeps gaps from forming at the top unless a widget cannot fit beside what is there; the startup pass settles by gravity only (no compaction).
- **No clipped content, no dead space:** every widget has min sizes; **Now playing and Manual controls heights lock to their content** (`dashboard/contentFit.ts`), so a user cannot resize them into cut-off controls or empty space. Service plan and Event stream scroll internally.
- **Now playing:** `live` panel with a left rail. At rest (idle/paused/stopped/ended) the rail is the dark Previous blue; when the countdown starts the live red **sweeps down from the top-left** and then a lighter band keeps travelling down it ("alive to the music"); on stop the blue sweeps back the same way. The label, elapsed time and glow wait for the sweep and follow it. Every colour change starts at the top; none fades the whole bar. Countdown: Geist Mono 300, +8px; elapsed time matches **Time remaining**'s style; `Up next` is teal, `Current · live` is the song actually started (`playing_song`), `Up next` is what **Start next** will play (`up_next_song`).
- **Service plan rows:** hover tints **dark** on non-song rows and a subtle red-orange "live" tint (`can-hover:bg-primary/[0.08]`) on song rows. The plan title uses bold pixel.
- **Manual controls:** six live-control buttons in **even rows**: 2 columns (default), 3 columns from 37rem, 6 columns from 74.5rem container width (`.manual-controls-grid`), never a ragged last row, and only more per row when the label fits on one line. Labels use bold pixel.
- Edit mode: **Edit layout** → drag the labelled **Move** handle, resize from the lower-right, **Compact layout**, **Reset layout**, **Done**. Normal mode locks every widget so a live press cannot move one. Layout data never leaves the browser.

---

## 8. Configuration panels: the shared anatomy

Every connection opens a panel under the status cards, in this order. **A new connection copies this exactly.**
1. **Header** (`SetupPanelHeader`): kicker `Connection configuration` (the StagePilot panel says `Backend configuration`), a 20px bold **title in the connection's own colour** (`-soft`; the StagePilot title uses its pale `-text` tint because its red is only 3.2:1), a one-sentence description in body grey, the **status pill** and the 44px **Close** button. The pill sits in a **fixed slot immediately left of Close on every panel**, uses the real state colours (green/red/idle), **not** the connection tint, and does not change the title position. Below 480px the pill moves to its own row under the title.
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
  - Fields: Service type, Timezone, Service plan title preference, Preferred service time in **1, 2 or 4 columns** (28rem / 56rem container), never 3+1. Then `Load service types` / `Load today's plan`.
  - The status checks at the bottom are four across while their words fit (panel ≥ 30rem), then two, then one.
- **Presentation** (orange): Host, Port, Detected timer, Timeout (seconds), ProPresenter Look (first option reads **Current Look**, meaning "leave the Look alone"), `Test connection`, `Refresh timers and Looks`.
- **Playback** (blue; accessible name "MIDI / Playback"): `ConnectionMethod` (Playback API primary / MIDI alternate). *Playback API view:* `Scan Network` with its status text to its **right**, and **Playback host** (select with "Other computer…") on the right half of the row, dropping under the scan button when narrow; network scan and automatic connecting are **always on and have no switch**; **Song order**: Playback's songs and the service plan's songs as linked pairs (`Song arrangement`, below), with `Update Services` and the category menu; `Recent activity` (`activity`). *MIDI view (blue-violet accent)*: Available input (select, `Refresh inputs`, `Connect`/`Disconnect`); **Playback song velocities** (an `advanced` Disclosure) holds MIDI channel, Fixed note, Debounce (ms) and the six action velocities (unique, 100-127) and sits just above the **live MIDI note monitor**.
- **Song arrangement (Playback, decided 2026-10-14).** Playback songs and service-plan songs are **linked pairs, and nothing can be reordered**: each side can only **skip** a song, and the Nth song left on one side pairs with the Nth left on the other, so a crossed pairing cannot exist. Left column **Service plan**, right column **Playback**, a numbered link between them. A skipped plan song reads `No track`; a skipped Playback song reads `Not in this service`; both are dashed, struck through and keep their switch. The only control is the shared `Switch` (a real `role="switch"`, 44px row) with the single word **Skip**, bottom right of every box and aligned across the pair. Skips are a **held change** saved by `Save configuration` (order: method, arrangement, MIDI). Playback's lengths show `N s apart` beside each pair (green within 30 s, red past 120 s with `Probably a different song`: a hint only, never an automatic change). If a skipped plan song disappears from the loaded plan, cues are held back and the panel offers `Forget it`. A new scan clears Playback skips (a new setlist) but keeps plan skips. The group decides by **its own width**: under 35rem each pair stacks (plan box, vertical link, Playback box), the plan box kept left and the Playback box kept right. A skipped Playback song never touches the service timer.
- **Lighting** (pink-red): `ConnectionMethod` (OSC primary / MIDI alternate; the choice is saved and only that method's cues fire). Connection fields (OSC: Host or IP address, UDP port; MIDI: MIDI Output, MIDI channel, Note length) fit one row when the panel is wide. Then the **Song lighting timeline** `feature` Disclosure, which holds: Song select, ♪# toggle and `Add Cue` beside it, **Test cue** (note/OSC address, velocity/argument, `Send test cue`, kept compact on one line and justified right of the song select, moving above it when narrow), the **Cue board** (saved reusable pads; press one while a song plays to capture a cue at the current time; `Preview only`, `Undo last capture`, `Edit pads`), the **Timeline** (marker labels stack on up to 3 rows; zoom), and the cue list in a `ScrollRegion` with Clear / Clear all. Lighting is optional, so it never turns the header pill to Check system.
- **StagePilot / backend** (red): top to bottom: **Status** (four recessed cards, 1/2/4 across: Version, State revision, API endpoint, WebSocket), **Advanced** (Timezone first, then Server port, Log level, Release channel, 1/2/4 across; the restart-needed note sits at its bottom), **Connection Access** (§9), then the Save bar and support-log buttons (`Copy Log`, `Send all logs to developer`) below a divider.
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
  The bar is a deep tint of the connection colour; a *clean* bar is quiet, a *held* or *major* one is not. The Lighting feature card and Save bar sit side by side and must read as different things in one family (card has a rail; Save bar is a flat tint).
- **Held until Save configuration:** connection method, which computer, velocities, MIDI network/channel/note length, OSC host/port, Presentation timer/Look, Services type/plan choice, StagePilot port/timezone/log level/release channel. Reason: saving most of these reconfigures or reconnects something a running show may use.
- **Saved immediately (and the bar says "Saved automatically"):** Connection Access choice, the PIN via its own `Save PIN`, Lighting cue-list edits (add/remove/edit cues, cue board captures), `Scan Network`/`Connect` host saves, Remote-access actions, UI preferences (collapse state, note-name display). Rule of thumb: **if pressing it during a show could break the show, hold it; if operators must do it live or it only changes what *you* see, apply it at once.**
- Held changes need **Save configuration** whether or not a show is running. Leaving a panel with held changes asks `Discard unsaved changes?` (focus lands on Discard; Escape and backdrop do not dismiss).
- **Send/Test actions** (Test connection, Send test cue, Test) run when pressed using the *saved* configuration, and are disabled with a reason while a different output is selected but unsaved.

---

## 10. Words

| Say | Never say | Meaning |
| --- | --- | --- |
| **Connection** | integration, plugin, connector | something StagePilot talks to |
| **Services** | Planning Center (except sign-in/OAuth/API screens) | supplies the service plan |
| **Presentation** | ProPresenter (except where the product is the subject: "ProPresenter Look") | shows the countdown |
| **Playback** | MultiTracks | tells StagePilot a song started. Status card accessible name: "MIDI / Playback" |
| **Lighting** | Lightkey | receives cues |
| **StagePilot** | backend, local API (except the kicker `Backend configuration`) | the fifth card; app-wide configuration |
| **Connection method**, **Connection Access** | connection type, mode | how a connection is reached; who can reach StagePilot |
| **Host / Local network / Internet** | This Computer only, Remote access (as the option names) | the three Connection Access options |
| **OSC**, **MIDI** | | protocol methods; MIDI is Playback's alternate and Lighting's alternate |
| **Configuration**, **General configuration**, **Advanced** | settings, setup | what you edit; the two parts of a panel |
| **Service plan** | plan | the day's song list |
| **Cue** (a message sent to Lighting), **Note** (MIDI note and its length) | pulse | |
| **Cue board / pad / Timeline** | | Lighting's saved reusable cues, their buttons, and the visual song line |
| **Remote access / People** | remote users | the Internet section and who may sign in |
| **Connected · Connecting · Disconnected · Error** | not connected | the four status words (sentence case in text, uppercase pixel in badges and cards) |
| **Current · live / Up next** | | the song actually started / the song Start next will play |
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

### 12.3 Still open
- Real-device touch, screen-reader and 200% browser-zoom checks are emulated only.
- A confirmation audit for every destructive action.
- Backend split for true per-field save-as-you-go.
- Decided exceptions: the compact status-card word may be below 11px; startup/error text over the brightest patch of the photo may measure 1.1:1.
