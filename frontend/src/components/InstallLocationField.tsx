import { useEffect, useState } from "react";

import { getInstallLocation, setInstallLocation } from "../api";
import { detectCity, geolocationAvailable } from "../lib/cityLookup";
import { Button } from "./ui/Button";
import { Field } from "./ui/Field";
import { Input } from "./ui/Input";

const detectErrors: Record<string, string> = {
  denied: "Location access was declined. Type your city instead.",
  "no-city": "Couldn't find a nearby city. Type your city instead.",
};

/**
 * "City, ST" this installation reports to the StagePilot team so the fleet page shows the right place (the network
 * address alone often points at the ISP's city). Typed, or filled in by "Detect my location": the coordinates are
 * turned into a city name on this computer and are never saved or sent.
 */
export function InstallLocationField({ allowDetect }: { allowDetect: boolean }) {
  const [available, setAvailable] = useState(false);
  const [saved, setSaved] = useState("");
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [detecting, setDetecting] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    getInstallLocation().then((next) => {
      if (cancelled) return;
      setAvailable(next.available);
      setSaved(next.location ?? "");
      setValue(next.location ?? "");
    }).catch(() => undefined);
    return () => { cancelled = true; };
  }, []);

  if (!available) return null;
  const held = value.trim() !== saved;
  const canDetect = allowDetect && geolocationAvailable();

  const save = async () => {
    setBusy(true); setMessage(null); setFailed(false);
    try {
      const next = await setInstallLocation(value.trim());
      setSaved(next.location ?? ""); setValue(next.location ?? "");
      setMessage(next.location ? "Location saved." : "Location cleared.");
    } catch {
      setFailed(true); setMessage("Couldn't save the location. Use a short city name (80 characters or fewer).");
    } finally { setBusy(false); }
  };
  const detect = async () => {
    setDetecting(true); setMessage(null); setFailed(false);
    try {
      setValue(await detectCity());
      setMessage("Detected. Check it, then press Save location.");
    } catch (cause) {
      setFailed(true);
      setMessage(detectErrors[cause instanceof Error ? cause.message : ""] ?? "Couldn't detect your location. Type your city instead.");
    } finally { setDetecting(false); }
  };

  return (
    <div className="mt-4" data-install-location>
      <Field
        heldChange={held}
        hint="Shown to the StagePilot team to tell installations apart. Only this city name is sent; coordinates never leave this computer."
        label="Location"
      >
        <div className="flex flex-wrap items-center gap-2">
          <Input
            disabled={busy}
            maxLength={80}
            onChange={(event) => setValue(event.target.value)}
            onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); if (held && !busy) void save(); } }}
            placeholder="City, ST"
            value={value}
          />
          <Button disabled={busy || !held} onClick={() => void save()} type="button" variant="secondary">Save location</Button>
          {canDetect && (
            <Button disabled={busy || detecting} onClick={() => void detect()} type="button" variant="secondary">
              {detecting ? "Detecting…" : "Detect my location"}
            </Button>
          )}
        </div>
      </Field>
      {message && <p className={`mt-2 text-xs ${failed ? "text-warning-text" : "text-ink-muted"}`} role="status">{message}</p>}
    </div>
  );
}
