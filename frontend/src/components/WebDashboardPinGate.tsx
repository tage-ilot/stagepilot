import { useEffect, useState, type FormEvent, type ReactNode } from "react";

import { getDashboardAuthStatus, loginDashboard } from "../api";
import { isDesktopShell } from "../desktop";
import { Button, Field, Input, Panel } from "./ui";

export function WebDashboardPinGate({ children }: { children: ReactNode }) {
  const [authorized, setAuthorized] = useState(isDesktopShell());
  const [checking, setChecking] = useState(!isDesktopShell());
  const [pin, setPin] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  useEffect(() => {
    if (isDesktopShell()) return;
    let active = true;
    void getDashboardAuthStatus()
      .then((status) => {
        if (active) setAuthorized(status.authenticated || !status.required);
      })
      .catch((cause) => {
        if (active) {
          setError(cause instanceof Error ? cause.message : "Dashboard access check failed.");
        }
      })
      .finally(() => {
        if (active) setChecking(false);
      });
    return () => {
      active = false;
    };
  }, []);

  if (authorized) return children;

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setPending(true);
    setError(null);
    try {
      const status = await loginDashboard(pin);
      if (status.authenticated) setAuthorized(true);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Incorrect dashboard PIN.");
    } finally {
      setPending(false);
    }
  };

  return (
    <main className="grid min-h-screen place-items-center px-6 text-center">
      <Panel as="div" className="w-full max-w-sm p-7 backdrop-blur-xl">
        <form className="flex flex-col gap-4" onSubmit={(event) => void submit(event)}>
          <h1 className="select-none font-brand text-4xl leading-none text-ink sm:text-6xl lg:text-7xl">StagePilot</h1>
          {checking && <span className="loading-spinner-circular loading-spinner-circular--mobile-only" aria-hidden="true" />}
          <p className="text-type-body-md text-ink-body">
            {checking ? "Checking dashboard access…" : "Enter the dashboard PIN to continue."}
          </p>
          {!checking && (
            <>
              <Field label="Dashboard PIN" className="text-left" error={error}>
                <Input
                  autoComplete="current-password"
                  autoFocus
                  className="text-center font-mono text-xl tracking-[0.35em]"
                  disabled={pending}
                  inputMode="numeric"
                  maxLength={64}
                  onChange={(event) => setPin(event.target.value)}
                  required
                  type="password"
                  value={pin}
                />
              </Field>
              <Button disabled={pending || pin.length < 4} fullWidth size="touch" type="submit" variant="primary">
                {pending ? "Checking…" : "Open dashboard"}
              </Button>
            </>
          )}
        </form>
      </Panel>
    </main>
  );
}
