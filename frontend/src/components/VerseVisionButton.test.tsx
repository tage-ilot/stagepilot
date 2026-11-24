import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AccessContext } from "../access/AccessContext";
import { DESKTOP_ACCESS } from "../access/accessState";
import type { DashboardAccess } from "../types";

const desktop = vi.hoisted(() => ({ isDesktopShell: vi.fn(() => false), openExternalUrl: vi.fn().mockResolvedValue(undefined) }));
vi.mock("../desktop", () => desktop);

import { apiOrigin } from "../api";
import { VerseVisionButton } from "./VerseVisionButton";

const lan: DashboardAccess = { ...DESKTOP_ACCESS, mode: "lan" };
const remote: DashboardAccess = { ...DESKTOP_ACCESS, mode: "remote" };

describe("VerseVisionButton", () => {
  beforeEach(() => {
    desktop.isDesktopShell.mockReturnValue(false);
    desktop.openExternalUrl.mockClear();
  });

  it("is the big VerseVision link: a new browser tab on the server's own address", () => {
    render(<AccessContext.Provider value={lan}><VerseVisionButton /></AccessContext.Provider>);
    const link = screen.getByRole("link", { name: /^VerseVision/ });
    expect(link).toHaveAttribute("href", `${apiOrigin}/versevision/`);
    expect(link).toHaveAttribute("target", "_blank");
    expect(screen.getByTestId("versevision-button")).toBe(link);
  });

  it("hands the address to the system browser in the desktop app, and does not navigate the dashboard", async () => {
    desktop.isDesktopShell.mockReturnValue(true);
    render(<VerseVisionButton />);
    await userEvent.setup().click(screen.getByRole("link", { name: /^VerseVision/ }));
    expect(desktop.openExternalUrl).toHaveBeenCalledExactlyOnceWith(`${apiOrigin}/versevision/`);
  });

  it("is not drawn for a remote session, which the server refuses it to", () => {
    const { container } = render(<AccessContext.Provider value={remote}><VerseVisionButton /></AccessContext.Provider>);
    expect(container).toBeEmptyDOMElement();
  });
});
