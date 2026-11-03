import { describe, expect, it } from "vitest";

import { apiHostMismatch, apiPortError, composeApiUrl, splitApiUrl } from "./lightkeyConnection";

describe("splitApiUrl", () => {
  it.each([
    ["http://192.0.2.10:8770", { host: "192.0.2.10", port: "8770" }],
    ["192.0.2.10:9000", { host: "192.0.2.10", port: "9000" }],
    ["https://rig.local", { host: "rig.local", port: "8770" }],
    ["http://[::1]:8771", { host: "[::1]", port: "8771" }],
    ["", { host: "", port: "8770" }],
  ])("%s", (url, expected) => expect(splitApiUrl(url)).toEqual(expected));
});

describe("composeApiUrl", () => {
  it.each([
    ["same host, default port: left empty so the backend derives it", "192.0.2.10", "8770", "192.0.2.10", ""],
    ["same host ignoring case and spaces", " Rig.Local ", "8770", "rig.local", ""],
    ["same host, other port", "192.0.2.10", "9000", "192.0.2.10", "192.0.2.10:9000"],
    ["host not yet saved as the OSC host", "192.0.2.11", "8770", "192.0.2.10", "192.0.2.11:8770"],
  ])("%s", (_name, host, apiPort, savedOscHost, expected) => {
    expect(composeApiUrl({ host, apiPort, savedOscHost })).toBe(expected);
  });
});

describe("apiHostMismatch", () => {
  it("is null when the API host is the OSC host", () => expect(apiHostMismatch("http://192.0.2.10:8770", "192.0.2.10")).toBeNull());
  it("names the other host", () => expect(apiHostMismatch("http://192.0.2.99:8770", "192.0.2.10")).toBe("192.0.2.99"));
  it("is null when there is no API address yet", () => expect(apiHostMismatch("", "192.0.2.10")).toBeNull());
});

describe("apiPortError", () => {
  it.each([["8770", false], ["1", false], ["65535", false], ["0", true], ["65536", true], ["", true], ["abc", true], ["12.5", true]])("%s", (port, bad) => {
    expect(apiPortError(port) !== null).toBe(bad);
  });
});
