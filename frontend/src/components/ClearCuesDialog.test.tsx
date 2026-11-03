import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ClearCuesDialog, type ClearCuesRequest } from "./ClearCuesDialog";

const request = (extra: Partial<ClearCuesRequest> = {}): ClearCuesRequest => ({
  kind: "song", subject: "Holy Forever", looks: 12, handmade: 3, loudness: 0, onErase: vi.fn(), ...extra,
});

describe("ClearCuesDialog", () => {
  it("renders nothing without a request", () => {
    render(<ClearCuesDialog request={null} onClose={vi.fn()} />);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("says the cues are permanently erased, names the song and shows the counts", () => {
    render(<ClearCuesDialog request={request()} onClose={vi.fn()} />);
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("Erase these cues permanently?");
    expect(dialog).toHaveTextContent("permanently erased");
    expect(dialog).toHaveTextContent("cannot be undone");
    expect(dialog).toHaveTextContent("Holy Forever");
    expect(within(dialog).getByTestId("erase-looks")).toHaveTextContent("12 looks");
    expect(within(dialog).getByTestId("erase-handmade")).toHaveTextContent("3 hand-made cues");
    expect(within(dialog).queryByTestId("erase-loudness")).toBeNull();
  });

  it("uses singular words and shows loudness when it is erased", () => {
    render(<ClearCuesDialog request={request({ kind: "loudness", looks: 0, handmade: 0, loudness: 1 })} onClose={vi.fn()} />);
    expect(screen.getByTestId("erase-loudness")).toHaveTextContent("loudness");
    expect(screen.queryByTestId("erase-looks")).toBeNull();
    expect(screen.getByRole("dialog")).toHaveTextContent("Erase this loudness permanently?");
  });

  it("names the whole plan for clear all", () => {
    render(<ClearCuesDialog request={request({ kind: "all", subject: 'every song in "Sunday"', looks: 1, handmade: 1 })} onClose={vi.fn()} />);
    expect(screen.getByRole("dialog")).toHaveTextContent('every song in "Sunday"');
    expect(screen.getByTestId("erase-looks")).toHaveTextContent("1 look");
    expect(screen.getByTestId("erase-handmade")).toHaveTextContent("1 hand-made cue");
  });

  it("focuses Keep cues by default; Escape and the backdrop cancel and erase nothing", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    const req = request();
    render(<ClearCuesDialog request={req} onClose={onClose} />);
    expect(screen.getByRole("button", { name: "Keep cues" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
    await user.click(document.querySelector("[data-dialog-backdrop]")!);
    expect(onClose).toHaveBeenCalledTimes(2);
    await user.click(screen.getByRole("button", { name: "Keep cues" }));
    expect(onClose).toHaveBeenCalledTimes(3);
    expect(req.onErase).not.toHaveBeenCalled();
  });

  it("Erase permanently calls the handler exactly once and closes", async () => {
    const onClose = vi.fn();
    const req = request();
    render(<ClearCuesDialog request={req} onClose={onClose} />);
    await userEvent.setup().click(screen.getByRole("button", { name: "Erase permanently" }));
    expect(req.onErase).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
