import { createRef } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Field } from "./Field";
import { Input } from "./Input";
import { Select } from "./Select";
import { Textarea } from "./Textarea";

const has = (el: HTMLElement, token: string) => el.className.split(/\s+/).includes(token);

describe("Field", () => {
  it("associates the visible label with the control", () => {
    render(<Field label="Cue name"><Input /></Field>);
    expect(screen.getByLabelText("Cue name")).toBeInstanceOf(HTMLInputElement);
  });

  it("works for select and textarea too", () => {
    render(<><Field label="Plan"><Select><option>A</option></Select></Field><Field label="Notes"><Textarea /></Field></>);
    expect(screen.getByLabelText("Plan").tagName).toBe("SELECT");
    expect(screen.getByLabelText("Notes").tagName).toBe("TEXTAREA");
  });

  it("puts the label before the control in the DOM (above, never left)", () => {
    render(<Field label="Name"><Input /></Field>);
    const label = screen.getByText("Name");
    const input = screen.getByLabelText("Name");
    expect(label.compareDocumentPosition(input) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(label.parentElement?.parentElement).toHaveClass("flex-col");
  });

  it("wires hint and error ids into aria-describedby", () => {
    render(<Field label="Port" hint="1 to 16" error="Enter a number from 1 to 16"><Input /></Field>);
    const input = screen.getByLabelText("Port");
    const ids = input.getAttribute("aria-describedby")!.split(" ");
    expect(ids).toHaveLength(2);
    expect(document.getElementById(ids[0]!)).toHaveTextContent("1 to 16");
    expect(document.getElementById(ids[1]!)).toHaveTextContent("Enter a number");
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByRole("alert")).toHaveTextContent("Enter a number from 1 to 16");
  });

  it("has no alert or aria-invalid without an error", () => {
    render(<Field label="Port"><Input /></Field>);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByLabelText("Port")).not.toHaveAttribute("aria-invalid");
    expect(screen.getByLabelText("Port")).not.toHaveAttribute("aria-describedby");
  });

  it("shows the held marker and amber border", () => {
    render(<Field label="Plan" heldChange><Input /></Field>);
    expect(screen.getByText("Not saved yet")).toBeInTheDocument();
    const input = screen.getByLabelText("Plan");
    expect(input).toHaveAttribute("data-state", "held");
    expect(has(input, "border-warning")).toBe(true);
    expect(input.getAttribute("aria-describedby")).toBeTruthy();
  });

  it("hides the held marker by default", () => {
    render(<Field label="Plan"><Input /></Field>);
    expect(screen.queryByText("Not saved yet")).toBeNull();
  });

  it("gives separate Fields separate ids", () => {
    render(<><Field label="A"><Input /></Field><Field label="B"><Input /></Field></>);
    expect(screen.getByLabelText("A").id).not.toBe(screen.getByLabelText("B").id);
  });
});

describe("Input", () => {
  it("is always 44px, touch-manipulation, and has the field look", () => {
    render(<Input aria-label="x" />);
    const el = screen.getByLabelText("x");
    for (const token of ["min-h-ds-touch-target", "touch-manipulation", "rounded-ds-lg", "bg-surface-950", "border-edge", "text-ink", "placeholder:text-ink-muted"]) {
      expect(has(el, token), token).toBe(true);
    }
    expect(el).toHaveAttribute("data-state", "default");
  });

  it("uses a hover-gated medium border", () => {
    render(<Input aria-label="x" />);
    expect(has(screen.getByLabelText("x"), "can-hover:border-edge-medium")).toBe(true);
  });

  it("error prop sets danger border, aria-invalid and data-state", () => {
    render(<Input aria-label="x" error />);
    const el = screen.getByLabelText("x");
    expect(el).toHaveAttribute("aria-invalid", "true");
    expect(el).toHaveAttribute("data-state", "error");
    expect(has(el, "border-danger")).toBe(true);
    expect(has(el, "can-hover:border-edge-medium")).toBe(false);
  });

  it("disabled dims, blocks typing, and drops hover", async () => {
    const onChange = vi.fn();
    render(<Input aria-label="x" disabled onChange={onChange} />);
    const el = screen.getByLabelText("x");
    await userEvent.type(el, "abc");
    expect(onChange).not.toHaveBeenCalled();
    expect(el).toHaveAttribute("data-state", "disabled");
    expect(has(el, "disabled:opacity-40")).toBe(true);
    expect(has(el, "disabled:cursor-not-allowed")).toBe(true);
    expect(has(el, "can-hover:border-edge-medium")).toBe(false);
  });

  it("types and fires onChange, and never blocks paste", async () => {
    const onChange = vi.fn();
    const onPaste = vi.fn();
    render(<Input aria-label="x" onChange={onChange} onPaste={onPaste} />);
    const el = screen.getByLabelText("x");
    await userEvent.type(el, "ab");
    expect(onChange).toHaveBeenCalledTimes(2);
    await userEvent.click(el);
    await userEvent.paste("123");
    expect(onPaste).toHaveBeenCalled();
    expect(el).toHaveValue("ab123");
  });

  it("passes type, inputMode, autoComplete through", () => {
    render(<Input aria-label="x" type="password" inputMode="numeric" autoComplete="one-time-code" />);
    const el = screen.getByLabelText("x");
    expect(el).toHaveAttribute("type", "password");
    expect(el).toHaveAttribute("inputmode", "numeric");
    expect(el).toHaveAttribute("autocomplete", "one-time-code");
  });

  it("merges describedby and appends className last", () => {
    render(<Field label="L" hint="h"><Input aria-describedby="why" className="extra" /></Field>);
    const el = screen.getByLabelText("L");
    expect(el.getAttribute("aria-describedby")).toMatch(/^why /);
    expect(el.className.endsWith("extra")).toBe(true);
  });

  it("forwards refs", () => {
    const ref = createRef<HTMLInputElement>();
    render(<Input ref={ref} aria-label="x" />);
    expect(ref.current).toBe(screen.getByLabelText("x"));
  });
});

describe("Textarea", () => {
  it("is 44px+, resizable vertically, taller minimum", () => {
    render(<Textarea aria-label="t" />);
    const el = screen.getByLabelText("t");
    expect(has(el, "min-h-ds-touch-target")).toBe(true);
    expect(has(el, "min-h-[5.5rem]")).toBe(true);
    expect(has(el, "resize-y")).toBe(true);
  });

  it("error, disabled and ref", async () => {
    const ref = createRef<HTMLTextAreaElement>();
    const onChange = vi.fn();
    render(<><Textarea ref={ref} aria-label="e" error /><Textarea aria-label="d" disabled onChange={onChange} /></>);
    expect(screen.getByLabelText("e")).toHaveAttribute("aria-invalid", "true");
    expect(ref.current).toBe(screen.getByLabelText("e"));
    await userEvent.type(screen.getByLabelText("d"), "x");
    expect(onChange).not.toHaveBeenCalled();
  });
});

describe("Select", () => {
  it("renders options and fires onChange with the new value (uncontrolled)", async () => {
    const onChange = vi.fn((e) => e.target.value);
    render(<Select aria-label="s" onChange={onChange}><option value="a">A</option><option value="b">B</option></Select>);
    expect(screen.getAllByRole("option")).toHaveLength(2);
    await userEvent.selectOptions(screen.getByLabelText("s"), "b");
    expect(onChange).toHaveReturnedWith("b");
  });

  it("controlled value is respected", () => {
    render(<Select aria-label="s" value="b" onChange={() => {}}><option value="a">A</option><option value="b">B</option></Select>);
    expect(screen.getByLabelText("s")).toHaveValue("b");
  });

  it("is 44px, native, with a decorative chevron in currentColor", () => {
    const { container } = render(<Select aria-label="s"><option>A</option></Select>);
    const el = screen.getByLabelText("s");
    expect(el.tagName).toBe("SELECT");
    expect(has(el, "min-h-ds-touch-target")).toBe(true);
    expect(has(el, "appearance-none")).toBe(true);
    const svg = container.querySelector("svg")!;
    expect(svg).toHaveAttribute("aria-hidden", "true");
    expect(svg).toHaveAttribute("stroke", "currentColor");
  });

  it("placeholder is disabled+hidden, selected, and styled muted until a value is chosen", async () => {
    render(<Select aria-label="s" placeholder="Choose a plan"><option value="a">A</option></Select>);
    const el = screen.getByLabelText("s");
    const ph = screen.getByText("Choose a plan") as HTMLOptionElement;
    expect(ph.disabled).toBe(true);
    expect(ph.hidden).toBe(true);
    expect(el).toHaveValue("");
    expect(has(el, "text-ink-muted")).toBe(true);
    expect(el).toHaveAttribute("data-placeholder", "true");
    await userEvent.selectOptions(el, "a");
    expect(el).toHaveValue("a");
    expect(has(el, "text-ink-muted")).toBe(false);
    expect(el).not.toHaveAttribute("data-placeholder");
  });

  it("loading is busy, disabled, shows the caller's label, and does not fire change", async () => {
    const onChange = vi.fn();
    render(<Select aria-label="s" loading loadingLabel="Loading…" onChange={onChange}><option value="a">A</option></Select>);
    const el = screen.getByLabelText("s");
    expect(el).toHaveAttribute("aria-busy", "true");
    expect(el).toBeDisabled();
    expect(el).toHaveAttribute("data-state", "loading");
    expect(el).toHaveAttribute("data-loading", "true");
    expect(screen.getByRole("option", { name: "Loading…" })).toBeInTheDocument();
    await userEvent.selectOptions(el, "a").catch(() => {});
    expect(onChange).not.toHaveBeenCalled();
    expect(has(el, "opacity-100") || el.className.includes("disabled:opacity-100")).toBe(true);
  });

  it("disabled does not fire change", async () => {
    const onChange = vi.fn();
    render(<Select aria-label="s" disabled onChange={onChange}><option value="a">A</option><option value="b">B</option></Select>);
    await userEvent.selectOptions(screen.getByLabelText("s"), "b").catch(() => {});
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByLabelText("s")).toHaveAttribute("data-state", "disabled");
  });

  it("error and Field wiring", () => {
    render(<Field label="Plan" error="Pick one"><Select><option>A</option></Select></Field>);
    const el = screen.getByLabelText("Plan");
    expect(el).toHaveAttribute("aria-invalid", "true");
    expect(has(el, "border-danger")).toBe(true);
    expect(el.getAttribute("aria-describedby")).toBeTruthy();
  });

  it("forwards refs", () => {
    const ref = createRef<HTMLSelectElement>();
    render(<Select ref={ref} aria-label="s"><option>A</option></Select>);
    expect(ref.current).toBe(screen.getByLabelText("s"));
  });
});
