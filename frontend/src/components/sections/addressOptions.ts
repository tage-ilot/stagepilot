import type { ScanState } from "./scanState";

/** One computer a scan found, as the address box lists it. `label` already reads "Name (address)" when a name is known. */
export interface ScannedAddress {
  host: string;
  label: string;
  /** The API port it answered on (ProPresenter), told to the block's `onPick` so it can fill its port box too. */
  port?: number;
}

export interface AddressOption {
  host: string;
  /** What the row says. */
  text: string;
  group: "scanned" | "recent";
  port?: number;
}

export interface AddressGroups { scanned: AddressOption[]; recent: AddressOption[] }

/** The most addresses the Recent group shows (the same five `useRecentAddresses` keeps). */
export const MAX_RECENT_SHOWN = 5;

export const sameAddress = (a: string, b: string): boolean => a.trim().toLowerCase() === b.trim().toLowerCase();

/**
 * The two groups of the address list, nothing else. Scanned: the candidates of the last scan, in the order the scan gave them. Recent: the
 * addresses that worked, newest first, at most five. An address that is in both is shown once, under Scanned; an address is never listed twice.
 */
export function addressGroups(scanned: readonly ScannedAddress[], recent: readonly string[]): AddressGroups {
  const seen = new Set<string>();
  const first = (host: string) => { const key = host.trim().toLowerCase(); if (!key || seen.has(key)) return false; seen.add(key); return true; };
  const scannedOptions = scanned.filter((entry) => first(entry.host)).map((entry): AddressOption => ({ host: entry.host.trim(), text: entry.label || entry.host, group: "scanned", port: entry.port }));
  const recentOptions = recent.slice(0, MAX_RECENT_SHOWN).filter(first).map((host): AddressOption => ({ host, text: host, group: "recent" }));
  return { scanned: scannedOptions, recent: recentOptions };
}

/**
 * What a scan offers the address box: the candidates of a scan that FOUND something, labelled "Name (address)" when the scan knows a name.
 * Nothing while it is still scanning, found nothing, or only tried the one address that was typed (that is Connect, not a scan): the box lists a
 * scan only while its result is the latest one.
 */
export function scannedAddresses(scan: ScanState): ScannedAddress[] {
  if (scan.state !== "found" || scan.typed) return [];
  return scan.candidates.map((candidate) => ({
    host: candidate.host,
    label: candidate.label && candidate.label !== candidate.host ? `${candidate.label} (${candidate.host})` : candidate.host,
    port: candidate.port,
  }));
}
