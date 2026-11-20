import type { RemoteStatus, RemoteUser } from "../api";
import { DESKTOP_ACCESS } from "../access/accessState";
import { REMOTE_ACCESS_OPEN_KEY } from "../components/BackendSetupPanel";
import { RemoteAccessPanel } from "../components/RemoteAccessPanel";
import { OptionalSection } from "../components/sections";
import type { RemoteAccessControl } from "../hooks/useRemoteAccess";
import { Fresh, Scripted, type Step } from "./Scripted";
import type { SampleState } from "./StateCard";

const connected: RemoteStatus = { available: true, provisioned: true, credential_available: true, enabled: true, state: "connected", url: "https://example.trycloudflare.com/", needs_operator: false, message: null, temporary_url: true, permanently_revoked: false };
const users: RemoteUser[] = [{ id: "u1", email: "operator@example.test", role: "Operator", enabled: true }, { id: "u2", email: "viewer@example.test", role: "Viewer", enabled: true }];

/** The same object `useRemoteAccess` returns, with made-up values: the page is drawn, nothing is sent. */
const control = (over: Partial<RemoteAccessControl> = {}): RemoteAccessControl => ({
  access: DESKTOP_ACCESS, status: connected, users, setUsers: () => undefined, error: null, setError: () => undefined, notice: null, setNotice: () => undefined,
  busy: false, bootstrap: false, confirmDisable: false, checkboxBusy: false, credentialWarning: false, canManage: true, local: true, run: async () => undefined,
  enabled: true, panelOpen: true, requestEnable: () => undefined, requestDisable: () => undefined, cancelDisable: () => undefined, confirmDisableAccept: () => undefined, ...over,
});

const shown = (steps: Step[], node: React.ReactNode, flags?: Record<string, boolean>) => <Fresh flags={flags}><Scripted steps={steps}>{node}</Scripted></Fresh>;
const remote = (over: Partial<RemoteAccessControl> = {}) => <RemoteAccessPanel control={control(over)} />;
const inSection = (node: React.ReactNode) => (
  <OptionalSection defaultOpen persistKey={REMOTE_ACCESS_OPEN_KEY} summary="Connected" title="Remote access" variant="activity">{node}</OptionalSection>
);

export const remotePages: SampleState[] = [
  { id: "connected", label: "Remote access: connected", look: "Two boxed groups (Remote link, People); notices as quiet lines in their tone; the people list; Add a person.", render: () => shown([], remote()) },
  { id: "persisted-open", label: "Remote access section: remembered open", look: "The activity section starts open and stays as the person leaves it, across restarts.", render: () => shown([], inSection(remote())) },
  { id: "persisted-closed", label: "Remote access section: remembered closed", look: "Closed by the person's choice: only its row and the one-line summary.", render: () => shown([{ press: "Remote access" }], inSection(remote())) },
  { id: "bootstrap", label: "Remote access: first Operator", look: "The warning line and the form (email, password, Create Operator).", render: () => shown([], remote({ bootstrap: true, status: { ...connected, needs_operator: true } })) },
  { id: "confirm-disable", label: "Remote access: asking to turn it off", look: "A warning box that is a named group (not an interruption), with Confirm disable and Cancel.", render: () => shown([], remote({ confirmDisable: true })) },
  { id: "confirm-regenerate", label: "Remote access: regenerate the link", look: "Press Regenerate Remote link: the warning group asks first.", render: () => shown([{ press: "Regenerate Remote link" }], remote()) },
  { id: "error", label: "Remote access: error and notice", look: "The danger line (role alert) and the success line (role status) under the groups.", render: () => shown([], remote({ error: "Remote access is unavailable. Local StagePilot is unaffected. Try again shortly.", notice: "Link copied.", status: { ...connected, state: "error", url: null } })) },
  { id: "read-only", label: "Remote access: read-only person", look: "One quiet line: an Operator manages Remote access.", render: () => shown([], remote({ canManage: false })) },
];
