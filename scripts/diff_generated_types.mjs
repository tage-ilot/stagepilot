#!/usr/bin/env node
// Report-only: list differences between the TypeScript generated from the Rust models
// (generated/ts/*.ts) and the hand-written types in frontend/src/types.ts.
//
//   node scripts/diff_generated_types.mjs            print the report, exit 0
//   node scripts/diff_generated_types.mjs --strict   exit 1 when any mismatch is found
//
// It compares, per type name present on both sides: object field names, field optionality
// (`a?: T` vs `a: T | null`) and string-literal union members. It does not compare field types
// beyond that, and it never edits the frontend.
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const strict = process.argv.includes("--strict");

const stripComments = (text) => text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, "");

function balanced(text, start) {
  let depth = 0;
  for (let i = start; i < text.length; i += 1) {
    if ("{([<".includes(text[i]) && !(text[i] === "<" && text[i - 1] === "=")) depth += 1;
    else if ("})]>".includes(text[i]) && !(text[i] === ">" && text[i - 1] === "=")) {
      depth -= 1;
      if (depth === 0) return i;
    }
  }
  return text.length;
}

function splitTop(body) {
  const parts = [];
  let depth = 0;
  let current = "";
  for (let i = 0; i < body.length; i += 1) {
    const c = body[i];
    if ("{([<".includes(c) && !(c === "<" && body[i - 1] === "=")) depth += 1;
    if ("})]>".includes(c) && !(c === ">" && body[i - 1] === "=")) depth -= 1;
    if (depth === 0 && (c === "," || c === ";" || c === "\n")) {
      if (current.trim()) parts.push(current.trim());
      current = "";
    } else current += c;
  }
  if (current.trim()) parts.push(current.trim());
  return parts;
}

// Returns Map(name -> {fields: Map(name -> {optional, type}) | null, literals: Set | null})
function parse(text) {
  const out = new Map();
  const clean = stripComments(text);
  const header = /export\s+(interface|type)\s+(\w+)\s*(?:<[^>]*>)?\s*=?\s*/g;
  let match;
  while ((match = header.exec(clean))) {
    const [, kind, name] = match;
    let i = header.lastIndex;
    const entry = { fields: null, literals: null };
    if (clean[i] === "{") {
      const end = balanced(clean, i);
      entry.fields = new Map();
      for (const part of splitTop(clean.slice(i + 1, end))) {
        const m = /^(?:readonly\s+)?["']?([\w$]+)["']?\s*(\?)?\s*:\s*([\s\S]*)$/.exec(part);
        if (m) entry.fields.set(m[1], { optional: Boolean(m[2]), type: m[3].trim() });
      }
      header.lastIndex = end;
    } else if (kind === "type") {
      let end = clean.indexOf(";", i);
      if (end < 0) end = clean.length;
      const rhs = clean.slice(i, end);
      const literals = [...rhs.matchAll(/"([^"]*)"/g)].map((m) => m[1]);
      if (literals.length && !rhs.includes("{")) entry.literals = new Set(literals);
      header.lastIndex = end;
    }
    out.set(name, entry);
  }
  return out;
}

const generatedDir = join(root, "generated", "ts");
const generated = new Map();
for (const file of readdirSync(generatedDir).filter((f) => f.endsWith(".ts")).sort()) {
  for (const [name, entry] of parse(readFileSync(join(generatedDir, file), "utf8"))) {
    generated.set(name, entry);
  }
}
const frontend = parse(readFileSync(join(root, "frontend", "src", "types.ts"), "utf8"));

const lines = [];
const missingInFrontend = [];
let compared = 0;
for (const [name, rust] of [...generated].sort(([a], [b]) => a.localeCompare(b))) {
  const ts = frontend.get(name);
  if (!ts) {
    missingInFrontend.push(name);
    continue;
  }
  compared += 1;
  if (rust.fields && ts.fields) {
    for (const [field, info] of rust.fields) {
      const other = ts.fields.get(field);
      if (!other) lines.push(`${name}.${field}: in Rust, not in frontend`);
      else {
        const rustNullable = /\|\s*null\b/.test(info.type);
        const tsOptional = other.optional;
        const tsNullable = /\|\s*null\b/.test(other.type);
        if (tsOptional && !rustNullable)
          lines.push(`${name}.${field}: frontend marks it optional (?), Rust always sends it`);
        if (tsNullable !== rustNullable && !tsOptional)
          lines.push(`${name}.${field}: nullable in ${rustNullable ? "Rust" : "frontend"} only`);
      }
    }
    for (const field of ts.fields.keys())
      if (!rust.fields.has(field)) lines.push(`${name}.${field}: in frontend, not in Rust`);
  } else if (rust.literals && ts.literals) {
    for (const v of rust.literals) if (!ts.literals.has(v)) lines.push(`${name}: "${v}" in Rust, not in frontend`);
    for (const v of ts.literals) if (!rust.literals.has(v)) lines.push(`${name}: "${v}" in frontend, not in Rust`);
  } else if (Boolean(rust.fields) !== Boolean(ts.fields) || Boolean(rust.literals) !== Boolean(ts.literals)) {
    lines.push(`${name}: different kind of type (object / literal union / other)`);
  }
}

console.log(`Rust-generated types: ${generated.size}; compared with frontend/src/types.ts: ${compared}`);
if (missingInFrontend.length)
  console.log(`\nNo frontend type with the same name (${missingInFrontend.length}):\n  ${missingInFrontend.join("\n  ")}`);
if (lines.length) console.log(`\nMismatches (${lines.length}):\n  ${lines.join("\n  ")}`);
else console.log("\nNo field or literal mismatches among the compared types.");
process.exit(strict && lines.length ? 1 : 0);
