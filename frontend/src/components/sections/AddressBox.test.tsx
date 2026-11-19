import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { rememberAddress } from "../../hooks/useRecentAddresses";
import { Field } from "../ui/Field";
import { AddressBox, NO_ADDRESSES, type AddressBoxProps } from "./AddressBox";
import { addressGroups, scannedAddresses } from "./addressOptions";
import { NO_SCAN, type ScanState } from "./scanState";

beforeEach(() => window.localStorage.clear());

const SCANNED = [{ host: "10.30.0.141", label: "Playback (front) (10.30.0.141)" }, { host: "10.30.0.152", label: "Playback (stage) (10.30.0.152)" }];

function Harness({ initial = "", ...props }: { initial?: string } & Partial<AddressBoxProps>) {
  const [value, setValue] = useState(initial);
  return <AddressBox aria-label="Host" {...props} onChange={(next) => { setValue(next); props.onChange?.(next); }} scope="playback" value={value} />;
}
const box = () => screen.getByRole("combobox", { name: "Host" });
const arrow = () => screen.getByRole("button", { name: "Show recent and scanned addresses" });
const names = () => screen.queryAllByRole("option").map((option) => option.textContent?.replace("(in the box now)", "").trim());
const remember = (...hosts: string[]) => hosts.reverse().forEach((host) => rememberAddress("playback", host));

describe("AddressBox: the text is always editable", () => {
  it("is an ordinary text box: typing, replacing and editing the current value all work, with the list closed or open", async () => {
    const user = userEvent.setup();
    remember("10.0.0.5");
    render(<Harness initial="192.0.2.10" />);
    expect(box()).toHaveValue("192.0.2.10");
    expect(box()).toHaveClass("font-mono", "!h-10");
    await user.clear(box());
    await user.type(box(), "10.0.0.77");
    expect(box()).toHaveValue("10.0.0.77");
    await user.type(box(), "{Backspace}{Backspace}9");
    expect(box()).toHaveValue("10.0.0.9");
    await user.click(arrow());
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    await user.type(box(), "1");
    expect(box()).toHaveValue("10.0.0.91");
    await user.tab();
    await user.click(box());
    await user.paste("pasted.local");
    expect(box()).toHaveValue("10.0.0.91pasted.local");
  });

  it("typing while the list is open edits the box and closes the list, so the next Enter is Connect", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    remember("10.0.0.5");
    render(<Harness onSubmit={onSubmit} />);
    await user.click(box());
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    await user.keyboard("10.0.0.9");
    expect(box()).toHaveValue("10.0.0.9");
    expect(screen.queryByRole("listbox")).toBeNull();
    await user.keyboard("{Enter}");
    expect(onSubmit).toHaveBeenCalledTimes(1);
    expect(box()).toHaveValue("10.0.0.9");
  });
});

describe("AddressBox: opening and closing", () => {
  it("is a combobox with the arrow inside its right edge; the list is closed at first", () => {
    render(<Harness />);
    expect(box()).toHaveAttribute("aria-expanded", "false");
    expect(box()).toHaveAttribute("aria-autocomplete", "none");
    expect(box()).toHaveAttribute("aria-controls");
    expect(arrow()).toHaveAttribute("aria-expanded", "false");
    expect(arrow()).toHaveAttribute("tabindex", "-1");
    expect(arrow()).toHaveClass("absolute", "right-0", "w-8", "coarse:w-11");
    expect(arrow().parentElement).toBe(box().parentElement);
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("opens with the arrow and closes with it again", async () => {
    const user = userEvent.setup();
    remember("10.0.0.5");
    render(<Harness />);
    await user.click(arrow());
    expect(box()).toHaveAttribute("aria-expanded", "true");
    expect(box()).toHaveFocus();
    const list = screen.getByRole("listbox");
    expect(box()).toHaveAttribute("aria-controls", list.id);
    expect(arrow()).toHaveAttribute("aria-controls", list.id);
    await user.click(arrow());
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(box()).toHaveAttribute("aria-expanded", "false");
  });

  it("opens with ArrowDown and with Alt+ArrowDown; ArrowDown goes on to the first option, Alt+ArrowDown only opens", async () => {
    const user = userEvent.setup();
    remember("10.0.0.5", "10.0.0.6");
    render(<Harness />);
    await user.click(box());
    await user.keyboard("{Alt>}{ArrowDown}{/Alt}");
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    expect(box()).not.toHaveAttribute("aria-activedescendant");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).toBeNull();
    await user.keyboard("{ArrowDown}");
    expect(box().getAttribute("aria-activedescendant")).toBe(screen.getAllByRole("option")[0]?.id);
  });

  it("closes on Escape (keeping what is typed and the focus), on choosing, on a click outside, and when focus leaves", async () => {
    const user = userEvent.setup();
    remember("10.0.0.5");
    render(<><Harness initial="typed" /><button type="button">Elsewhere</button><p>Plain text</p></>);
    await user.click(arrow());
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(box()).toHaveValue("typed");
    expect(box()).toHaveFocus();

    await user.click(arrow());
    await user.click(screen.getByText("Plain text"));
    expect(screen.queryByRole("listbox")).toBeNull();

    await user.click(arrow());
    await user.click(screen.getByRole("button", { name: "Elsewhere" }));
    expect(screen.queryByRole("listbox")).toBeNull();

    await user.click(arrow());
    await user.click(screen.getByRole("option", { name: /10\.0\.0\.5/ }));
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(box()).toHaveValue("10.0.0.5");
  });

  it("Escape with the list closed is left alone (it is not swallowed), with it open it does not reach a dialog around the box", async () => {
    const user = userEvent.setup();
    const outer = vi.fn();
    remember("10.0.0.5");
    render(<div onKeyDown={(event) => { if (event.key === "Escape") outer(); }}><Harness /></div>);
    await user.click(box());
    await user.keyboard("{Escape}");
    expect(outer).toHaveBeenCalledTimes(1);
    await user.keyboard("{ArrowDown}{Escape}");
    expect(outer).toHaveBeenCalledTimes(1);
  });

  it("never traps focus: Tab leaves the box (closing the list) and goes on to the next control, from the open list too", async () => {
    const user = userEvent.setup();
    remember("10.0.0.5");
    render(<><Harness /><button type="button">Next</button></>);
    await user.click(box());
    await user.keyboard("{ArrowDown}");
    await user.tab();
    expect(screen.getByRole("button", { name: "Next" })).toHaveFocus();
    expect(screen.queryByRole("listbox")).toBeNull();
    await user.tab({ shift: true });
    expect(box()).toHaveFocus();
  });
});

describe("AddressBox: choosing", () => {
  it("a click on an option puts its address in the box, keeps the focus in the box, and connects nothing", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    remember("10.0.0.5", "10.0.0.6");
    render(<Harness initial="old" onSubmit={onSubmit} />);
    await user.click(arrow());
    await user.click(screen.getByRole("option", { name: /10\.0\.0\.6/ }));
    expect(box()).toHaveValue("10.0.0.6");
    expect(box()).toHaveFocus();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("a chosen address is only a starting point: it can be edited or replaced at once", async () => {
    const user = userEvent.setup();
    remember("10.0.0.5");
    render(<Harness />);
    await user.click(arrow());
    await user.click(screen.getByRole("option", { name: /10\.0\.0\.5/ }));
    await user.type(box(), "0");
    expect(box()).toHaveValue("10.0.0.50");
    await user.clear(box());
    await user.type(box(), "other.local");
    expect(box()).toHaveValue("other.local");
  });

  it("arrows move through the options (wrapping), Enter chooses the active one, and Enter does not submit", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    remember("10.0.0.5", "10.0.0.6");
    render(<Harness onSubmit={onSubmit} scanned={SCANNED} />);
    await user.click(box());
    await user.keyboard("{ArrowDown}");
    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(4);
    expect(options[0]).toHaveAttribute("aria-selected", "true");
    expect(box()).toHaveAttribute("aria-activedescendant", options[0]?.id);
    await user.keyboard("{ArrowDown}{ArrowDown}");
    expect(options[2]).toHaveAttribute("aria-selected", "true");
    expect(options[0]).toHaveAttribute("aria-selected", "false");
    await user.keyboard("{ArrowUp}{ArrowUp}{ArrowUp}");
    expect(options[3]).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{ArrowDown}");
    expect(options[0]).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{ArrowDown}{Enter}");
    expect(box()).toHaveValue("10.30.0.152");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(onSubmit).not.toHaveBeenCalled();
    expect(box()).not.toHaveAttribute("aria-activedescendant");
  });

  it("Enter in the open list with nothing active only closes it", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    remember("10.0.0.5");
    render(<Harness initial="keep" onSubmit={onSubmit} />);
    await user.click(box());
    await user.keyboard("{Alt>}{ArrowDown}{/Alt}{Enter}");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(box()).toHaveValue("keep");
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("opening starts on the option that is already in the box", async () => {
    const user = userEvent.setup();
    remember("10.0.0.5", "10.0.0.6");
    render(<Harness initial="10.0.0.6" />);
    await user.click(box());
    await user.keyboard("{ArrowDown}");
    expect(box().getAttribute("aria-activedescendant")).toBe(screen.getByRole("option", { name: /10\.0\.0\.6/ }).id);
  });

  it("tells onPick which group and port an entry came from, after the address is in the box", async () => {
    const user = userEvent.setup();
    const onPick = vi.fn();
    const onChange = vi.fn();
    render(<Harness onChange={onChange} onPick={onPick} scanned={[{ host: "192.0.2.21", label: "Front MacBook (192.0.2.21)", port: 1026 }]} />);
    await user.click(arrow());
    await user.click(screen.getByRole("option", { name: /Front MacBook/ }));
    expect(onChange).toHaveBeenCalledWith("192.0.2.21");
    expect(onPick).toHaveBeenCalledWith({ host: "192.0.2.21", port: 1026, group: "scanned" });
  });
});

describe("AddressBox: Enter submits only with the list closed", () => {
  it("Enter in the box with the list closed is the block's Connect", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(<Harness initial="10.0.0.5" onSubmit={onSubmit} />);
    await user.click(box());
    await user.keyboard("{Enter}");
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it("without onSubmit, Enter is left to the form around the box", () => {
    const submit = vi.fn((event: { preventDefault: () => void }) => event.preventDefault());
    render(<form onSubmit={submit}><Harness initial="10.0.0.5" /><button type="submit">Go</button></form>);
    box().focus();
    fireEvent.keyDown(box(), { key: "Enter" });
    expect(box()).toHaveValue("10.0.0.5");
    // jsdom does not submit on a keydown; what matters is that the box did not stop the key.
    const event = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
    box().dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
  });
});

describe("AddressBox: what the list holds", () => {
  it("has two labelled groups, Scanned then Recent, and nothing else", async () => {
    const user = userEvent.setup();
    remember("10.0.0.5", "10.0.0.6");
    render(<Harness scanned={SCANNED} />);
    await user.click(arrow());
    const list = screen.getByRole("listbox");
    const groups = within(list).getAllByRole("group");
    expect(groups.map((group) => group.getAttribute("aria-labelledby") && document.getElementById(group.getAttribute("aria-labelledby") as string)?.textContent)).toEqual(["Scanned", "Recent"]);
    expect(within(groups[0]!).getAllByRole("option").map((option) => option.textContent)).toEqual(["Playback (front) (10.30.0.141)", "Playback (stage) (10.30.0.152)"]);
    expect(within(groups[1]!).getAllByRole("option").map((option) => option.textContent)).toEqual(["10.0.0.5", "10.0.0.6"]);
    expect(within(list).queryByText(/other computer/i)).toBeNull();
    expect(screen.getByRole("group", { name: "Scanned" })).toBe(groups[0]);
    expect(screen.getByRole("group", { name: "Recent" })).toBe(groups[1]);
  });

  it("hides the Scanned group when there is no scan result, and the Recent group when nothing worked yet", async () => {
    const user = userEvent.setup();
    remember("10.0.0.5");
    const { unmount } = render(<Harness />);
    await user.click(arrow());
    expect(screen.queryByRole("group", { name: "Scanned" })).toBeNull();
    expect(screen.getByRole("group", { name: "Recent" })).toBeInTheDocument();
    unmount();
    window.localStorage.clear();
    render(<Harness scanned={SCANNED} />);
    await user.click(arrow());
    expect(screen.getByRole("group", { name: "Scanned" })).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Recent" })).toBeNull();
  });

  it("lists the newest recent address first, at most five", async () => {
    const user = userEvent.setup();
    for (const host of ["10.0.0.1", "10.0.0.2", "10.0.0.3", "10.0.0.4", "10.0.0.5", "10.0.0.6", "10.0.0.7"]) rememberAddress("playback", host);
    render(<Harness />);
    await user.click(arrow());
    expect(names()).toEqual(["10.0.0.7", "10.0.0.6", "10.0.0.5", "10.0.0.4", "10.0.0.3"]);
  });

  it("shows an address that is both scanned and recent once, under Scanned", async () => {
    const user = userEvent.setup();
    remember("10.30.0.141", "10.0.0.5");
    render(<Harness scanned={SCANNED} />);
    await user.click(arrow());
    expect(names()).toEqual(["Playback (front) (10.30.0.141)", "Playback (stage) (10.30.0.152)", "10.0.0.5"]);
    expect(within(screen.getByRole("group", { name: "Recent" })).getAllByRole("option")).toHaveLength(1);
  });

  it("marks the address that is in the box with a check and words (not by colour), only when it is in the list", async () => {
    const user = userEvent.setup();
    remember("10.0.0.5", "10.0.0.6");
    render(<Harness initial=" 10.0.0.6 " />);
    await user.click(arrow());
    const current = screen.getByRole("option", { name: /10\.0\.0\.6/ });
    expect(current).toHaveAttribute("data-current", "true");
    expect(current.querySelector("[data-part=check]")).not.toBeNull();
    expect(current).toHaveTextContent("(in the box now)");
    const other = screen.getByRole("option", { name: "10.0.0.5" });
    expect(other).not.toHaveAttribute("data-current");
    expect(other.querySelector("[data-part=check]")).toBeNull();
  });

  it("says there is nothing yet, in a calm line (not an alert), and keeps the arrow", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(arrow());
    expect(screen.getByText(NO_ADDRESSES)).toBeInTheDocument();
    expect(NO_ADDRESSES).toBe("No recent or scanned addresses yet.");
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryAllByRole("option")).toHaveLength(0);
    expect(box()).toHaveAttribute("aria-expanded", "true");
    expect(box().getAttribute("aria-controls")).toBe(screen.getByText(NO_ADDRESSES).id);
    await user.keyboard("{ArrowDown}{Enter}");
    expect(screen.queryByText(NO_ADDRESSES)).toBeNull();
  });

  it("the list is as wide as the box, scrolls past its max height, and sits above what is below it", async () => {
    const user = userEvent.setup();
    const many = Array.from({ length: 12 }, (_, i) => ({ host: `10.0.1.${i}`, label: `Computer ${i} (10.0.1.${i})` }));
    render(<Harness scanned={many} />);
    await user.click(arrow());
    const list = screen.getByRole("listbox").parentElement as HTMLElement;
    expect(list).toHaveClass("absolute", "left-0", "right-0", "max-h-60", "overflow-y-auto", "z-20");
    expect(screen.getAllByRole("option")).toHaveLength(12);
  });

  it("follows the recent addresses when one is remembered while it is open", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(arrow());
    expect(screen.getByText(NO_ADDRESSES)).toBeInTheDocument();
    rememberAddress("playback", "10.0.0.9");
    expect(await screen.findByRole("option", { name: "10.0.0.9" })).toBeInTheDocument();
  });

  it("remembers the address that is connected right now", () => {
    render(<Harness working="10.0.0.42" />);
    expect(JSON.parse(window.localStorage.getItem("stagepilot.recent-addresses.playback") ?? "[]")).toEqual(["10.0.0.42"]);
  });

  it("keeps the three places apart (the scope)", async () => {
    const user = userEvent.setup();
    rememberAddress("lighting", "10.0.0.5");
    render(<Harness />);
    await user.click(arrow());
    expect(screen.getByText(NO_ADDRESSES)).toBeInTheDocument();
  });

  it("takes the recent addresses from the `recent` prop when one is given (the gallery)", async () => {
    const user = userEvent.setup();
    render(<Harness recent={["192.0.2.1"]} />);
    await user.click(arrow());
    expect(names()).toEqual(["192.0.2.1"]);
  });
});

describe("AddressBox: states", () => {
  it("disabled: the box and the arrow do nothing, the list cannot open, and one that was open closes", async () => {
    const user = userEvent.setup();
    remember("10.0.0.5");
    const { rerender } = render(<AddressBox aria-label="Host" defaultOpen disabled onChange={() => undefined} scope="playback" value="x" />);
    expect(box()).toBeDisabled();
    expect(arrow()).toBeDisabled();
    expect(screen.queryByRole("listbox")).toBeNull();
    await user.click(arrow());
    expect(screen.queryByRole("listbox")).toBeNull();
    rerender(<AddressBox aria-label="Host" defaultOpen onChange={() => undefined} scope="playback" value="x" />);
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    rerender(<AddressBox aria-label="Host" disabled onChange={() => undefined} scope="playback" value="x" />);
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("error: the box is invalid and points at the error and held-change text it was given", () => {
    render(
      <>
        <AddressBox aria-describedby="held-id error-id" aria-label="Host" error held onChange={() => undefined} scope="lighting" value="bad host" />
        <span id="held-id">Not saved yet</span><span id="error-id">Enter a host.</span>
      </>,
    );
    expect(box()).toBeInvalid();
    expect(box()).toHaveAccessibleDescription("Not saved yet Enter a host.");
    expect(box()).toHaveClass("!border-warning");
  });

  it("keeps the placeholder, the title and the field's own label", () => {
    render(<Field label="Computer"><AddressBox aria-label="Host" onChange={() => undefined} placeholder="192.0.2.10" scope="lighting" title="The computer" value="" /></Field>);
    expect(box()).toHaveAttribute("placeholder", "192.0.2.10");
    expect(box()).toHaveAttribute("title", "The computer");
  });

  it("sizes the box like every Live Link field and the arrow's touch target to 44 px on a coarse pointer", () => {
    render(<Harness />);
    expect(box()).toHaveClass("!h-10", "!min-h-10", "coarse:!h-11", "coarse:!min-h-[44px]", "!pr-8", "coarse:!pr-11");
    expect(arrow()).toHaveClass("w-8", "coarse:w-11", "inset-y-0");
  });
});

describe("addressGroups and scannedAddresses", () => {
  const scan = (over: Partial<ScanState>): ScanState => ({ ...NO_SCAN, ...over });

  it("offers a scan's candidates only while the scan FOUND something, and not a one-address Connect", () => {
    const candidates = [{ host: "10.0.0.1", label: "Front" }, { host: "10.0.0.2", label: "10.0.0.2" }];
    expect(scannedAddresses(scan({ state: "found", candidates }))).toEqual([{ host: "10.0.0.1", label: "Front (10.0.0.1)", port: undefined }, { host: "10.0.0.2", label: "10.0.0.2", port: undefined }]);
    for (const state of ["idle", "scanning", "not_found"] as const) expect(scannedAddresses(scan({ state, candidates }))).toEqual([]);
    expect(scannedAddresses(scan({ state: "found", typed: true, candidates }))).toEqual([]);
  });

  it("shows an address once, in any letter case, and never more than five recent", () => {
    const groups = addressGroups([{ host: "Stage.local", label: "Stage (Stage.local)" }, { host: "stage.LOCAL", label: "again" }], ["stage.local", "a", "b", "c", "d", "e"]);
    expect(groups.scanned.map((o) => o.host)).toEqual(["Stage.local"]);
    expect(groups.recent.map((o) => o.host)).toEqual(["a", "b", "c", "d"]);
  });
});
