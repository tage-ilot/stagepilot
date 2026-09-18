import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const root = path.resolve(import.meta.dirname, "..");
const version = process.argv[2];

if (!version || version === "--help" || version === "-h") {
  console.log("Usage: node scripts/set-release-version.mjs VERSION");
  process.exit(version ? 0 : 2);
}

if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error(`Invalid semantic version: ${version}`);
}

const read = (file) => fs.readFileSync(path.join(root, file), "utf8");
const write = (file, content) => fs.writeFileSync(path.join(root, file), content, "utf8");
const currentVersion = JSON.parse(read("desktop/src-tauri/tauri.conf.json")).version;

const replaceOnce = (content, pattern, replacement, file) => {
  const matches = content.match(new RegExp(pattern.source, pattern.flags.includes("g") ? pattern.flags : `${pattern.flags}g`));
  if (!matches || matches.length !== 1) {
    throw new Error(`Expected exactly one version match in ${file}; found ${matches?.length ?? 0}.`);
  }
  return content.replace(pattern, replacement);
};

const replaceJsonRootVersion = (file) => {
  const content = read(file);
  const updated = replaceOnce(
    content,
    /^(\s*"version"\s*:\s*")[^"]+(")/m,
    `$1${version}$2`,
    file,
  );
  write(file, updated);
};

const replacePackageLockVersions = (file) => {
  const content = read(file);
  let count = 0;
  const updated = content.replace(
    /^(\s*"version"\s*:\s*")[^"]+(")/gm,
    (match, prefix, suffix) => {
      if (count >= 2) return match;
      count += 1;
      return `${prefix}${version}${suffix}`;
    },
  );
  if (count !== 2) throw new Error(`Expected two root version values in ${file}; found ${count}.`);
  write(file, updated);
};

for (const file of [
  "desktop/src-tauri/tauri.conf.json",
  "desktop/package.json",
  "frontend/package.json",
]) {
  replaceJsonRootVersion(file);
}

for (const file of ["desktop/package-lock.json", "frontend/package-lock.json"]) {
  replacePackageLockVersions(file);
}

const replacements = [
  ["backend/pyproject.toml", /^version = "[^"]+"/m, `version = "${version}"`],
  ["desktop/src-tauri/Cargo.toml", /^version = "[^"]+"/m, `version = "${version}"`],
  ["backend/src/stagepilot/__init__.py", /^__version__ = "[^"]+"/m, `__version__ = "${version}"`],
  [
    "backend/src/stagepilot/core/config.py",
    /default="StagePilot\/[^ ]+ \(https:\/\/github\.com\/tage-ilot\/stagepilot\)"/,
    `default="StagePilot/${version} (https://github.com/tage-ilot/stagepilot)"`,
  ],
  [
    "backend/src/stagepilot/core/config.py",
    /^\s+version: str = "[^"]+"/m,
    `    version: str = "${version}"`,
  ],
  [
    "backend/uv.lock",
    /(name = "stagepilot"\r?\nversion = ")[^"]+(")/,
    `$1${version}$2`,
  ],
  [
    "desktop/src-tauri/Cargo.lock",
    /(name = "stagepilot-desktop"\r?\nversion = ")[^"]+(")/,
    `$1${version}$2`,
  ],
];

for (const [file, pattern, replacement] of replacements) {
  const content = read(file);
  write(file, replaceOnce(content, pattern, replacement, file));
}

let readme = read("README.md");
readme = replaceOnce(
  readme,
  /^## Download StagePilot [^\r\n]+/m,
  `## Download StagePilot ${version}`,
  "README.md",
);
readme = readme.replace(
  /https:\/\/github\.com\/tage-ilot\/stagepilot\/releases\/(?:tag|download)\/v\d+\.\d+\.\d+/g,
  (url) => url.replace(/v\d+\.\d+\.\d+$/, `v${version}`),
);
readme = readme.replace(
  /\[StagePilot v\d+\.\d+\.\d+ release\]/,
  `[StagePilot v${version} release]`,
);
readme = readme.replace(
  /StagePilot_\d+\.\d+\.\d+_/g,
  `StagePilot_${version}_`,
);
write("README.md", readme);

if (currentVersion === version) {
  console.log(`StagePilot is already version ${version}; verified and refreshed all version sources.`);
} else {
  console.log(`Updated StagePilot from ${currentVersion} to ${version}.`);
}
