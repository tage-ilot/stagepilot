import { afterEach, describe, expect, it, vi } from "vitest";

import { detectCity, formatCity, geolocationAvailable, loadCities, nearestCity, parseCities } from "./cityLookup";

describe("offline nearest city", () => {
  it.each([
    ["Pasadena, CA", 34.1478, -118.1445],
    ["Austin, TX", 30.2672, -97.7431],
    ["Denver, CO", 39.7392, -104.9903],
    ["Seattle, WA", 47.6062, -122.3321],
    ["Boulder, CO", 40.015, -105.2705],
    ["Toronto, Canada", 43.6532, -79.3832],
    ["London, United Kingdom", 51.5074, -0.1278],
  ])("finds %s", async (expected, lat, lon) => {
    expect(nearestCity(lat, lon, await loadCities())).toBe(expected);
  });

  it("picks the nearby small town, not the far larger city", async () => {
    // South Pasadena sits about 3 km from Los Angeles' centre point but is the closer named place here.
    expect(nearestCity(34.1161, -118.1503, await loadCities())).toBe("South Pasadena, CA");
    const fixture = parseCities("Bigville|US|CA|34.00|-118.00\nSmalltown|US|CA|34.20|-118.00");
    expect(nearestCity(34.19, -118.0, fixture)).toBe("Smalltown, CA");
  });

  it("returns null in the open ocean and for invalid input", async () => {
    const cities = await loadCities();
    expect(nearestCity(0, -30, cities)).toBeNull();
    expect(nearestCity(Number.NaN, 0, cities)).toBeNull();
    expect(nearestCity(95, 0, cities)).toBeNull();
  });

  it("formats US cities with the state and others with the country", () => {
    expect(formatCity({ name: "Paris", country: "FR", state: "", lat: 0, lon: 0 })).toBe("Paris, France");
    expect(formatCity({ name: "Reno", country: "US", state: "NV", lat: 0, lon: 0 })).toBe("Reno, NV");
  });

  it("ships a compact dataset", async () => {
    expect((await loadCities()).length).toBeGreaterThan(30000);
  });
});

describe("detectCity", () => {
  afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

  it("reports unavailable when the webview has no geolocation", async () => {
    vi.stubGlobal("navigator", {});
    expect(geolocationAvailable()).toBe(false);
    await expect(detectCity()).rejects.toThrow("unavailable");
  });

  it("turns coordinates into a city with no network calls", async () => {
    const fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
    vi.stubGlobal("navigator", { geolocation: {
      getCurrentPosition: (ok: PositionCallback) => ok({ coords: { latitude: 34.1478, longitude: -118.1445 } } as GeolocationPosition),
    } });
    await expect(detectCity()).resolves.toBe("Pasadena, CA");
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("maps a declined permission", async () => {
    vi.stubGlobal("navigator", { geolocation: {
      getCurrentPosition: (_ok: PositionCallback, fail: PositionErrorCallback) => fail({ code: 1 } as GeolocationPositionError),
    } });
    await expect(detectCity()).rejects.toThrow("denied");
  });
});
