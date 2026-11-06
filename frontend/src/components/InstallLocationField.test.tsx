import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { InstallLocationField } from "./InstallLocationField";

vi.mock("../api", async (original) => ({
  ...await original<typeof import("../api")>(),
  getInstallLocation: vi.fn(), setInstallLocation: vi.fn(),
}));

function stubGeolocation(latitude: number, longitude: number) {
  vi.stubGlobal("navigator", { geolocation: {
    getCurrentPosition: (ok: PositionCallback) => ok({ coords: { latitude, longitude } } as GeolocationPosition),
  } });
}

beforeEach(() => {
  vi.mocked(api.getInstallLocation).mockResolvedValue({ available: true, location: null });
  vi.mocked(api.setInstallLocation).mockImplementation(async (location) => ({ available: true, location: location || null }));
});
afterEach(() => { vi.unstubAllGlobals(); vi.clearAllMocks(); });

describe("InstallLocationField", () => {
  it("renders the saved location and saves a typed one", async () => {
    vi.mocked(api.getInstallLocation).mockResolvedValue({ available: true, location: "Austin, TX" });
    render(<InstallLocationField allowDetect />);
    const input = await screen.findByRole("textbox", { name: /Location/ });
    expect(input).toHaveValue("Austin, TX");
    fireEvent.change(input, { target: { value: "  Pasadena, CA " } });
    fireEvent.click(screen.getByRole("button", { name: "Save location" }));
    await waitFor(() => expect(api.setInstallLocation).toHaveBeenCalledWith("Pasadena, CA"));
    expect(await screen.findByText("Location saved.")).toBeInTheDocument();
  });

  it("renders nothing when the installation cannot store a location", async () => {
    vi.mocked(api.getInstallLocation).mockResolvedValue({ available: false, location: null });
    const { container } = render(<InstallLocationField allowDetect />);
    await waitFor(() => expect(api.getInstallLocation).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });

  it("hides Detect when geolocation is unavailable or this is a remote session", async () => {
    vi.stubGlobal("navigator", {});
    const { unmount } = render(<InstallLocationField allowDetect />);
    await screen.findByRole("textbox", { name: /Location/ });
    expect(screen.queryByRole("button", { name: /Detect my location/ })).not.toBeInTheDocument();
    unmount();
    stubGeolocation(34.1478, -118.1445);
    render(<InstallLocationField allowDetect={false} />);
    await screen.findByRole("textbox", { name: /Location/ });
    expect(screen.queryByRole("button", { name: /Detect my location/ })).not.toBeInTheDocument();
  });

  it("detection fills the field, sends nothing until Save, and never sends coordinates", async () => {
    stubGeolocation(34.1478, -118.1445);
    const fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
    render(<InstallLocationField allowDetect />);
    fireEvent.click(await screen.findByRole("button", { name: "Detect my location" }));
    await waitFor(() => expect(screen.getByRole("textbox", { name: /Location/ })).toHaveValue("Pasadena, CA"));
    expect(api.setInstallLocation).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Save location" }));
    await waitFor(() => expect(api.setInstallLocation).toHaveBeenCalledWith("Pasadena, CA"));
    // Only the city string ever reaches the API layer; the webview never fetches with coordinates.
    expect(JSON.stringify(vi.mocked(api.setInstallLocation).mock.calls)).not.toMatch(/34\.1|118\.1/);
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("explains a declined permission and keeps manual entry", async () => {
    vi.stubGlobal("navigator", { geolocation: {
      getCurrentPosition: (_ok: PositionCallback, fail: PositionErrorCallback) => fail({ code: 1 } as GeolocationPositionError),
    } });
    render(<InstallLocationField allowDetect />);
    fireEvent.click(await screen.findByRole("button", { name: "Detect my location" }));
    expect(await screen.findByText(/declined/)).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /Location/ })).toBeEnabled();
  });
});
