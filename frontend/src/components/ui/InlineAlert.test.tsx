import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { InlineAlert } from "./InlineAlert";

describe("InlineAlert", () => {
  it.each([
    ["danger", "alert"],
    ["warning", "alert"],
    ["success", "status"],
    ["info", "status"],
  ] as const)("%s uses role %s", (tone, role) => {
    render(<InlineAlert tone={tone}>Message</InlineAlert>);
    const el = screen.getByRole(role);
    expect(el).toHaveAttribute("data-tone", tone);
    const edge = tone === "success" ? "success-soft" : tone;
    expect(el).toHaveClass("rounded-ds-lg", "border", `bg-${edge}/10`, `border-${edge}/${tone === "success" ? 40 : 30}`);
  });

  it("appends className last", () => {
    render(<InlineAlert tone="info" className="extra">m</InlineAlert>);
    expect(screen.getByRole("status").className.endsWith("extra")).toBe(true);
  });

  it("a confirmation that holds buttons can be a named group instead of an interruption", () => {
    render(<InlineAlert aria-label="Confirm" role="group" tone="warning">Sure?</InlineAlert>);
    expect(screen.getByRole("group", { name: "Confirm" })).toHaveAttribute("data-tone", "warning");
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
