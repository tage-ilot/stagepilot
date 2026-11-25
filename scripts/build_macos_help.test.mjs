import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { buildHelpBook, renderMarkdown } from "./build_macos_help.mjs";

test("Markdown renderer produces searchable safe HTML", () => {
  const html = renderMarkdown("# MIDI setup\n\nUse **E7** and `112`.\n\n<script>alert(1)</script>");
  assert.match(html, /<h1 id="midi-setup">MIDI setup<\/h1>/);
  assert.match(html, /<strong>E7<\/strong>/);
  assert.match(html, /<code>112<\/code>/);
  assert.doesNotMatch(html, /<script>/);
  assert.match(html, /&lt;script&gt;/);
});

test("help builder converts repository Markdown into an Apple Help Book", () => {
  const fixture = fs.mkdtempSync(path.join(os.tmpdir(), "stagepilot-help-"));
  try {
    const source = path.join(fixture, "source");
    const output = path.join(fixture, "generated");
    fs.mkdirSync(path.join(source, "docs"), { recursive: true });
    fs.writeFileSync(path.join(source, "README.md"), "# StagePilot\n\nLive production.");
    fs.writeFileSync(path.join(source, "docs", "midi.md"), "# MIDI\n\nConfigure Playback.");
    fs.mkdirSync(path.join(source, "node_modules"), { recursive: true });
    fs.writeFileSync(path.join(source, "node_modules", "ignored.md"), "# Ignore");

    const result = buildHelpBook({ sourceRoot: source, outputRoot: output, createIndex: false });
    assert.equal(result.files, 2);
    const help = path.join(output, "StagePilot.help", "Contents");
    const appInfo = fs.readFileSync(path.join(help, "Info.plist"), "utf8");
    const index = fs.readFileSync(
      path.join(help, "Resources", "English.lproj", "index.html"),
      "utf8",
    );
    assert.match(appInfo, /org\.stagepilot\.desktop\.help/);
    assert.match(appInfo, /StagePilot\.helpindex/);
    assert.match(index, /AppleTitle/);
    assert.match(index, /pages\/docs-midi\.html/);
    assert.match(index, /pages\/readme\.html/);
  } finally {
    fs.rmSync(fixture, { recursive: true, force: true });
  }
});

