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
    expect(el).toHaveClass("rounded-ds-lg", "border", `bg-${tone}/10`, `border-${tone}/30`);
  });

  it("appends className last", () => {
    render(<InlineAlert tone="info" className="extra">m</InlineAlert>);
    expect(screen.getByRole("status").className.endsWith("extra")).toBe(true);
  });
});
