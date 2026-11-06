/**
 * Offline coordinates -> "City, ST" lookup. The dataset (frontend/data/cities.txt, GeoNames cities15000, CC BY 4.0)
 * ships inside the app and is loaded only when the user presses "Detect my location"; nothing here touches the network.
 */

export interface City { name: string; country: string; state: string; lat: number; lon: number }

/** Farther than this from every known city is treated as "no answer" (open ocean, polar regions). */
export const MAX_CITY_DISTANCE_KM = 100;

let cache: Promise<City[]> | null = null;

export function parseCities(text: string): City[] {
  const cities: City[] = [];
  for (const line of text.split("\n")) {
    const [name, country, state, lat, lon] = line.split("|");
    if (!name || !country || lon === undefined) continue;
    cities.push({ name, country, state: state ?? "", lat: Number(lat), lon: Number(lon) });
  }
  return cities;
}

export function loadCities(): Promise<City[]> {
  cache ??= import("../../data/cities.txt?raw").then((module) => parseCities(module.default));
  return cache;
}

function haversineKm(lat1: number, lon1: number, lat2: number, lon2: number): number {
  const rad = Math.PI / 180;
  const dLat = (lat2 - lat1) * rad;
  const dLon = (lon2 - lon1) * rad;
  const a = Math.sin(dLat / 2) ** 2 + Math.cos(lat1 * rad) * Math.cos(lat2 * rad) * Math.sin(dLon / 2) ** 2;
  return 12742 * Math.asin(Math.min(1, Math.sqrt(a)));
}

function countryName(code: string): string {
  try {
    return new Intl.DisplayNames(["en"], { type: "region" }).of(code) ?? code;
  } catch {
    return code;
  }
}

/** "Pasadena, CA" for the US, "Toronto, Canada" elsewhere. */
export function formatCity(city: City): string {
  return city.country === "US" && city.state ? `${city.name}, ${city.state}` : `${city.name}, ${countryName(city.country)}`;
}

/** The closest known city (by distance, never by size), or null when none is within MAX_CITY_DISTANCE_KM. */
export function nearestCity(lat: number, lon: number, cities: City[]): string | null {
  if (!Number.isFinite(lat) || !Number.isFinite(lon) || Math.abs(lat) > 90 || Math.abs(lon) > 180) return null;
  let best: City | null = null;
  let bestKm = Infinity;
  for (const city of cities) {
    // Cheap latitude prefilter (1 degree is about 111 km) before the trig.
    if (Math.abs(city.lat - lat) > 2) continue;
    const km = haversineKm(lat, lon, city.lat, city.lon);
    if (km < bestKm) { best = city; bestKm = km; }
  }
  return best && bestKm <= MAX_CITY_DISTANCE_KM ? formatCity(best) : null;
}

/** True only where the webview exposes geolocation; the Detect button is hidden otherwise. */
export function geolocationAvailable(): boolean {
  return typeof navigator !== "undefined" && "geolocation" in navigator && Boolean(navigator.geolocation);
}

/**
 * Asks the webview for the position (the OS shows its own permission prompt), converts it to a city name on this
 * machine, and returns only that text. The coordinates live in this function's scope and are never stored or sent.
 */
export function detectCity(): Promise<string> {
  return new Promise((resolve, reject) => {
    if (!geolocationAvailable()) { reject(new Error("unavailable")); return; }
    navigator.geolocation.getCurrentPosition(
      (position) => {
        loadCities().then((cities) => {
          const city = nearestCity(position.coords.latitude, position.coords.longitude, cities);
          if (city) resolve(city); else reject(new Error("no-city"));
        }, () => reject(new Error("no-city")));
      },
      (error) => reject(new Error(error.code === 1 ? "denied" : "failed")),
      { enableHighAccuracy: false, timeout: 15000, maximumAge: 600000 },
    );
  });
}
