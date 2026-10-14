import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AccessContext } from "../access/AccessContext";
import { DESKTOP_ACCESS } from "../access/accessState";
import * as api from "../api";
import * as desktop from "../desktop";
import { useRemoteAccess } from "../hooks/useRemoteAccess";
import { RemoteAccessPanel } from "./RemoteAccessPanel";

vi.mock("../api", async (original) => ({
  ...await original<typeof import("../api")>(),
  getRemoteStatus: vi.fn(), getRemoteUsers: vi.fn(), setRemoteEnabled: vi.fn(),
  bootstrapRemote: vi.fn(), createRemoteUser: vi.fn(),
  updateRemoteUser: vi.fn(), deleteRemoteUser: vi.fn(), regenerateRemote: vi.fn(),
  resetRemoteIdentity: vi.fn(),
}));
vi.mock("../desktop", async (original) => ({
  ...await original<typeof import("../desktop")>(),
  setRemoteAutostart: vi.fn(),
}));
const off: api.RemoteStatus = {available: true, provisioned: true, credential_available: true, enabled: false, state: "off", url: null, needs_operator: true, message: null, temporary_url: true, permanently_revoked: false};
const operator: api.RemoteUser = {id: "one", email: "operator@example.test", role: "Operator", enabled: true};
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getRemoteStatus).mockResolvedValue(off);
  vi.mocked(api.getRemoteUsers).mockResolvedValue([]);
  vi.mocked(desktop.setRemoteAutostart).mockResolvedValue();
});

/** Renders RemoteAccessPanel wired to the real useRemoteAccess hook, the
 * same way BackendSetupPanel does. Enable/disable/bootstrap lifecycle
 * behavior itself is covered end-to-end via the checkbox in
 * BackendSetupPanel.test.tsx; these tests cover status rendering, link
 * management, and user management that RemoteAccessPanel still owns. */
function Harness() {
  const control = useRemoteAccess();
  return <RemoteAccessPanel control={control} />;
}

describe("Remote Access", () => {
  it("shows only safe connected URL and protects last Operator", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue({...off, enabled: true, state: "connected", needs_operator: false, url: "https://test.trycloudflare.com"});
    vi.mocked(api.getRemoteUsers).mockResolvedValue([operator]);
    render(<Harness />);
    expect(await screen.findByLabelText("Remote URL")).toHaveTextContent("https://test.trycloudflare.com");
    expect(screen.getByText(/Temporary Remote link/)).toBeInTheDocument();
    expect(screen.getByRole("button", {name: `Delete ${operator.email}`})).toBeDisabled();
    expect(screen.getByRole("combobox", {name: `Role for ${operator.email}`})).toBeDisabled();
  });

  it("shows a stable-link label for named Remote", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue({...off, enabled: true, state: "connected", needs_operator: false, url: "https://remote.example.test", temporary_url: false});
    render(<Harness />);
    expect(await screen.findByText(/Stable Remote link/)).toBeInTheDocument();
    expect(screen.queryByText(/Temporary Remote link/)).not.toBeInTheDocument();
  });

  it("confirms password replacement and explains last-Operator protection", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue({...off, needs_operator: false});
    vi.mocked(api.getRemoteUsers).mockResolvedValue([operator]);
    vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
    render(<Harness />);
    expect(await screen.findByText(/Last Operator protection is active/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", {name: `Change password for ${operator.email}`}));
    fireEvent.change(screen.getByLabelText(`New password for ${operator.email}`), {target: {value: "replacement-password"}});
    fireEvent.click(screen.getByRole("button", {name: "Save password"}));
    expect(api.updateRemoteUser).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", {name: "Save password"}));
    await waitFor(() => expect(api.updateRemoteUser).toHaveBeenCalledWith(operator.id, {password: "replacement-password"}));
  });

  describe("Add a person", () => {
    const connected = {...off, enabled: true, state: "connected" as const, needs_operator: false, url: "https://test.trycloudflare.com"};
    const setup = () => {
      vi.mocked(api.getRemoteStatus).mockResolvedValue(connected);
      vi.mocked(api.getRemoteUsers).mockResolvedValue([operator]);
    };

    it("keeps the sign-in fields folded away until the button is pressed", async () => {
      setup();
      render(<Harness />);
      expect(await screen.findByRole("button", {name: "Add a person"})).toBeEnabled();
      expect(screen.queryByLabelText("Email")).not.toBeInTheDocument();
      expect(screen.queryByLabelText("Password")).not.toBeInTheDocument();
      expect(screen.queryByRole("button", {name: "Add user"})).not.toBeInTheDocument();
    });

    it("opens the email, password and role fields, and hides the button while they are open", async () => {
      setup();
      render(<Harness />);
      fireEvent.click(await screen.findByRole("button", {name: "Add a person"}));
      expect(screen.getByLabelText("Email")).toBeVisible();
      expect(screen.getByLabelText("Password")).toBeVisible();
      expect(screen.getByLabelText("New user role")).toBeVisible();
      expect(screen.getByRole("button", {name: "Add user"})).toBeInTheDocument();
      expect(screen.queryByRole("button", {name: "Add a person"})).not.toBeInTheDocument();
    });

    it("Cancel folds it away again and clears what was typed", async () => {
      setup();
      render(<Harness />);
      fireEvent.click(await screen.findByRole("button", {name: "Add a person"}));
      fireEvent.change(screen.getByLabelText("Email"), {target: {value: "new@example.test"}});
      fireEvent.change(screen.getByLabelText("Password"), {target: {value: "long-test-password"}});
      fireEvent.click(screen.getByRole("button", {name: "Cancel"}));
      expect(screen.queryByLabelText("Email")).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", {name: "Add a person"}));
      expect(screen.getByLabelText("Email")).toHaveValue("");
      expect(screen.getByLabelText("Password")).toHaveValue("");
    });

    it("adds the person with the chosen role, then folds the form away", async () => {
      setup();
      vi.mocked(api.createRemoteUser).mockResolvedValue({id: "two", email: "new@example.test", role: "Operator", enabled: true});
      render(<Harness />);
      fireEvent.click(await screen.findByRole("button", {name: "Add a person"}));
      fireEvent.change(screen.getByLabelText("Email"), {target: {value: "new@example.test"}});
      fireEvent.change(screen.getByLabelText("Password"), {target: {value: "long-test-password"}});
      fireEvent.change(screen.getByLabelText("New user role"), {target: {value: "Operator"}});
      fireEvent.click(screen.getByRole("button", {name: "Add user"}));
      await waitFor(() => expect(api.createRemoteUser).toHaveBeenCalledWith("new@example.test", "long-test-password", "Operator"));
      await waitFor(() => expect(screen.queryByLabelText("Email")).not.toBeInTheDocument());
      expect(screen.getByRole("button", {name: "Add a person"})).toBeInTheDocument();
    });

    it("keeps the form open with what was typed when adding fails", async () => {
      setup();
      vi.mocked(api.createRemoteUser).mockRejectedValue(new Error("no"));
      render(<Harness />);
      fireEvent.click(await screen.findByRole("button", {name: "Add a person"}));
      fireEvent.change(screen.getByLabelText("Email"), {target: {value: "new@example.test"}});
      fireEvent.change(screen.getByLabelText("Password"), {target: {value: "long-test-password"}});
      fireEvent.click(screen.getByRole("button", {name: "Add user"}));
      await waitFor(() => expect(api.createRemoteUser).toHaveBeenCalledTimes(1));
      expect(await screen.findByRole("alert")).toBeInTheDocument();
      expect(screen.getByLabelText("Email")).toHaveValue("new@example.test");
    });

    it("hides the button while a password is being changed, and the first-Operator form stays open", async () => {
      setup();
      render(<Harness />);
      fireEvent.click(await screen.findByRole("button", {name: `Change password for ${operator.email}`}));
      expect(screen.queryByRole("button", {name: "Add a person"})).not.toBeInTheDocument();
      expect(screen.getByLabelText(`New password for ${operator.email}`)).toBeVisible();
    });

    it("never offers it on the first-Operator step: that form is already the only thing to fill", async () => {
      vi.mocked(api.getRemoteStatus).mockResolvedValue({...off, enabled: true, state: "connected", needs_operator: true});
      render(<Harness />);
      expect(await screen.findByRole("button", {name: "Create Operator"})).toBeInTheDocument();
      expect(screen.queryByRole("button", {name: "Add a person"})).not.toBeInTheDocument();
    });
  });

  it("renders deliberate Viewer read-only without admin requests", () => {
    render(<AccessContext.Provider value={{...DESKTOP_ACCESS, mode: "remote", capabilities: {...DESKTOP_ACCESS.capabilities, canConfigure: false, canOperate: false}}}>
      <Harness />
    </AccessContext.Provider>);
    expect(screen.getByText(/Read-only access/)).toBeInTheDocument();
    expect(api.getRemoteStatus).not.toHaveBeenCalled();
  });

  it("hides unsafe links and raw infrastructure errors", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue({...off, enabled: true, state: "connected", url: "javascript:alert(1)", needs_operator: false});
    vi.mocked(api.getRemoteUsers).mockRejectedValue(new Error("private /database/provider error"));
    render(<Harness />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Local StagePilot is unaffected");
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
    expect(screen.queryByText(/private \/database/)).not.toBeInTheDocument();
  });

  it("regenerate asks for confirmation; cancel makes no changes; confirm calls the API", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue({...off, enabled: true, state: "connected", needs_operator: false, url: "https://test.trycloudflare.com"});
    vi.mocked(api.regenerateRemote).mockResolvedValue({...off, enabled: true, state: "connected", needs_operator: false, url: "https://test2.trycloudflare.com"});
    render(<Harness />);
    expect(await screen.findByRole("button", {name: "Regenerate Remote link"})).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", {name: "Regenerate Remote link"}));
    expect(await screen.findByRole("group", {name: "Confirm regenerate Remote link"})).toBeInTheDocument();
    expect(screen.getByText(/stop working/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", {name: "Cancel"}));
    expect(api.regenerateRemote).not.toHaveBeenCalled();
    expect(screen.queryByRole("group", {name: "Confirm regenerate Remote link"})).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", {name: "Regenerate Remote link"}));
    fireEvent.click(screen.getByRole("button", {name: "Confirm regenerate"}));
    await waitFor(() => expect(api.regenerateRemote).toHaveBeenCalledTimes(1));
  });

  it("shows an in-button loading state during regenerate, disables the button, and blocks concurrent clicks", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue({...off, enabled: true, state: "connected", needs_operator: false, url: "https://test.trycloudflare.com"});
    let resolveRegenerate!: (value: api.RemoteStatus) => void;
    vi.mocked(api.regenerateRemote).mockReturnValue(new Promise((resolve) => { resolveRegenerate = resolve; }));
    render(<Harness />);
    expect(await screen.findByRole("button", {name: "Regenerate Remote link"})).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", {name: "Regenerate Remote link"}));
    fireEvent.click(screen.getByRole("button", {name: "Confirm regenerate"}));

    const busyButton = await screen.findByRole("button", {name: /Regenerating…/});
    expect(busyButton).toBeDisabled();

    // A second click while in flight must never trigger a second call
    // (there is no visible confirm dialog anymore, but clicking the busy
    // button itself must be a no-op).
    fireEvent.click(busyButton);
    expect(api.regenerateRemote).toHaveBeenCalledTimes(1);

    resolveRegenerate({...off, enabled: true, state: "connected", needs_operator: false, url: "https://test2.trycloudflare.com"});
    await waitFor(() => expect(screen.queryByRole("button", {name: /Regenerating…/})).not.toBeInTheDocument());
    expect(await screen.findByRole("button", {name: "Regenerate Remote link"})).toBeEnabled();
  });

  it("restores normal state and surfaces an error when regenerate fails", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue({...off, enabled: true, state: "connected", needs_operator: false, url: "https://test.trycloudflare.com"});
    vi.mocked(api.regenerateRemote).mockRejectedValue(new Error("provider unavailable"));
    render(<Harness />);
    expect(await screen.findByRole("button", {name: "Regenerate Remote link"})).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", {name: "Regenerate Remote link"}));
    fireEvent.click(screen.getByRole("button", {name: "Confirm regenerate"}));

    expect(await screen.findByRole("alert")).toHaveTextContent("Local StagePilot is unaffected");
    expect(await screen.findByRole("button", {name: "Regenerate Remote link"})).toBeEnabled();
    expect(screen.queryByRole("button", {name: /Regenerating…/})).not.toBeInTheDocument();
  });

  it("shows a reset action for a permanently revoked installation and calls the reset API", async () => {
    vi.mocked(api.getRemoteStatus).mockResolvedValue({...off, needs_operator: false, permanently_revoked: true});
    vi.mocked(api.resetRemoteIdentity).mockResolvedValue({...off, needs_operator: true, permanently_revoked: false});
    render(<Harness />);
    expect(await screen.findByText(/needs to be reset/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", {name: "Reset installation identity"}));
    await waitFor(() => expect(api.resetRemoteIdentity).toHaveBeenCalledTimes(1));
  });

  it("does not show the revoked-credential banner for a single transient false reading that self-resolves", async () => {
    vi.useFakeTimers({shouldAdvanceTime: true});
    const transient: api.RemoteStatus = {...off, needs_operator: false, credential_available: false};
    const recovered: api.RemoteStatus = {...off, needs_operator: false, credential_available: true};
    vi.mocked(api.getRemoteStatus)
      .mockResolvedValueOnce(transient)
      .mockResolvedValue(recovered);
    render(<Harness />);
    await vi.waitFor(() => expect(api.getRemoteStatus).toHaveBeenCalledTimes(1));
    expect(screen.queryByText(/credential is unavailable or revoked/)).not.toBeInTheDocument();
    await vi.advanceTimersByTimeAsync(2000);
    await vi.waitFor(() => expect(api.getRemoteStatus).toHaveBeenCalledTimes(2));
    expect(screen.queryByText(/credential is unavailable or revoked/)).not.toBeInTheDocument();
    vi.useRealTimers();
  });

  it("shows the revoked-credential banner only once unavailability persists across multiple consecutive polls", async () => {
    vi.useFakeTimers({shouldAdvanceTime: true});
    const unavailable: api.RemoteStatus = {...off, needs_operator: false, credential_available: false};
    vi.mocked(api.getRemoteStatus).mockResolvedValue(unavailable);
    render(<Harness />);
    await vi.waitFor(() => expect(api.getRemoteStatus).toHaveBeenCalledTimes(1));
    expect(screen.queryByText(/credential is unavailable or revoked/)).not.toBeInTheDocument();
    await vi.advanceTimersByTimeAsync(2000);
    await vi.waitFor(() => expect(api.getRemoteStatus).toHaveBeenCalledTimes(2));
    expect(screen.queryByText(/credential is unavailable or revoked/)).not.toBeInTheDocument();
    await vi.advanceTimersByTimeAsync(2000);
    await vi.waitFor(() => expect(api.getRemoteStatus).toHaveBeenCalledTimes(3));
    expect(await screen.findByText(/credential is unavailable or revoked/)).toBeInTheDocument();
    vi.useRealTimers();
  });
});
