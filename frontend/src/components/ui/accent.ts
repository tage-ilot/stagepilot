import { createContext, useContext } from "react";

import type { PanelIdentity } from "./Panel";

/**
 * Accent inside a configuration panel (DESIGN.md "Accent inside a configuration panel").
 * A Panel with a `tone` provides its connection here; Button, Badge, StatusDot, InlineAlert and SaveBar read it
 * and swap the primary red, the status green and the teal info for the connection's own colour.
 * Errors (danger), warnings, idle and destructive controls never change. Class strings are written out in full
 * so Tailwind can see them.
 */
export const AccentContext = createContext<PanelIdentity | null>(null);

export function useAccent(): PanelIdentity | null {
  return useContext(AccentContext);
}

export interface AccentClasses {
  /** Solid fill with its label colour: dark on the light fills (Services, Presentation), white on the rest. */
  solid: string;
  /** Outline + 10% fill + readable text (tinted buttons, badges, alerts). */
  tinted: string;
  /** Hover fill for tinted. */
  tintedHover: string;
  /** Small readable text. */
  text: string;
  /** The panel title: the connection colour itself (large bold text needs only 3:1). Backend uses its pale tint. */
  title: string;
  /** Link hover / pressed text. */
  textHover: string;
  /** A dot. */
  dot: string;
  /** Checkbox / radio accent. */
  checkbox: string;
  /** The Save bar: the accent at ~22% over the panel (a deep tint) with an accent outline. */
  bar: string;
  /** The banner (InlineAlert) look for success / info messages. */
  banner: string;
  /** The chosen option in the Connection method control: the connection colour as a 45% fill with a solid outline. */
  chip: string;
}

export const accentClasses: Record<PanelIdentity, AccentClasses> = {
  services: {
    solid: "border-transparent bg-integration-services text-on-light can-hover:bg-integration-services/85 active:bg-integration-services/75",
    tinted: "border-integration-services-soft/30 bg-integration-services-soft/10 text-integration-services-text",
    tintedHover: "can-hover:bg-integration-services-soft/20",
    text: "text-integration-services-text",
    title: "text-integration-services-soft",
    textHover: "can-hover:text-integration-services-text",
    dot: "bg-integration-services-soft",
    checkbox: "accent-integration-services",
    bar: "border-integration-services-soft/50 bg-integration-services-soft/[0.22]",
    banner: "border-integration-services-soft/40 bg-integration-services-soft/10 text-integration-services-text",
    chip: "border-integration-services-soft bg-integration-services-soft/45",
  },
  presentation: {
    solid: "border-transparent bg-integration-presentation text-on-light can-hover:bg-integration-presentation/85 active:bg-integration-presentation/75",
    tinted: "border-integration-presentation-soft/30 bg-integration-presentation-soft/10 text-integration-presentation-text",
    tintedHover: "can-hover:bg-integration-presentation-soft/20",
    text: "text-integration-presentation-text",
    title: "text-integration-presentation-soft",
    textHover: "can-hover:text-integration-presentation-text",
    dot: "bg-integration-presentation-soft",
    checkbox: "accent-integration-presentation",
    bar: "border-integration-presentation-soft/50 bg-integration-presentation-soft/[0.22]",
    banner: "border-integration-presentation-soft/40 bg-integration-presentation-soft/10 text-integration-presentation-text",
    chip: "border-integration-presentation-soft bg-integration-presentation-soft/45",
  },
  playback: {
    solid: "border-transparent bg-integration-playback text-on-primary text-shadow-label can-hover:bg-integration-playback/85 active:bg-integration-playback/75",
    tinted: "border-integration-playback-soft/30 bg-integration-playback-soft/10 text-integration-playback-text",
    tintedHover: "can-hover:bg-integration-playback-soft/20",
    text: "text-integration-playback-text",
    title: "text-integration-playback-soft",
    textHover: "can-hover:text-integration-playback-text",
    dot: "bg-integration-playback-soft",
    checkbox: "accent-integration-playback",
    bar: "border-integration-playback-soft/50 bg-integration-playback-soft/[0.22]",
    banner: "border-integration-playback-soft/40 bg-integration-playback-soft/10 text-integration-playback-text",
    chip: "border-integration-playback-soft bg-integration-playback-soft/45",
  },
  lighting: {
    solid: "border-transparent bg-integration-lighting text-on-primary text-shadow-label can-hover:bg-integration-lighting/85 active:bg-integration-lighting/75",
    tinted: "border-integration-lighting-soft/30 bg-integration-lighting-soft/10 text-integration-lighting-text",
    tintedHover: "can-hover:bg-integration-lighting-soft/20",
    text: "text-integration-lighting-text",
    title: "text-integration-lighting-soft",
    textHover: "can-hover:text-integration-lighting-text",
    dot: "bg-integration-lighting-soft",
    checkbox: "accent-integration-lighting",
    bar: "border-integration-lighting-soft/50 bg-integration-lighting-soft/[0.22]",
    banner: "border-integration-lighting-soft/40 bg-integration-lighting-soft/10 text-integration-lighting-text",
    chip: "border-integration-lighting-soft bg-integration-lighting-soft/45",
  },
  backend: {
    solid: "border-transparent bg-integration-backend text-on-primary text-shadow-label can-hover:bg-integration-backend/85 active:bg-integration-backend/75",
    tinted: "border-integration-backend-soft/30 bg-integration-backend-soft/10 text-integration-backend-text",
    tintedHover: "can-hover:bg-integration-backend-soft/20",
    text: "text-integration-backend-text",
    title: "text-integration-backend-text",
    textHover: "can-hover:text-integration-backend-text",
    dot: "bg-integration-backend-soft",
    checkbox: "accent-integration-backend",
    bar: "border-integration-backend-soft/50 bg-integration-backend-soft/[0.22]",
    banner: "border-integration-backend-soft/40 bg-integration-backend-soft/10 text-integration-backend-text",
    chip: "border-integration-backend-soft bg-integration-backend-soft/45",
  },
};
