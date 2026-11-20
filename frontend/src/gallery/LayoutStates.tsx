import { AcmeSetupPanel } from "./AcmeSetupPanel";
import { PanelLayout } from "../components/sections";
import { SetupPanelHeader } from "../components/SetupPanelHeader";
import { Fresh } from "./Scripted";
import type { SampleState } from "./StateCard";

/** A labelled stand-in for what a page puts in a slot, so the ORDER of a page can be judged without any page's own content. */
const slot = (number: number, name: string, what: string) => (
  <div className="rounded-ds-lg border border-dashed border-edge px-3 py-2 text-sm text-ink-body" data-slot-sample={name}><b className="text-ink">{number}. {name}</b>: {what}</div>
);

const header = <SetupPanelHeader closeLabel="Close the example" description="Header: the title in the connection's colour, the status pill, Close." status="connected" title="Example connection" />;
const slots = {
  connection: slot(3, "connection", "ConnectionSection (or the page's own connection row)"),
  settings: slot(4, "settings", "SettingsSection(s)"),
  actions: slot(5, "actions", "ActionRow: Test / Refresh / Load"),
  optional: slot(6, "optional", "OptionalSection(s): Advanced, then optional features"),
  content: slot(7, "content", "what only this page has (the song list, the MIDI monitor)"),
  notices: slot(8, "notices", "SectionNotice: why Save is off, the last failure"),
  saveBar: slot(9, "saveBar", "the Save bar, always visible"),
  summary: slot(10, "summary", "StatusSummary"),
};
const alerts = slot(2, "alert lines", "loading, connecting, a failure or message");

export const layoutTemplates: SampleState[] = [
  { id: "order", label: "Panel order: every slot", look: "PanelLayout with every slot filled: header, alert lines, connection, settings, actions, optional, content, notices, Save bar, summary. A page leaves a slot empty to skip it.", render: () => <Fresh><PanelLayout alerts={alerts} header={header} id="example-order" tone="presentation" {...slots} /></Fresh> },
  { id: "summary-top", label: "Panel order: summary first", look: "summaryAt=\"top\": the page opens with its summary (StagePilot's Status cards); nothing else moves.", render: () => <Fresh><PanelLayout header={header} id="example-summary-top" summaryAt="top" tone="backend" {...slots} /></Fresh> },
  { id: "new-connection", label: "A new connection page, built in 35 lines (AcmeSetupPanel.tsx)", look: "The worked example of DESIGN.md: type a host and press Test connection (it answers unless the host is \"bad\"). Real templates, made-up answers.", render: () => <Fresh><AcmeSetupPanel onSave={() => undefined} onTest={async (host) => { await new Promise((resolve) => setTimeout(resolve, 300)); return host.trim() !== "bad"; }} saved={{ host: "192.0.2.50", port: "9000" }} /></Fresh> },
];
