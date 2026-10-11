import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { invalidateAccess, LOCAL_CAPABILITIES, NO_CAPABILITIES, setApiAccess } from "../access/accessState";
import { useDashboardAccess } from "../access/AccessContext";
import type { DashboardAccess } from "../types";

const mocks = vi.hoisted(() => ({ getAccess: vi.fn(), isDesktop: vi.fn(), login: vi.fn(), remoteLogin: vi.fn(), logout: vi.fn() }));
vi.mock("../api", () => ({ getAccess: mocks.getAccess, loginDashboard: mocks.login, loginRemote: mocks.remoteLogin, logoutRemote: mocks.logout }));
vi.mock("../desktop", () => ({ isDesktopShell: mocks.isDesktop }));

import { DashboardAccessGate } from "./DashboardAccessGate";

const lan: DashboardAccess = {
  mode: "lan", authentication: "pin", authenticated: false, capabilities: NO_CAPABILITIES,
  user: null, csrf_token: null, expires_at: null,
};
const remote: DashboardAccess = { ...lan, mode: "remote", authentication: "password" };
const viewer: DashboardAccess = {
  ...remote, authenticated: true, capabilities: { ...NO_CAPABILITIES, canRead: true },
  user: { email: "viewer@example.com", role: "Viewer" }, csrf_token: "test-csrf",
};
function Probe() {
  const access = useDashboardAccess();
  return <div>Dashboard · {access.capabilities.canConfigure ? "full" : "read-only"}</div>;
}
function renderGate() { return render(<DashboardAccessGate><Probe /></DashboardAccessGate>); }

beforeEach(() => {
  vi.resetAllMocks();
  mocks.isDesktop.mockReturnValue(false);
  mocks.getAccess.mockResolvedValue(lan);
});
afterEach(() => { setApiAccess(null); vi.useRealTimers(); });

it("keeps the LAN PIN gate and confirms capabilities with the backend after login", async () => {
  const user = userEvent.setup();
  mocks.login.mockResolvedValue({ authenticated: true });
  renderGate();
  await user.type(await screen.findByLabelText("Dashboard PIN"), "1234");
  mocks.getAccess.mockResolvedValue({ ...lan, authenticated: true, capabilities: LOCAL_CAPABILITIES });
  await user.click(screen.getByRole("button", { name: "Open dashboard" }));
  expect(mocks.login).toHaveBeenCalledWith("1234");
  expect(await screen.findByText("Dashboard · full")).toBeInTheDocument();
  expect(mocks.remoteLogin).not.toHaveBeenCalled();
});

it("skips the PIN UI when the backend disables protection", async () => {
  mocks.getAccess.mockResolvedValue({ ...lan, authentication: "none", authenticated: true, capabilities: LOCAL_CAPABILITIES });
  renderGate();
  expect(await screen.findByText("Dashboard · full")).toBeInTheDocument();
});

it("does not delay native desktop startup for a web access check", async () => {
  mocks.isDesktop.mockReturnValue(true);
  renderGate();
  expect(screen.getByText("Dashboard · full")).toBeInTheDocument();
  await waitFor(() => expect(mocks.getAccess).not.toHaveBeenCalled());
});

it("uses email/password only for backend-assigned remote access and receives Viewer capabilities", async () => {
  const user = userEvent.setup();
  mocks.getAccess.mockResolvedValue(remote);
  mocks.remoteLogin.mockResolvedValue({ authenticated: true, csrf_token: "test-csrf" });
  renderGate();
  await user.type(await screen.findByLabelText("Email"), "viewer@example.com");
  await user.type(screen.getByLabelText("Password"), "test-password");
  expect(screen.queryByLabelText("Dashboard PIN")).not.toBeInTheDocument();
  mocks.getAccess.mockResolvedValue(viewer);
  await user.click(screen.getByRole("button", { name: "Sign in" }));
  expect(mocks.remoteLogin).toHaveBeenCalledWith("viewer@example.com", "test-password");
  expect(mocks.login).not.toHaveBeenCalled();
  expect(await screen.findByText("Dashboard · read-only")).toBeInTheDocument();
  expect(window.localStorage.getItem("stagepilot.remote-session")).toBeNull();
});

it("unmounts privileged content and returns to remote login on rejected access", async () => {
  mocks.getAccess.mockResolvedValue({ ...viewer, capabilities: { ...LOCAL_CAPABILITIES, canActivateServices: false } });
  renderGate();
  await screen.findByText("Dashboard · full");
  act(() => invalidateAccess());
  expect(screen.queryByText("Dashboard · full")).not.toBeInTheDocument();
  expect(screen.getByLabelText("Email")).toBeInTheDocument();
  expect(screen.queryByLabelText("Dashboard PIN")).not.toBeInTheDocument();
  expect(mocks.getAccess).toHaveBeenCalledOnce();
});

it("returns a rejected LAN session to its PIN gate", async () => {
  mocks.getAccess.mockResolvedValue({ ...lan, authenticated: true, capabilities: LOCAL_CAPABILITIES });
  renderGate();
  await screen.findByText("Dashboard · full");
  act(() => invalidateAccess());
  expect(screen.getByLabelText("Dashboard PIN")).toBeInTheDocument();
});

it("offers retry instead of guessing a login method when discovery fails", async () => {
  const user = userEvent.setup();
  mocks.getAccess.mockRejectedValue(new Error("Backend unavailable"));
  renderGate();
  const retry = await screen.findByRole("button", { name: "Retry access check" });
  expect(screen.queryByLabelText("Dashboard PIN")).not.toBeInTheDocument();
  expect(screen.queryByLabelText("Password")).not.toBeInTheDocument();
  mocks.getAccess.mockResolvedValue(remote);
  await user.click(retry);
  expect(await screen.findByLabelText("Email")).toBeInTheDocument();
});

it("keeps failed remote login inline and clears the password field", async () => {
  const user = userEvent.setup();
  mocks.getAccess.mockResolvedValue(remote);
  mocks.remoteLogin.mockRejectedValue(new Error("Too many login attempts. Try again later."));
  renderGate();
  await user.type(await screen.findByLabelText("Email"), "viewer@example.com");
  await user.type(screen.getByLabelText("Password"), "wrong-password");
  await user.click(screen.getByRole("button", { name: "Sign in" }));
  expect(await screen.findByText(/Too many login attempts/)).toBeInTheDocument();
  expect(screen.getByLabelText("Password")).toHaveValue("");
  expect(screen.queryByText("Dashboard · read-only")).not.toBeInTheDocument();
});

it("signs out through the remote endpoint and clears dashboard content", async () => {
  const user = userEvent.setup();
  mocks.getAccess.mockResolvedValue(viewer);
  mocks.logout.mockResolvedValue(undefined);
  renderGate();
  await user.click(await screen.findByRole("button", { name: "Sign out" }));
  expect(mocks.logout).toHaveBeenCalledOnce();
  expect(await screen.findByLabelText("Email")).toBeInTheDocument();
  expect(screen.queryByText("Dashboard · read-only")).not.toBeInTheDocument();
});

it("returns to the gate at the backend session expiry without reconnecting", async () => {
  vi.useFakeTimers();
  mocks.getAccess.mockResolvedValue({ ...viewer, expires_at: (Date.now() + 5_000) / 1000 });
  renderGate();
  await act(async () => { await Promise.resolve(); });
  expect(screen.getByText("Dashboard · read-only")).toBeInTheDocument();
  act(() => vi.advanceTimersByTime(5_001));
  expect(screen.getByLabelText("Email")).toBeInTheDocument();
  expect(mocks.getAccess).toHaveBeenCalledOnce();
});

describe("fits narrow (mobile/tablet) viewports, matching the App startup screen fix", () => {
  const originalWidth = window.innerWidth;

  beforeEach(() => {
    Object.defineProperty(window, "innerWidth", { value: 375, configurable: true, writable: true });
  });

  afterEach(() => {
    Object.defineProperty(window, "innerWidth", { value: originalWidth, configurable: true, writable: true });
  });

  it("renders the wordmark with a responsive size class instead of a fixed size", async () => {
    renderGate();
    const heading = await screen.findByRole("heading", { name: "StagePilot" });
    expect(heading.className.split(/\s+/)).not.toContain("text-7xl");
    expect(heading.className).toMatch(/\bsm:text-\S+\b/);
    expect(heading.className).toMatch(/\blg:text-\S+\b/);
  });

  it("shows the mobile-only circular spinner while the access check is in flight", async () => {
    renderGate();
    const spinner = document.querySelector(".loading-spinner-circular");
    expect(spinner).not.toBeNull();
    expect(spinner?.className).toContain("loading-spinner-circular--mobile-only");
    await screen.findByLabelText("Dashboard PIN");
    expect(document.querySelector(".loading-spinner-circular")).toBeNull();
  });
});

it("shows a failed remote sign-in under the password field, announced, with one primary button", async () => {
  const user = userEvent.setup();
  mocks.getAccess.mockResolvedValue(remote);
  mocks.remoteLogin.mockRejectedValue(new Error("Too many login attempts. Try again later."));
  renderGate();
  await user.type(await screen.findByLabelText("Email"), "viewer@example.com");
  await user.type(screen.getByLabelText("Password"), "wrong-password");
  await user.click(screen.getByRole("button", { name: "Sign in" }));
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("Too many login attempts. Try again later.");
  const password = screen.getByLabelText("Password");
  expect(password).toHaveAttribute("aria-invalid", "true");
  expect(password.getAttribute("aria-describedby")).toContain(alert.id);
  expect(alert.closest("[data-field]")).toContainElement(password);
  expect(screen.getAllByRole("alert")).toHaveLength(1);
  expect(screen.getAllByRole("button")).toHaveLength(1);
});

it("shows a failed PIN under the PIN field and keeps one primary button", async () => {
  const user = userEvent.setup();
  mocks.getAccess.mockResolvedValue({ ...remote, authentication: "pin" });
  mocks.login.mockRejectedValue(new Error("Incorrect dashboard PIN."));
  renderGate();
  await user.type(await screen.findByLabelText("Dashboard PIN"), "1234");
  await user.click(screen.getByRole("button", { name: "Open dashboard" }));
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("Incorrect dashboard PIN.");
  const pin = screen.getByLabelText("Dashboard PIN");
  expect(pin).toHaveAttribute("aria-invalid", "true");
  expect(pin.getAttribute("aria-describedby")).toContain(alert.id);
  expect(screen.getAllByRole("button")).toHaveLength(1);
});
