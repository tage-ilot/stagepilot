import { useEffect, useRef, useState } from "react";
import { chooseLengthCategory, confirmPlanLengths, getLengthCategories, previewPlanLengths, restorePlanLengths } from "../api";
import { Dialog } from "./ui/Dialog";
import type { LengthPreview, PlaybackStatusResponse, PlanningCenterServiceType } from "../types";

const button = "min-h-ds-touch-target touch-manipulation rounded-ds-lg border border-integration-services-soft/30 bg-integration-services-soft/10 px-3 py-2 text-sm font-semibold text-integration-services-text transition-colors duration-[140ms] motion-reduce:transition-none can-hover:bg-integration-services-soft/20 disabled:cursor-not-allowed disabled:border-edge disabled:bg-surface-900 disabled:text-ink-muted";
const duration = (n: number | null) => n === null ? "Unknown" : `${Math.floor(n / 60)}:${String(n % 60).padStart(2, "0")}`;
export function lengthUpdateReason(status: PlaybackStatusResponse): string | null {
  if (!status.planning_center_connected) return "Connect Services first.";
  if (status.playing) return "Stop Playback first.";
  if (status.discovery === "failed") return "The song scan failed or was cancelled. Scan songs again.";
  if (status.discovery === "running") return "Wait for the song scan to finish.";
  if (!status.captured_at || !status.song_order.length) return "Scan songs first to read the lengths from Playback.";
  if (status.stale) return "The setlist changed. Scan songs again.";
  if (!(status.song_lengths ?? []).some(n => n !== null)) return "The scan has no song lengths. Scan songs again.";
  return status.planning_center_update_reason ?? null;
}

export function PlanningCenterLengths({ status, onStatus }: { status: PlaybackStatusResponse; onStatus?: (status: PlaybackStatusResponse) => void }) {
  const [categories, setCategories] = useState<PlanningCenterServiceType[]>([]);
  const [open, setOpen] = useState(false);
  const [selected, setSelected] = useState(status.planning_center_update_service_type_id);
  const [preview, setPreview] = useState<LengthPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<string | null>(null);
  const [undo, setUndo] = useState(status.planning_center_undo_available ?? false);
  const main = useRef<HTMLButtonElement>(null);
  const arrow = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const generation = useRef(0);
  const returnFocus = useRef<"arrow" | "main" | null>(null);
  useEffect(() => { if (!busy && returnFocus.current) { (returnFocus.current === "arrow" ? arrow : main).current?.focus(); returnFocus.current = null; } }, [busy, open, preview]);
  const reason = lengthUpdateReason(status);
  const fingerprint = `${status.captured_at}|${status.stale}|${status.discovery}|${status.playing}|${status.planning_center_connected}`;
  useEffect(() => { generation.current++; setPreview(null); }, [fingerprint]);
  useEffect(() => () => { generation.current++; }, []);
  useEffect(() => setSelected(status.planning_center_update_service_type_id), [status.planning_center_update_service_type_id]);
  useEffect(() => setUndo(status.planning_center_undo_available ?? false), [status.planning_center_undo_available]);
  useEffect(() => { if (open) menu.current?.querySelector<HTMLButtonElement>("button")?.focus(); }, [open, categories]);
  useEffect(() => {
    if (!open) return;
    const outside = (e: PointerEvent) => { if (!menu.current?.contains(e.target as Node) && !arrow.current?.contains(e.target as Node)) setOpen(false); };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);
  const close = () => { setPreview(null); returnFocus.current = "main"; if (!busy) main.current?.focus(); };
  const run = async (action: () => Promise<void>) => {
    if (busy) return;
    setBusy(true); setError(null);
    try { await action(); } catch (cause) { setError(cause instanceof Error ? cause.message : "Services could not complete this change. Try again later."); }
    finally { setBusy(false); }
  };
  const accept = (next: PlaybackStatusResponse) => { setResult(next.planning_center_lengths?.message ?? null); setUndo(next.planning_center_undo_available ?? false); onStatus?.(next); };
  const showPreview = () => void run(async () => {
    const current = generation.current;
    const next = await previewPlanLengths();
    if (current === generation.current) { setPreview(next); }
  });
  return <div className="min-w-0 space-y-2" aria-label="Services song times" role="group">
    <div className="relative flex w-fit max-w-full">
      <button ref={main} className={`${button} rounded-r-none`} disabled={!!reason || busy} aria-disabled={!!reason || busy} aria-describedby="pc-length-reason" onClick={showPreview}>Update Services</button>
      <button ref={arrow} className={`${button} rounded-l-none border-l-0 coarse:min-w-ds-touch-target`} aria-label="Choose service plan category" aria-haspopup="menu" aria-expanded={open} disabled={!status.planning_center_connected || busy || status.discovery === "running"} onKeyDown={e => { if (e.key === "ArrowDown") { e.preventDefault(); arrow.current?.click(); } }} onClick={() => void run(async () => { if (open) { setOpen(false); return; } setCategories(await getLengthCategories()); setOpen(true); })}>▾</button>
      {open && <div ref={menu} role="menu" aria-label="Service plan categories" className="absolute left-0 top-full z-20 mt-1 w-64 max-w-full rounded-ds-lg border border-edge-medium bg-surface-950 p-1 shadow-elevation-overlay" onKeyDown={e => {
        const items = [...menu.current!.querySelectorAll<HTMLButtonElement>("button")];
        const index = items.indexOf(document.activeElement as HTMLButtonElement);
        if (e.key === "Escape") { e.preventDefault(); setOpen(false); arrow.current?.focus(); }
        else if (["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) { e.preventDefault(); items[e.key === "Home" ? 0 : e.key === "End" ? items.length - 1 : (index + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length]?.focus(); }
        else if (e.key === "Tab") setOpen(false);
      }}>{categories.map(c => <button key={c.id} role="menuitemradio" aria-checked={selected === c.id} tabIndex={-1} className="block min-h-ds-touch-target w-full touch-manipulation break-words rounded-ds-lg p-2 text-left text-sm text-ink can-hover:bg-integration-services-soft/20 focus:bg-integration-services-soft/20" onClick={() => void run(async () => { const next = await chooseLengthCategory(c.id); setSelected(c.id); setPreview(null); onStatus?.(next); setOpen(false); returnFocus.current = "arrow"; })}>{selected === c.id ? "✓ " : ""}{c.name}</button>)}</div>}
    </div>
    <p id="pc-length-reason" className="break-words text-xs text-ink-muted">{busy ? "Please wait…" : reason ?? "Preview the next upcoming service plan before changing its song times."}</p>
    {error && <p role="alert" className="break-words text-sm text-danger-text">{error}</p>}
    {(result || status.planning_center_lengths?.message) && <p role="status" className="break-words text-sm text-ink-body">{result || status.planning_center_lengths?.message}</p>}
    {undo && <button className={button} disabled={busy || status.playing || status.discovery === "running" || !status.planning_center_connected} onClick={() => void run(async () => accept(await restorePlanLengths()))}>Restore Services times</button>}
    <Dialog
      open={!!preview}
      dismissible={!busy}
      onClose={close}
      title="Update Services?"
      description={preview ? `${preview.category} · ${preview.plan_title} · ${preview.plan_date}` : undefined}
      actions={preview && <>
        {preview.items.some(r => r.status === "updated") && <button className={button} disabled={busy || !!reason} onClick={() => void run(async () => { accept(await confirmPlanLengths(preview.token)); close(); })}>Update Services</button>}
        <button className={button} disabled={busy} onClick={close}>Cancel</button>
      </>}
    >{preview && <>
      <ul className="divide-y divide-edge-subtle text-sm text-ink-body">{preview.items.map(r => <li key={r.item_id} className="break-words py-2"><strong>{r.title}</strong> {r.status === "skipped" ? `No change — ${r.reason}` : `${duration(r.old_length)} → ${duration(r.new_length)}`}</li>)}</ul>
      {preview.message && <p className="text-sm text-ink-body">{preview.message}</p>}
      {error && <p role="alert" className="text-sm text-danger-text">{error}</p>}
    </>}</Dialog>
  </div>;
}
