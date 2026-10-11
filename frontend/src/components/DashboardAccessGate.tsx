import { useCallback, useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";

import { getAccess, loginDashboard, loginRemote, logoutRemote } from "../api";
import { isDesktopShell } from "../desktop";
import { AccessContext } from "../access/AccessContext";
import { DESKTOP_ACCESS, NO_CAPABILITIES, invalidateAccess, onAccessInvalidated, setApiAccess } from "../access/accessState";
import type { DashboardAccess } from "../types";
import { Button, Field, Input, Panel } from "./ui";

export function DashboardAccessGate({ children }: { children: ReactNode }) {
  const [access, setAccess] = useState<DashboardAccess | null>(() => isDesktopShell() ? DESKTOP_ACCESS : null);
  const [checking, setChecking] = useState(!isDesktopShell());
  const [pin, setPin] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const requestId = useRef(0);

  const accept = useCallback((next: DashboardAccess) => {
    setApiAccess(next);
    setAccess(next);
    setChecking(false);
  }, []);

  const check = useCallback(async () => {
    const id = ++requestId.current;
    setChecking(true);
    setError(null);
    try {
      const next = await getAccess();
      if (id === requestId.current) accept(next);
    } catch (cause) {
      if (id === requestId.current) {
        setError(cause instanceof Error ? cause.message : "Dashboard access check failed.");
      }
    } finally {
      if (id === requestId.current) setChecking(false);
    }
  }, [accept]);

  useEffect(() => {
    const unsubscribe = onAccessInvalidated(() => {
      requestId.current += 1;
      setAccess((current) => current ? {
        ...current, authenticated: false, capabilities: NO_CAPABILITIES, csrf_token: null,
      } : null);
      setChecking(false);
      setPending(false);
      setPin("");
      setPassword("");
      setError("Your access has ended. Please sign in again.");
    });
    if (isDesktopShell()) setApiAccess(DESKTOP_ACCESS);
    else void check();
    return () => {
      requestId.current += 1;
      unsubscribe();
      setApiAccess(null);
    };
  }, [check]);

  useEffect(() => {
    if (!access?.authenticated || !access.expires_at) return;
    const timer = window.setTimeout(() => invalidateAccess(),
      Math.max(0, access.expires_at * 1_000 - Date.now()));
    return () => window.clearTimeout(timer);
  }, [access]);

  const signOut = async () => {
    setPending(true);
    setError(null);
    try {
      await logoutRemote();
      invalidateAccess();
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Sign out failed. Try again.");
    } finally {
      setPending(false);
    }
  };

  if (access?.authenticated && access.capabilities.canRead) {
    return (
      <AccessContext.Provider value={access}>
        {access.mode === "remote" && (
          <div className="flex flex-wrap items-center justify-end gap-3 px-6 py-2 text-type-body-sm text-ink-body">
            <span>Remote · {access.user?.role} · {access.user?.email}</span>
            {!access.capabilities.canOperate && <span>Read-only access</span>}
            <Button disabled={pending} onClick={() => void signOut()} size="touch" type="button" variant="secondary">Sign out</Button>
            {error && <span className="text-danger-text" role="alert">{error}</span>}
          </div>
        )}
        {children}
      </AccessContext.Provider>
    );
  }

  const remote = access?.authentication === "password";
  const hasLogin = remote || access?.authentication === "pin";
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!hasLogin || pending) return;
    const id = ++requestId.current;
    setPending(true);
    setError(null);
    try {
      if (remote) await loginRemote(email, password);
      else await loginDashboard(pin);
      const next = await getAccess();
      if (id === requestId.current) {
        accept(next);
        if (!next.authenticated) setError("Sign-in could not be confirmed. Please try again.");
      }
    } catch (cause) {
      if (id === requestId.current) setError(cause instanceof Error ? cause.message : "Sign in failed.");
    } finally {
      if (id === requestId.current) {
        setPassword("");
        setPin("");
        setPending(false);
      }
    }
  };

  return (
    <main className="grid min-h-screen place-items-center px-6 text-center">
      <Panel as="div" className="w-full max-w-sm p-7 backdrop-blur-xl">
        <form className="flex flex-col gap-4" onSubmit={(event) => void submit(event)}>
          <h1 className="select-none font-brand text-4xl leading-none text-ink sm:text-6xl lg:text-7xl">StagePilot</h1>
          {checking && <span className="loading-spinner-circular loading-spinner-circular--mobile-only" aria-hidden="true" />}
          <p className="text-type-body-md text-ink-body">
            {checking ? "Checking dashboard access…" : remote ? "Sign in to StagePilot Remote."
              : hasLogin ? "Enter the dashboard PIN to continue." : "Dashboard access is unavailable."}
          </p>
          {!checking && hasLogin && <>
            {remote ? <>
              <Field label="Email" className="text-left">
                <Input autoComplete="username" autoFocus disabled={pending}
                  maxLength={254} onChange={(event) => setEmail(event.target.value)} required type="email" value={email} />
              </Field>
              <Field label="Password" className="text-left" error={error}>
                <Input autoComplete="current-password" disabled={pending}
                  maxLength={1024} onChange={(event) => setPassword(event.target.value)} required type="password" value={password} />
              </Field>
            </> : <Field label="Dashboard PIN" className="text-left" error={error}>
              <Input autoComplete="current-password" autoFocus
                className="text-center font-mono text-xl tracking-[0.35em]" disabled={pending}
                inputMode="numeric" maxLength={64} onChange={(event) => setPin(event.target.value)} required type="password" value={pin} />
            </Field>}
            <Button disabled={pending || (remote ? !email || !password : pin.length < 4)} fullWidth size="touch" type="submit" variant="primary">
              {pending ? "Checking…" : remote ? "Sign in" : "Open dashboard"}
            </Button>
          </>}
          {!checking && !hasLogin && <Button onClick={() => void check()} size="touch" type="button" variant="secondary">Retry access check</Button>}
          {error && !(hasLogin && !checking) && <p aria-live="assertive" className="text-type-body-md text-danger-text">{error}</p>}
        </form>
      </Panel>
    </main>
  );
}
