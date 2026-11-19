import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Field } from "../ui/Field";
import { Input } from "../ui/Input";
import { Select } from "../ui/Select";
import { readIndexCss } from "./readIndexCss";
import { SettingsSection } from "./SettingsSection";

/** The DOM contract of the Settings section: the order (title, fields, note), the grid classes, and what busy does. */
const fields = (
  <>
    <Field error="Enter a timeout." heldChange hint="Seconds." label="Timeout"><Input defaultValue="3" /></Field>
    <Field label="Look"><Select defaultValue="a"><option value="a">A</option></Select></Field>
  </>
);

describe("SettingsSection", () => {
  it("is the optional kicker, the fields in the grid, then the one note", () => {
    render(<SettingsSection note="Takes effect after a restart." noteId="n" testId="s" title="Timing">{fields}</SettingsSection>);
    const section = screen.getByTestId("s");
    expect(section).toHaveAttribute("data-section", "settings");
    expect(Array.from(section.children).map((child) => child.getAttribute("data-part"))).toEqual(["title", "fields", "note"]);
    expect(within(section).getByRole("heading", { name: "Timing" }).tagName).toBe("H3");
    // The title names the group for a screen reader.
    expect(screen.getByRole("group", { name: "Timing" })).toBe(section);
    expect(section.querySelector("[data-part=note]")).toHaveAttribute("id", "n");
  });

  it("draws no title and no group role when there is nothing to name", () => {
    render(<SettingsSection testId="s">{fields}</SettingsSection>);
    const section = screen.getByTestId("s");
    expect(section.querySelector("[data-part=title]")).toBeNull();
    expect(section).not.toHaveAttribute("role");
    expect(section.firstElementChild).toHaveAttribute("data-part", "fields");
  });

  it("keeps each Field's held marker, hint and error line (the grid only lays them out)", () => {
    render(<SettingsSection>{fields}</SettingsSection>);
    expect(screen.getByText("Not saved yet")).toBeInTheDocument();
    expect(screen.getByText("Seconds.")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Enter a timeout.");
    expect(screen.getByLabelText("Timeout")).toHaveAttribute("aria-invalid", "true");
  });

  it("is 1, 2 or 4 columns by default and says which column set it uses", () => {
    const { container, rerender } = render(<SettingsSection>{fields}</SettingsSection>);
    const grid = () => container.querySelector("[data-part=fields]") as HTMLElement;
    expect(grid()).toHaveAttribute("data-columns", "standard");
    expect(grid()).toHaveClass("section-fields", "grid", "gap-4");
    expect(container.firstElementChild).toHaveClass("section-fields-wrap");
    for (const columns of ["inset", "pair", "triple", "lead", "single"] as const) {
      rerender(<SettingsSection columns={columns}>{fields}</SettingsSection>);
      expect(grid()).toHaveAttribute("data-columns", columns);
    }
  });

  it("keeps a lone field from stretching when it is narrow", () => {
    const { container } = render(<SettingsSection width="narrow">{fields}</SettingsSection>);
    expect(container.firstElementChild).toHaveClass("max-w-md");
  });

  it("disables every input and select inside while busy, and only then", () => {
    const { rerender } = render(<SettingsSection busy>{fields}</SettingsSection>);
    expect(screen.getByLabelText("Timeout")).toBeDisabled();
    expect(screen.getByLabelText("Look")).toBeDisabled();
    rerender(<SettingsSection busy={false}>{fields}</SettingsSection>);
    expect(screen.getByLabelText("Timeout")).toBeEnabled();
    expect(screen.getByLabelText("Look")).toBeEnabled();
  });

  it("leaves a control the page disabled for its own reason disabled when not busy", () => {
    render(<SettingsSection><Field label="Service type"><Select disabled><option>None</option></Select></Field></SettingsSection>);
    expect(screen.getByLabelText("Service type")).toBeDisabled();
  });

  it("has its column breakpoints in index.css: 1, 2 or 4 across (28rem, 56rem), never 3 + 1", async () => {
    const css = await readIndexCss();
    expect(css).toMatch(/\.section-fields-wrap,\s*\.section-summary-wrap \{\s*container-type: inline-size;/);
    expect(css).toMatch(/@container \(min-width: 28rem\) \{\s*\.section-fields\[data-columns="standard"\] \{\s*grid-template-columns: repeat\(2, minmax\(0, 1fr\)\);/);
    expect(css).toMatch(/@container \(min-width: 56rem\) \{\s*\.section-fields\[data-columns="standard"\] \{\s*grid-template-columns: repeat\(4, minmax\(0, 1fr\)\);/);
    expect(css).toMatch(/@container \(min-width: 21\.8rem\) \{\s*\.section-fields\[data-columns="inset"\]/);
    expect(css).toMatch(/@container \(min-width: 49\.8rem\) \{\s*\.section-fields\[data-columns="inset"\]/);
    expect(css).toMatch(/\.section-fields\[data-columns="lead"\] \{\s*grid-template-columns: minmax\(0, 2fr\) minmax\(0, 1fr\) minmax\(0, 1fr\);/);
  });
});
