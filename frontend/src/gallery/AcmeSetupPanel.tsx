import { useState } from "react";

import { SetupPanelHeader } from "../components/SetupPanelHeader";
import { ActionButton, ConnectionRow, ConnectionSection, LiveLinkField, LiveLinkPill, PanelLayout, SectionNotice, SettingsSection, StatusSummary } from "../components/sections";
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
    <ConnectionRow>
      <LiveLinkField caption="Host"><Input aria-label="Host" onChange={(event) => setHost(event.target.value)} value={host} /></LiveLinkField>
      <ActionButton busy={testing} busyLabel="Testing…" label="Test connection" onClick={() => void test()} />
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
