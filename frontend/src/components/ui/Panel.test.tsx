import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Panel } from "./Panel";

describe("Panel", () => {
  it("renders a section by default and the three allowed elements", () => {
    const { rerender } = render(<Panel data-testid="p">x</Panel>);
    expect(screen.getByTestId("p").tagName).toBe("SECTION");
    rerender(<Panel as="div" data-testid="p">x</Panel>);
    expect(screen.getByTestId("p").tagName).toBe("DIV");
    rerender(<Panel as="article" data-testid="p">x</Panel>);
    expect(screen.getByTestId("p").tagName).toBe("ARTICLE");
  });

  it.each(["default", "setup", "live"] as const)("%s has the shared border, radius and shadow", (variant) => {
    render(<Panel variant={variant} data-testid="p" />);
    const el = screen.getByTestId("p");
    expect(el).toHaveAttribute("data-variant", variant);
    expect(el).toHaveClass("border", "border-edge", "rounded-ds-xl", "shadow-elevation-default");
  });

  it("uses 16px padding for default and live, 20px for setup", () => {
    const { rerender } = render(<Panel data-testid="p" />);
    expect(screen.getByTestId("p")).toHaveClass("p-ds-lg", "bg-surface-panel/[0.94]");
    rerender(<Panel variant="setup" data-testid="p" />);
    expect(screen.getByTestId("p")).toHaveClass("p-ds-xl");
    rerender(<Panel variant="live" data-testid="p" />);
    expect(screen.getByTestId("p")).toHaveClass("p-ds-lg", "bg-surface-panel-strong/90");
  });

  it("has the wash only on setup and live, and the rail only on live", () => {
    const { rerender } = render(<Panel data-testid="p" />);
    const classes = () => screen.getByTestId("p").className;
    expect(classes()).not.toMatch(/linear-gradient|from-primary|before:/);
    rerender(<Panel variant="setup" data-testid="p" />);
    expect(classes()).toMatch(/120deg/);
    expect(classes()).toContain("from-primary/[0.13]");
    expect(classes()).toContain("to-[36%]");
    expect(classes()).not.toContain("before:");
    rerender(<Panel variant="live" data-testid="p" />);
    expect(classes()).toMatch(/135deg/);
    expect(classes()).toContain("to-[42%]");
    expect(classes()).toContain("before:bg-primary");
    expect(classes()).toContain("before:w-[5px]");
  });

  it("adds overscroll-contain only when scrollable", () => {
    const { rerender } = render(<Panel data-testid="p" />);
    expect(screen.getByTestId("p")).not.toHaveClass("overscroll-contain");
    rerender(<Panel scrollable data-testid="p" />);
    expect(screen.getByTestId("p")).toHaveClass("overscroll-contain");
  });

  it("appends className last and never uses !important", () => {
    render(<Panel className="extra" data-testid="p" />);
    expect(screen.getByTestId("p").className.endsWith("extra")).toBe(true);
    for (const variant of ["default", "setup", "live"] as const) {
      const { unmount } = render(<Panel variant={variant} data-testid="v" />);
      expect(screen.getByTestId("v").className).not.toContain("!");
      unmount();
    }
  });
});
