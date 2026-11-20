import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ActionButton, ReasonButton, ReasonWrap, ScanConnectButtons } from "./index";

/** The ReasonButton contract (the design audit's "disabled controls say why"): the one wrapper every page uses. */
describe("ReasonButton", () => {
  it("with a reason: disabled, a <span title> around it, and a hidden copy the button is described by", () => {
    render(<ReasonButton reason="Sign in to Planning Center first.">Load service types</ReasonButton>);
    const button = screen.getByRole("button", { name: "Load service types" });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("title", "Sign in to Planning Center first.");
    expect(button).toHaveAccessibleDescription("Sign in to Planning Center first.");
    const wrap = button.parentElement!;
    expect(wrap.tagName).toBe("SPAN");
    expect(wrap).toHaveAttribute("title", "Sign in to Planning Center first.");
    const hidden = wrap.querySelector(".sr-only")!;
    expect(hidden).toHaveTextContent("Sign in to Planning Center first.");
    expect(button.getAttribute("aria-describedby")).toBe(hidden.id);
  });

  it("a reason switches the button off even if `disabled` says false, and a click does nothing", async () => {
    const onClick = vi.fn();
    render(<ReasonButton disabled={false} onClick={onClick} reason="Nothing to undo.">Undo</ReasonButton>);
    await userEvent.setup().click(screen.getByRole("button", { name: "Undo" }));
    expect(onClick).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Undo" })).toBeDisabled();
  });

  it("without a reason: the plain button, no wrapper, no hidden copy, no description", async () => {
    const onClick = vi.fn();
    const { container } = render(<ReasonButton onClick={onClick}>Undo</ReasonButton>);
    const button = screen.getByRole("button", { name: "Undo" });
    expect(button).toBeEnabled();
    expect(button).not.toHaveAttribute("aria-describedby");
    expect(button).not.toHaveAttribute("title");
    expect(container.querySelector("span")).toBeNull();
    await userEvent.setup().click(button);
    expect(onClick).toHaveBeenCalledOnce();
  });

  it("an empty reason is no reason", () => {
    for (const reason of [undefined, null, "", "  "]) {
      const { unmount, container } = render(<ReasonButton reason={reason}>Go</ReasonButton>);
      expect(screen.getByRole("button", { name: "Go" })).toBeEnabled();
      expect(container.querySelector("[title]")).toBeNull();
      unmount();
    }
  });

  it("a button that is off for another cause (busy) stays off without a reason and adds nothing", () => {
    const { container } = render(<ReasonButton disabled>Test connection</ReasonButton>);
    expect(screen.getByRole("button", { name: "Test connection" })).toBeDisabled();
    expect(container.querySelector("[title]")).toBeNull();
  });

  it("alwaysWrap keeps the wrapper and its hooks while there is no reason, and the reason comes and goes on the same wrapper", () => {
    const { rerender, container } = render(<ReasonButton alwaysWrap wrapProps={{ "data-testid": "wrap", "data-part": "scan-wrap" }}>Search</ReasonButton>);
    const wrap = screen.getByTestId("wrap");
    expect(wrap).toHaveAttribute("data-part", "scan-wrap");
    expect(wrap).not.toHaveAttribute("title");
    expect(container.querySelector(".sr-only")).toBeNull();
    rerender(<ReasonButton alwaysWrap reason="Wait for the search to finish." wrapProps={{ "data-testid": "wrap", "data-part": "scan-wrap" }}>Search</ReasonButton>);
    expect(screen.getByTestId("wrap")).toBe(wrap);
    expect(wrap).toHaveAttribute("title", "Wait for the search to finish.");
  });

  it("keeps a describedby the page already gives, and adds its own", () => {
    render(<><p id="note">Uses the saved settings.</p><ReasonButton aria-describedby="note" reason="Save first.">Test</ReasonButton></>);
    expect(screen.getByRole("button", { name: "Test" })).toHaveAccessibleDescription("Uses the saved settings. Save first.");
  });
});

describe("ActionButton with a reason", () => {
  it("is the same contract", () => {
    render(<ActionButton label="Test connection" onClick={() => undefined} reason="Enter the Planning Center credentials first." />);
    const button = screen.getByRole("button", { name: "Test connection" });
    expect(button).toBeDisabled();
    expect(button).toHaveAccessibleDescription("Enter the Planning Center credentials first.");
    expect(button.parentElement).toHaveAttribute("title", "Enter the Planning Center credentials first.");
  });

  it("without a reason it is unchanged", () => {
    const { container } = render(<ActionButton label="Refresh" onClick={() => undefined} />);
    expect(container.firstElementChild!.tagName).toBe("BUTTON");
  });
});

describe("ReasonWrap on a control that is not a button", () => {
  it("wraps a select the same way", () => {
    render(<ReasonWrap reason="Load service types first.">{(describedBy) => <select aria-describedby={describedBy} aria-label="Service type" disabled><option>One</option></select>}</ReasonWrap>);
    const select = screen.getByRole("combobox", { name: "Service type" });
    expect(select).toHaveAccessibleDescription("Load service types first.");
    expect(select.parentElement).toHaveAttribute("title", "Load service types first.");
  });
});

describe("the Live Link pair uses it", () => {
  it("Search and Connect carry their hints through the shared wrapper", () => {
    render(<ScanConnectButtons connectHint="Type an address to connect." connectDisabled scanHint="Turn Lighting on first." scanDisabled onConnect={() => undefined} onScan={() => undefined} />);
    expect(screen.getByRole("button", { name: "Search" })).toHaveAccessibleDescription("Turn Lighting on first.");
    expect(screen.getByRole("button", { name: "Connect" })).toHaveAccessibleDescription("Type an address to connect.");
    expect(screen.getByRole("button", { name: "Connect" }).parentElement).toHaveAttribute("data-part", "connect-wrap");
  });
});
