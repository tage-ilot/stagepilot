import { useEffect, useRef, useState, type FormEvent } from "react";
import {
  bootstrapRemote, createRemoteUser, deleteRemoteUser, regenerateRemote, resetRemoteIdentity,
  updateRemoteUser, type RemoteUser,
} from "../api";
import type { RemoteAccessControl } from "../hooks/useRemoteAccess";
import { ActionRow, SectionNotice, SettingsSection } from "./sections";
import { Button } from "./ui/Button";
import { Field } from "./ui/Field";
import { Input } from "./ui/Input";
import { Select } from "./ui/Select";

const labels: Record<import("../api").RemoteStatus["state"], string> = {
  off: "Off", enabling: "Enabling…", connected: "Connected", reconnecting: "Reconnecting…", error: "Connection unavailable",
};

function usePrefersReducedMotion(): boolean {
  const [reduced, setReduced] = useState(
    () => window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false,
  );
  useEffect(() => {
    const query = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    if (!query) return;
    const onChange = () => setReduced(query.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);
  return reduced;
}

export function ButtonSpinner() {
  const reducedMotion = usePrefersReducedMotion();
  if (reducedMotion) {
    return <span aria-hidden="true" className="text-xs font-semibold tracking-wide">Working…</span>;
  }
  return <span
    aria-hidden="true"
    className="inline-block h-3.5 w-3.5 animate-spin rounded-full border-2 border-current border-t-transparent"
  />;
}

function safeUrl(value: string | null): string | null {
  if (!value) return null;
  try {
    const parsed = new URL(value);
    return parsed.protocol === "https:" && !parsed.username && !parsed.password && !parsed.port
      && parsed.pathname === "/" && !parsed.search && !parsed.hash ? value : null;
  } catch { return null; }
}

export function RemoteAccessPanel({ control }: { control: RemoteAccessControl }) {
  const {
    access, status, users, error, notice, busy, bootstrap,
    confirmDisable, cancelDisable, confirmDisableAccept, canManage, local, run, credentialWarning,
  } = control;
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [role, setRole] = useState<RemoteUser["role"]>("Viewer");
  const [edit, setEdit] = useState<RemoteUser | null>(null);
  // The sign-in form for another person stays folded away until "Add a person" is pressed.
  const [adding, setAdding] = useState(false);
  const [confirmRegenerate, setConfirmRegenerate] = useState(false);
  const [regenBusy, setRegenBusy] = useState(false);
  const [resetBusy, setResetBusy] = useState(false);
  const passwordCleared = useRef(false);

  useEffect(() => {
    if (!busy && !passwordCleared.current) { setPassword(""); passwordCleared.current = true; }
    if (busy) passwordCleared.current = false;
  }, [busy]);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (edit && !window.confirm(`Replace the password for ${edit.email} and revoke all of their Remote sessions?`)) return;
    void run(async () => {
      if (bootstrap) {
        // Remote is already running by the time this form can appear (see
        // useRemoteAccess's `bootstrap` derivation) -- just create the
        // Operator; no redundant enable call needed here.
        await bootstrapRemote(email, password);
        setEmail("");
      } else if (edit) {
        await updateRemoteUser(edit.id, {password}); setEdit(null);
      } else {
        await createRemoteUser(email, password, role); setEmail(""); setAdding(false);
      }
    }, edit?.email === access.user?.email).catch(() => undefined); // run() already shows the error; do not also throw
  };
  const url = status?.state === "connected" ? safeUrl(status.url) : null;
  const operators = users.filter((user) => user.enabled && user.role === "Operator").length;
  if (!canManage) return <div aria-label="Remote access"><p className="text-sm text-ink-body">Read-only access. An Operator manages Remote access.</p></div>;

  // Remote Access is two separate groups (owner, 2026-10-14): the link and its status, then the people who can use it.
  const showPeople = Boolean(status?.needs_operator && !local) || bootstrap || Boolean(status && !status.needs_operator);

  return <div aria-label="Remote access" className="space-y-4">
    <SettingsSection aria-label="Remote link" boxed columns="single" data-remote-group="link">
    <SectionNotice tone="info" variant="line">Securely view or operate this StagePilot from another device. Local operation continues if Remote disconnects.</SectionNotice>
    <SectionNotice role="status" tone="success" variant="line">{status ? labels[status.state] : "Checking Remote access…"}</SectionNotice>
    {status?.state === "enabling" && <SectionNotice quiet tone="info" variant="line">
      Check for a system password prompt (Keychain/Credential Manager) and enter it — this can take a moment.
    </SectionNotice>}
    {status?.temporary_url ?
      <SectionNotice tone="warning" variant="line">Temporary Remote link: the address changes after reconnection or restart. This preview is not a permanent remote address.</SectionNotice> :
      <SectionNotice tone="success" variant="line">Stable Remote link: this installation keeps the same address after reconnecting.</SectionNotice>}
    {status && !status.available && <SectionNotice tone="info" variant="line">Remote access is unavailable on this installation. Local StagePilot is unaffected.</SectionNotice>}
    {status?.provisioned && !status.credential_available && credentialWarning && <SectionNotice tone="warning" variant="line">The installation credential is unavailable or revoked. Remote remains off; contact beta support to recover this installation.</SectionNotice>}
    {status?.permanently_revoked && <SectionNotice
      actions={<Button className="gap-2" variant="primary" disabled={busy || resetBusy} type="button" onClick={() => {
        if (resetBusy) return;
        setResetBusy(true);
        void run(async () => { await resetRemoteIdentity(); }).catch(() => undefined).finally(() => setResetBusy(false));
      }}>{resetBusy && <ButtonSpinner />}{resetBusy ? "Resetting…" : "Reset installation identity"}</Button>}
      group="Reset installation identity"
      tone="warning"
    >
      Your previous Remote access configuration could not be restored and needs to be reset. This is safe and won't affect local StagePilot.
    </SectionNotice>}
    {status?.state === "error" && <SectionNotice tone="info" variant="line">Check your Internet connection. You can disable Remote and try enabling it again.</SectionNotice>}
    {status?.state === "reconnecting" && <SectionNotice tone="info" variant="line">Reconnecting. Check here for the current link once connected.</SectionNotice>}

    {url && <div className="flex flex-wrap items-center gap-3">
      <output aria-label="Remote URL" className="break-all text-sm text-integration-backend-text">{url}</output>
      <Button variant="secondary" type="button" onClick={() => {
        if (!navigator.clipboard) { control.setError("Copy unavailable. Select and copy the link above."); return; }
        void navigator.clipboard.writeText(url).then(() => control.setNotice("Link copied."), () => control.setError("Copy unavailable. Select and copy the link above."));
      }}>Copy link</Button>
      <Button className="gap-2" variant="secondary" disabled={busy || regenBusy} type="button" onClick={() => setConfirmRegenerate(true)}>{regenBusy && <ButtonSpinner />}{regenBusy ? "Regenerating…" : "Regenerate Remote link"}</Button>
    </div>}
    {confirmRegenerate && <SectionNotice
      actions={<ActionRow>
        <Button className="gap-2" variant="primary" disabled={busy || regenBusy} type="button" onClick={() => {
          if (regenBusy) return;
          setConfirmRegenerate(false);
          setRegenBusy(true);
          void run(async () => { await regenerateRemote(); }).catch(() => undefined).finally(() => setRegenBusy(false));
        }}>{regenBusy && <ButtonSpinner />}{regenBusy ? "Regenerating…" : "Confirm regenerate"}</Button>
        <Button variant="secondary" disabled={regenBusy} type="button" onClick={() => setConfirmRegenerate(false)}>Cancel</Button>
      </ActionRow>}
      group="Confirm regenerate Remote link"
      tone="warning"
    >
      The current Remote link will stop working. Anyone using the old address will lose access. Continue?
    </SectionNotice>}
    {confirmDisable && <SectionNotice
      actions={<ActionRow>
        <Button variant="primary" disabled={busy} type="button" onClick={confirmDisableAccept}>Confirm disable</Button>
        <Button variant="secondary" type="button" onClick={cancelDisable}>Cancel</Button>
      </ActionRow>}
      group="Confirm disable Remote access"
      tone="warning"
    >
      Disconnect all people? Local StagePilot will keep running.
    </SectionNotice>}
    </SettingsSection>
    {showPeople && <SettingsSection aria-label="Remote users" boxed columns="single" data-remote-group="people">
    {status?.needs_operator && !local && <SectionNotice tone="info" variant="line">Create the first Operator from local StagePilot.</SectionNotice>}
    {bootstrap && <>
      <h3 className="text-xs font-bold uppercase tracking-wider text-ink-muted">Create first Operator</h3>
      <SectionNotice tone="warning" variant="line">Remote access is running. Create the first Operator to allow sign-in.</SectionNotice>
    </>}
    {(bootstrap || status && !status.needs_operator) && <>
      <SectionNotice tone="info" variant="line">Viewers are read-only. Operators can control StagePilot.</SectionNotice>
      {!bootstrap && <ul className="divide-y divide-edge-subtle">{users.map((user) => {
        const last = user.enabled && user.role === "Operator" && operators <= 1;
        const self = user.email === access.user?.email;
        return <li key={user.id} className="flex flex-wrap items-center gap-3 text-sm text-ink-body">
          <span className="text-sm text-ink">{user.email} · {user.enabled ? "Enabled" : "Disabled"}{last ? " · Last Operator" : ""}</span>
          <Field label={`Role for ${user.email}`}><Select value={user.role} disabled={busy || last}
            onChange={(event) => {
              const nextRole = event.target.value as RemoteUser["role"];
              if (user.role === "Operator" && nextRole === "Viewer" && !window.confirm(`Change ${user.email} to Viewer and revoke their Remote sessions?`)) return;
              void run(() => updateRemoteUser(user.id, {role: nextRole}), self).catch(() => undefined);
            }}>
            <option>Viewer</option><option>Operator</option></Select></Field>
          <Button variant="secondary" disabled={busy || last} type="button" onClick={() => {
            if (user.enabled && !window.confirm(`Disable ${user.email} and revoke their Remote sessions?`)) return;
            void run(() => updateRemoteUser(user.id, {enabled: !user.enabled}), self).catch(() => undefined);
          }}>{user.enabled ? "Disable" : "Enable"} {user.email}</Button>
          <Button variant="secondary" disabled={busy} type="button" onClick={() => {setEdit(user); setPassword("");}}>Change password for {user.email}</Button>
          <Button variant="secondary" disabled={busy || last} type="button" onClick={() => {
            if (window.confirm(`Delete ${user.email} and revoke their sessions?`)) void run(() => deleteRemoteUser(user.id), self).catch(() => undefined);
          }}>Delete {user.email}</Button>
        </li>;
      })}</ul>}
      {operators === 1 && <SectionNotice tone="warning" variant="line">Last Operator protection is active. Add or enable another Operator before changing or deleting the remaining Operator.</SectionNotice>}
      {!bootstrap && !edit && !adding && (
        <div>
          <Button disabled={busy} onClick={() => setAdding(true)} type="button" variant="tinted">Add a person</Button>
        </div>
      )}
      {(bootstrap || edit || adding) && <form onSubmit={submit} className="flex flex-wrap items-end gap-3">
        {!edit && <Field label="Email">
          <Input type="email" autoComplete="off" required maxLength={254} disabled={busy} value={email} onChange={(e) => setEmail(e.target.value)} />
        </Field>}
        <Field label={edit ? `New password for ${edit.email}` : "Password"}>
          <Input type="password" autoComplete="new-password" required minLength={12} maxLength={1024} disabled={busy} value={password} onChange={(e) => setPassword(e.target.value)} />
        </Field>
        {!edit && !bootstrap && <Field label="New user role">
          <Select disabled={busy} value={role} onChange={(e) => setRole(e.target.value as RemoteUser["role"])}><option>Viewer</option><option>Operator</option></Select>
        </Field>}
        <Button variant="primary" disabled={busy} type="submit">{bootstrap ? "Create Operator" : edit ? "Save password" : "Add user"}</Button>
        {(edit || adding) && <Button variant="secondary" type="button" onClick={() => {setEdit(null); setAdding(false); setEmail(""); setPassword("");}}>Cancel</Button>}
      </form>}
    </>}
    </SettingsSection>}
    {notice && <SectionNotice role="status" tone="success" variant="line">{notice}</SectionNotice>}
    {error && <SectionNotice role="alert" tone="danger" variant="line">{error}</SectionNotice>}
  </div>;
}
