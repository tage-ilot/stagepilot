import type { ReactNode } from "react";

import { Badge, StatusBadge } from "../Badge";
import type { StatusWord } from "../statusWord";
import { InlineAlert } from "../InlineAlert";
import { Panel } from "../Panel";
import { StatusDot } from "../StatusDot";
import type { Tone } from "../types";
import { Section } from "./Section";

const tones: Tone[] = ["services", "presentation", "playback", "lights", "success", "warning", "danger", "info", "idle"];
const words: StatusWord[] = ["Connected", "Connecting", "Not connected", "Error"];

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div data-gallery-row={label} className="space-y-2">
      <h3 className="font-semibold text-ink-body">{label}</h3>
      {children}
    </div>
  );
}

/** Real primitives. Panels sit on the app's film-flare body background (index.css), not a flat colour. */
export function PanelSection() {
  return (
    <Section name="panel-status" title="Panel, StatusDot, Badge, InlineAlert">
      <Row label="Panel variants (on the film-flare background)">
        <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
          {(["default", "setup", "live"] as const).map((variant) => (
            <Panel key={variant} variant={variant} data-gallery-target>
              <h4 className="font-bold text-ink">{variant} panel</h4>
              <p className="text-ink-body">Content sits 16px in ({variant === "setup" ? "20px" : "16px"} here).</p>
            </Panel>
          ))}
        </div>
      </Row>
      <Row label="StatusDot, sm (8px) and md (10px), every tone">
        <div className="flex flex-wrap items-center gap-3">
          {tones.map((tone) => (
            <span key={tone} className="inline-flex items-center gap-2 text-sm text-ink-body">
              <StatusDot tone={tone} />
              <StatusDot tone={tone} size="md" label={`${tone} status`} />
              {tone}
            </span>
          ))}
        </div>
      </Row>
      <Row label="Badge, tinted, every tone">
        <div className="flex flex-wrap gap-2">
          {tones.map((tone) => <Badge key={tone} tone={tone} dot>{tone}</Badge>)}
        </div>
      </Row>
      <Row label="Badge, solid">
        <div className="flex flex-wrap gap-2">
          {(["success", "warning", "danger"] as const).map((tone) => <Badge key={tone} tone={tone} emphasis="solid">{tone}</Badge>)}
        </div>
      </Row>
      <Row label="StatusBadge, the four status words">
        <div className="flex flex-wrap gap-2">
          {words.map((word) => <StatusBadge key={word} status={word} />)}
        </div>
      </Row>
      <Row label="InlineAlert">
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
          <InlineAlert tone="danger">Could not reach Planning Center. Check the connection and try again.</InlineAlert>
          <InlineAlert tone="warning">Another operator is editing these settings.</InlineAlert>
          <InlineAlert tone="success">Settings saved.</InlineAlert>
          <InlineAlert tone="info">Changes apply to the next service.</InlineAlert>
        </div>
      </Row>
    </Section>
  );
}
