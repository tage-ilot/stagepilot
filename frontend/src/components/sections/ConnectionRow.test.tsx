import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ConnectionRow } from "./ConnectionRow";

const buttons = (row: HTMLElement) => within(row).getAllByRole("button");
const names = (row: HTMLElement) => buttons(row).map((button) => button.textContent);

describe("ConnectionRow: the one line of address, port and the two buttons", () => {
  it("is one wrapping row with its bottoms aligned and a little room above it", () => {
    render(<ConnectionRow testId="row"><input aria-label="Host" /></ConnectionRow>);
    const row = screen.getByTestId("row");
    expect(row.tagName).toBe("DIV");
    expect(row).toHaveClass("mt-2", "flex", "flex-wrap", "items-end", "gap-x-2.5", "gap-y-2");
    expect(row).toHaveAttribute("data-part", "connection-row");
  });

  it("ALWAYS ends with Scan, then Connect, in every state: nothing wired, busy, scanning", () => {
    const states = [{}, { onScan: vi.fn(), onConnect: vi.fn() }, { onScan: vi.fn(), onConnect: vi.fn(), connecting: true }, { onScan: vi.fn(), onConnect: vi.fn(), scanning: true }, { scanDisabled: true, connectDisabled: true }];
    for (const state of states) {
      const { unmount } = render(<ConnectionRow testId="row" {...state}><input aria-label="Host" /></ConnectionRow>);
      const row = screen.getByTestId("row");
      expect(Array.from(row.children).map((child) => child.tagName)).toEqual(["INPUT", "SPAN", "SPAN"]);
      expect(buttons(row)).toHaveLength(2);
      expect(buttons(row)[0]).toHaveAttribute("data-part", "scan");
      expect(buttons(row)[1]).toHaveAttribute("data-part", "connect");
      unmount();
    }
  });

  it("says exactly Scan and Connect, and the busy words while it works", () => {
    const { rerender } = render(<ConnectionRow onConnect={vi.fn()} onScan={vi.fn()} testId="row">x</ConnectionRow>);
    expect(names(screen.getByTestId("row"))).toEqual(["Scan", "Connect"]);
    rerender(<ConnectionRow onCancelScan={vi.fn()} onConnect={vi.fn()} onScan={vi.fn()} scanning testId="row">x</ConnectionRow>);
    expect(screen.getByRole("button", { name: "Scanning\u2026 (cancel)" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Connect" })).toBeEnabled();
    rerender(<ConnectionRow connecting onConnect={vi.fn()} onScan={vi.fn()} testId="row">x</ConnectionRow>);
    const busy = screen.getByRole("button", { name: "Connecting\u2026" });
    expect(busy).toHaveAttribute("aria-busy", "true");
    // Connect is busy, so Scan waits (nothing else is switched on while one request runs).
    expect(screen.getByRole("button", { name: "Scan" })).toBeDisabled();
    expect(screen.queryByText(/Test connection|Check again|Scan Network|Scan again/)).toBeNull();
  });

  it("Scan scans, Scanning… (cancel) cancels, Connect connects; a disabled one does nothing and says why", async () => {
    const user = userEvent.setup();
    const onScan = vi.fn(); const onCancelScan = vi.fn(); const onConnect = vi.fn();
    const { rerender } = render(<ConnectionRow onCancelScan={onCancelScan} onConnect={onConnect} onScan={onScan} testId="row">x</ConnectionRow>);
    await user.click(screen.getByRole("button", { name: "Scan" }));
    await user.click(screen.getByRole("button", { name: "Connect" }));
    expect([onScan.mock.calls.length, onConnect.mock.calls.length, onCancelScan.mock.calls.length]).toEqual([1, 1, 0]);
    rerender(<ConnectionRow onCancelScan={onCancelScan} onConnect={onConnect} onScan={onScan} scanning testId="row">x</ConnectionRow>);
    await user.click(screen.getByRole("button", { name: "Scanning\u2026 (cancel)" }));
    expect([onScan.mock.calls.length, onCancelScan.mock.calls.length]).toEqual([1, 1]);
    rerender(<ConnectionRow connectDisabled connectHint="Choose a computer first." onConnect={onConnect} onScan={onScan} scanDisabled scanHint="Turn it on first." testId="row">x</ConnectionRow>);
    expect(screen.getByRole("button", { name: "Scan" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Connect" }).closest("[data-part=connect-wrap]")).toHaveAttribute("title", "Choose a computer first.");
    expect(screen.getByRole("button", { name: "Scan" }).closest("[data-part=scan-wrap]")).toHaveAttribute("title", "Turn it on first.");
    await user.click(screen.getByRole("button", { name: "Connect" }));
    expect(onConnect).toHaveBeenCalledTimes(1);
  });

  it("a block that cannot scan still draws Scan, switched off, with its reason", () => {
    render(<ConnectionRow onConnect={vi.fn()} scanHint="Acme cannot be found by scanning." testId="row">x</ConnectionRow>);
    expect(screen.getByRole("button", { name: "Scan" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Scan" }).closest("[data-part=scan-wrap]")).toHaveAttribute("title", "Acme cannot be found by scanning.");
  });

  it("both buttons are the same size and the same class of control, narrow and wide", () => {
    render(<ConnectionRow onConnect={vi.fn()} onScan={vi.fn()} testId="row">x</ConnectionRow>);
    const [scan, connect] = buttons(screen.getByTestId("row"));
    for (const size of ["flex-none", "min-h-ds-control"]) {
      expect(scan!.className).toContain(size);
      expect(connect!.className).toContain(size);
    }
    expect(scan).toHaveAttribute("data-size", connect!.getAttribute("data-size"));
    // They wrap with the row (one line when wide, below the fields when narrow), never apart from each other.
    expect(screen.getByTestId("row")).toHaveClass("flex-wrap");
  });

  it("keyboard order: the fields, then Scan, then Connect, then whatever comes after", async () => {
    const user = userEvent.setup();
    render(<ConnectionRow after={<button type="button">Mode</button>} onConnect={vi.fn()} onScan={vi.fn()} testId="row"><input aria-label="Host" /><input aria-label="API" /></ConnectionRow>);
    const order: string[] = [];
    for (let step = 0; step < 5; step += 1) { await user.tab(); const el = document.activeElement as HTMLElement; order.push(el.getAttribute("aria-label") ?? el.textContent ?? ""); }
    expect(order).toEqual(["Host", "API", "Scan", "Connect", "Mode"]);
  });

  it("can be a form: Enter in the address field presses Connect, and not while it is busy or off", () => {
    const onConnect = vi.fn();
    const { rerender } = render(<ConnectionRow as="form" id="specific" onConnect={onConnect} onScan={vi.fn()} testId="row"><input aria-label="Host" /></ConnectionRow>);
    const row = screen.getByTestId("row");
    expect(row.tagName).toBe("FORM");
    expect(row).toHaveAttribute("id", "specific");
    expect(screen.getByRole("button", { name: "Connect" })).toHaveAttribute("type", "submit");
    expect(screen.getByRole("button", { name: "Scan" })).toHaveAttribute("type", "button");
    fireEvent.submit(row);
    expect(onConnect).toHaveBeenCalledTimes(1);
    rerender(<ConnectionRow as="form" connectDisabled id="specific" onConnect={onConnect} testId="row"><input aria-label="Host" /></ConnectionRow>);
    fireEvent.submit(screen.getByTestId("row"));
    rerender(<ConnectionRow as="form" connecting id="specific" onConnect={onConnect} testId="row"><input aria-label="Host" /></ConnectionRow>);
    fireEvent.submit(screen.getByTestId("row"));
    expect(onConnect).toHaveBeenCalledTimes(1);
  });

  it("a row inside another row takes no space above it, draws no buttons, and keeps the page's own class", () => {
    render(<ConnectionRow className="live-link-modes" nested testId="row">x</ConnectionRow>);
    expect(screen.getByTestId("row")).not.toHaveClass("mt-2");
    expect(screen.getByTestId("row")).toHaveClass("live-link-modes", "flex", "flex-wrap");
    expect(within(screen.getByTestId("row")).queryAllByRole("button")).toHaveLength(0);
  });
});
