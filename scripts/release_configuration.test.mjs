import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

const root = path.resolve(import.meta.dirname, "..");
const read = (file) => fs.readFileSync(path.join(root, file), "utf8").replace(/\r\n/g, "\n");

test("Tauri updater configuration points at the one release feed", () => {
  const config = JSON.parse(read("desktop/src-tauri/tauri.conf.json"));
  const windowsConfig = JSON.parse(read("desktop/src-tauri/tauri.release.conf.json"));
  const macOSConfig = JSON.parse(read("desktop/src-tauri/tauri.macos.conf.json"));
  assert.equal(config.productName, "StagePilot");
  assert.equal(config.identifier, "org.stagepilot.desktop");
  assert.equal(config.app.windows[0].label, "main");
  assert.equal(config.bundle.createUpdaterArtifacts, true);
  assert.equal(config.bundle.macOS.signingIdentity, "-");
  assert.equal(macOSConfig.bundle.macOS.signingIdentity, "-");
  assert.equal(macOSConfig.bundle.macOS.hardenedRuntime, false);
  assert.equal(macOSConfig.bundle.macOS.minimumSystemVersion, "12.0");
  assert.deepEqual(macOSConfig.bundle.targets, ["app", "dmg"]);
  assert.equal(
    macOSConfig.bundle.resources["generated-help/StagePilot.help/"],
    "StagePilot.help/",
  );
  assert.equal(
    macOSConfig.bundle.resources["generated-help/StagePilot.help/**"],
    undefined,
    "Tauri's trailing /** pattern matches directories rather than resource files",
  );
  assert.deepEqual(config.plugins.updater.endpoints, [
    "https://github.com/tage-ilot/stagepilot/releases/latest/download/latest.json",
  ]);
  // Packaged release builds must use the same direct, anonymous GitHub
  // releases endpoint as the base config — never the control-plane broker,
  // which returns 503 whenever GITHUB_RELEASE_TOKEN is unset (deliberately
  // unset).
  const directGitHubEndpoint =
    "https://github.com/tage-ilot/stagepilot/releases/latest/download/latest.json";
  const deadBrokerEndpoint =
    "https://stagepilot-beta-control-plane.stagepilot-illuminary-beta.workers.dev/v1/releases/latest.json";
  assert.deepEqual(windowsConfig.plugins.updater.endpoints, [directGitHubEndpoint]);
  assert.deepEqual(macOSConfig.plugins.updater.endpoints, [directGitHubEndpoint]);
  for (const endpoints of [
    config.plugins.updater.endpoints,
    windowsConfig.plugins.updater.endpoints,
    macOSConfig.plugins.updater.endpoints,
  ]) {
    assert.ok(!endpoints.includes(deadBrokerEndpoint));
  }
  assert.ok(config.plugins.updater.pubkey);
  assert.equal(
    config.bundle.windows.nsis.installerHooks,
    "windows/installer-hooks.nsh",
  );
});

test("Windows installer stops the backend before install and uninstall file operations", () => {
  const hooks = read("desktop/src-tauri/windows/installer-hooks.nsh");
  assert.match(hooks, /NSIS_HOOK_PREINSTALL/);
  assert.match(hooks, /NSIS_HOOK_PREUNINSTALL/);
  assert.match(
    hooks,
    /taskkill\.exe" \/F \/T \/IM "stagepilot-backend\.exe"/,
  );
  assert.match(hooks, /Sleep 1000/);
});

test("Windows Jump List stores titles in COM-owned memory", () => {
  const jumpList = read("desktop/src-tauri/src/windows_jump_list.rs");
  assert.match(jumpList, /SHStrDupW\(PCWSTR\(title\.as_ptr\(\)\)\)/);
  assert.match(jumpList, /pwszVal: owned_title/);
  assert.doesNotMatch(jumpList, /pwszVal: PWSTR\(title\.as_mut_ptr\(\)\)/);
});

test("macOS bundle registers and builds searchable StagePilot Help", () => {
  const infoPlist = read("desktop/src-tauri/Info.plist");
  const desktopPackage = JSON.parse(read("desktop/package.json"));
  assert.match(infoPlist, /CFBundleHelpBookFolder/);
  assert.match(infoPlist, /<string>StagePilot\.help<\/string>/);
  assert.match(infoPlist, /CFBundleHelpBookName/);
  assert.match(infoPlist, /<string>org\.stagepilot\.desktop\.help<\/string>/);
  assert.match(desktopPackage.scripts["build:mac"], /build:help/);
  assert.equal(desktopPackage.scripts["build:help"], "node ../scripts/build_macos_help.mjs");
});
test("desktop capability is narrow and contains updater lifecycle permissions", () => {
  const capability = JSON.parse(read("desktop/src-tauri/capabilities/default.json"));
  assert.ok(capability.permissions.includes("updater:default"));
  assert.ok(capability.permissions.includes("process:allow-restart"));
  assert.ok(capability.permissions.includes("window-state:default"));
  for (const dangerous of ["shell:allow-execute", "shell:allow-open", "fs:default"]) {
    assert.ok(!capability.permissions.includes(dangerous));
  }
});

test("desktop Remote packages cloudflared and uses only native platform credential stores", () => {
  const windows = JSON.parse(read("desktop/src-tauri/tauri.release.conf.json"));
  const windowsCI = JSON.parse(read("desktop/src-tauri/tauri.ci.conf.json"));
  const macOS = JSON.parse(read("desktop/src-tauri/tauri.macos.conf.json"));
  const cargo = read("desktop/src-tauri/Cargo.toml");
  const broker = read("desktop/src-tauri/src/native_credentials.rs");
  assert.equal(windows.bundle.resources["resources/cloudflared.exe"], "cloudflared.exe");
  assert.equal(
    windowsCI.bundle.resources["resources/cloudflared.exe"],
    "cloudflared.exe",
    "the CI installer must exercise the same self-contained Remote runtime",
  );
  assert.equal(macOS.bundle.resources["resources/cloudflared"], "cloudflared");
  for (const resources of [
    windows.bundle.resources,
    windowsCI.bundle.resources,
    macOS.bundle.resources,
  ]) {
    assert.ok(!Object.keys(resources).some((name) => /bootstrap|credential|token|secret|\.pem$/i.test(name)));
  }
  assert.match(cargo, /features = \["windows-native"\]/);
  assert.match(cargo, /features = \["apple-native"\]/);
  assert.match(broker, /org\.stagepilot\.desktop\.remote/);
  assert.match(broker, /Cache-Control: no-store/);
});

test("release workflow requires secrets and publishes latest.json last", () => {
  const workflow = read(".github/workflows/release-macos.yml");
  const deployWorkflow = read(".github/workflows/deploy-control-plane.yml");
  assert.match(workflow, /secrets\.TAURI_SIGNING_PRIVATE_KEY/);
  assert.match(workflow, /secrets\.TAURI_SIGNING_PRIVATE_KEY_PASSWORD/);
  assert.match(workflow, /generate_updater_manifest\.mjs/);
  assert.match(workflow, /validate_updater_manifest\.mjs/);
  assert.match(workflow, /audit_release\.mjs source/);
  assert.match(workflow, /audit_release\.mjs assets/);
  assert.match(workflow, /node scripts\/publish_release\.mjs "\$RELEASE_TAG" release-assets/);
  assert.equal((workflow.match(/ref: \$\{\{ inputs\.release_tag \|\| github\.ref \}\}/g) ?? []).length, 3);
  assert.match(workflow, /refs\/tags\/\$RELEASE_TAG\^\{commit\}/);
  assert.match(workflow, /MACOSX_DEPLOYMENT_TARGET: "12\.0"/);
  assert.match(workflow, /verify_macos_release_bundle\.sh --app/);
  assert.match(workflow, /verify_macos_release_bundle\.sh --dmg/);
  assert.match(workflow, /verify_macos_release_bundle\.sh --archive/);
  assert.ok(
    workflow.indexOf("Generate macOS Help Book") < workflow.indexOf("Validate application"),
    "the release workflow must generate bundled Help before Cargo validates Tauri resources",
  );
  assert.match(
    workflow,
    /name: Generate macOS Help Book\s+if: runner\.os == 'macOS'\s+run: npm --prefix desktop run build:help/,
  );
  assert.ok(
    workflow.indexOf("verify_macos_release_bundle.sh --archive") <
      workflow.indexOf("Collect macOS release assets"),
  );
  assert.doesNotMatch(workflow, /APPLE_(?:CERTIFICATE|SIGNING_IDENTITY|ID|PASSWORD)/);
  assert.doesNotMatch(workflow, /notarytool|notariz/);
  assert.match(workflow, /Copy-Item \$installer\.FullName/);
  assert.match(workflow, /Copy-Item "\$\(\$installer\.FullName\)\.sig"/);
  // The publish job does not depend on the `gh` CLI, so the release is created
  // through the REST API by this script. The
  // invariants it must preserve are asserted directly against its source.
  const publisher = read("scripts/publish_release.mjs");
  assert.doesNotMatch(workflow, /\bgh release\b/, "the publish job must not depend on the gh CLI");
  assert.match(
    publisher,
    /!name\.endsWith\("\.sig"\)/,
    "standalone updater signatures must not be published as user-facing release assets",
  );
  assert.match(publisher, /Release \$\{tag\} already exists; immutable release assets will not be replaced/);
  assert.match(publisher, /process\.exit\(1\)/, "an existing release must abort publication");
  assert.ok(
    publisher.includes('filter((n) => n !== "latest.json"), ...entries.filter((n) => n === "latest.json")'),
    "latest.json must be uploaded after every artifact it references",
  );
  assert.match(publisher, /draft: true/, "the release must be created as a draft");
  assert.match(publisher, /draft: false/, "the release must be published only after all uploads succeed");
  assert.doesNotMatch(workflow, /--clobber/);
  // The release broker/in-app updater are deferred: no
  // STAGEPILOT_RELEASE_TOKEN Actions secret is issued, and the deploy
  // workflow must never require or reference one.
  assert.doesNotMatch(deployWorkflow, /STAGEPILOT_RELEASE_TOKEN/);
  assert.doesNotMatch(deployWorkflow, /GITHUB_RELEASE_TOKEN/);
  // The publisher uploads every other asset first, then latest.json, and only
  // then flips the draft off — so no client can ever see a manifest that
  // references an asset the release does not yet carry.
  assert.ok(
    publisher.indexOf("uploaded ${name}") < publisher.indexOf("draft: false"),
    "assets must all be uploaded before the release leaves draft",
  );
  assert.ok(
    publisher.indexOf("latest.json is missing from the staging directory") <
      publisher.indexOf("draft: false"),
    "publication must abort when latest.json was never generated",
  );
});

test("macOS lifecycle CI generates bundled Help before Cargo compilation", () => {
  const workflow = read(".github/workflows/ci.yml");
  const macOSJob = workflow.slice(workflow.indexOf("desktop-macos-lifecycle:"));
  assert.ok(macOSJob.includes("Generate macOS Help Book"));
  assert.ok(
    macOSJob.indexOf("Generate macOS Help Book") <
      macOSJob.indexOf("Compile-check macOS desktop shell"),
  );
  assert.match(
    macOSJob,
    /name: Generate macOS Help Book\s+run: npm --prefix desktop run build:help/,
  );
});

test("release automation targets only this repo, suggests the next patch and confirms with YES", () => {
  const releaseScript = read("scripts/create-release.ps1");
  const helpers = read("scripts/release-helpers.ps1");
  assert.match(releaseScript, /\$Repo = "tage-ilot\/stagepilot"/);
  assert.match(releaseScript, /gh api repos\/\$Repo\/tags --paginate --jq '\.\[\]\.name'/);
  assert.doesNotMatch(releaseScript, /stagepilot-(beta|alpha)/);
  assert.match(releaseScript, /Test-ReleaseOrigin \$OriginUrl/);
  assert.match(releaseScript, /Assert-ReleaseVersion \$Version \$RemoteTags/);
  assert.match(releaseScript, /Test-ChangelogHasVersion/);
  assert.match(releaseScript, /audit_release\.mjs source/);
  assert.match(releaseScript, /validate_workflow_runners\.py/);
  assert.match(releaseScript, /New StagePilot version \(default \$SuggestedTag\)/);
  assert.match(releaseScript, /Type YES to begin the unattended release/);
  assert.match(releaseScript, /ToUpperInvariant\(\) -ne "YES"/);
  assert.match(releaseScript, /STAGEPILOT_RELEASE_WATCH/);
  assert.match(helpers, /\^v\(\\d\+\)\\\.\(\\d\+\)\\\.\(\\d\+\)\$/);
  assert.match(helpers, /tage-ilot\/stagepilot/);
  assert.doesNotMatch(helpers, /beta/i);
});

test("macOS final-artifact verifier checks signing, minimum macOS, Help and the running app", () => {
  const verifier = read("scripts/verify_macos_release_bundle.sh");
  assert.match(verifier, /codesign --verify --deep --strict/);
  assert.match(verifier, /flags=\.\*runtime/);
  assert.match(verifier, /vtool -show-build/);
  assert.match(verifier, /LC_VERSION_MIN_MACOSX/);
  assert.match(verifier, /otool -l/);
  assert.match(verifier, /api\/v1\/health/);
  assert.match(verifier, /api\/v1\/dashboard-auth\/login/);
  assert.match(verifier, /CFBundleHelpBookFolder/);
  assert.match(verifier, /StagePilot\.helpindex/);
  assert.match(verifier, /--cookie-jar/);
  assert.match(verifier, /api\/v1\/state/);
  assert.match(verifier, /hdiutil attach/);
  assert.match(verifier, /tar -xzf/);
});

test("manifest generator requires artifacts and signatures for every platform", () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "stagepilot-updater-"));
  try {
    for (const file of [
      "StagePilot_1.2.0_aarch64.app.tar.gz",
      "StagePilot_1.2.0_x64.app.tar.gz",
      "StagePilot_1.2.0_x64-setup.exe",
    ]) {
      fs.writeFileSync(path.join(directory, file), "artifact");
      fs.writeFileSync(path.join(directory, `${file}.sig`), `signature-${file}`);
    }
    execFileSync(process.execPath, [
      path.join(root, "scripts/generate_updater_manifest.mjs"),
      directory,
      "v1.2.0",
      "https://stagepilot-beta-control-plane.stagepilot-illuminary-beta.workers.dev/v1/releases",
    ]);
    execFileSync(process.execPath, [
      path.join(root, "scripts/validate_updater_manifest.mjs"),
      path.join(directory, "latest.json"),
      directory,
      "https://stagepilot-beta-control-plane.stagepilot-illuminary-beta.workers.dev/v1/releases",
    ]);
    const manifest = JSON.parse(fs.readFileSync(path.join(directory, "latest.json"), "utf8"));
    assert.equal(manifest.version, "1.2.0");
    assert.deepEqual(Object.keys(manifest.platforms).sort(), [
      "darwin-aarch64",
      "darwin-x86_64",
      "windows-x86_64",
    ]);
    manifest.platforms["darwin-aarch64"].url =
      "https://stagepilot-beta-control-plane.stagepilot-illuminary-beta.workers.dev/v1/releases/v1.2.0/%2e%2e%2foutside.bin";
    fs.writeFileSync(path.join(directory, "latest.json"), JSON.stringify(manifest));
    assert.throws(() => execFileSync(process.execPath, [
      path.join(root, "scripts/validate_updater_manifest.mjs"),
      path.join(directory, "latest.json"),
      directory,
      "https://stagepilot-beta-control-plane.stagepilot-illuminary-beta.workers.dev/v1/releases",
    ], { stdio: "pipe" }));
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test("repository does not track private updater key files", () => {
  const tracked = execFileSync("git", ["ls-files"], { cwd: root, encoding: "utf8" });
  assert.doesNotMatch(tracked, /\.(key|pem)$/m);
});

test("pre-release tags are published as pre-releases and can never become Latest", () => {
  const publisher = read("scripts/publish_release.mjs");
  assert.match(publisher, /const prerelease = tag\.includes\("-"\)/);
  assert.match(publisher, /\(\?:-\(\?:alpha\|beta\|rc\)\\\.\\d\+\)\?/, "only alpha/beta/rc suffixes are accepted");
  assert.match(publisher, /prerelease \? \{ draft: false, prerelease: true \} : \{ draft: false, make_latest: "true" \}/);
  // make_latest "true" appears exactly once, on the stable-only branch.
  assert.equal((publisher.match(/make_latest: "true"/g) ?? []).length, 1);
  // Request-level behaviour is proven with a mocked fetch in scripts/publish_release.test.mjs.
  assert.match(read("desktop/package.json"), /publish_release\.test\.mjs/);
});

test("updater manifest tooling accepts a pre-release version and keeps it under its own tag URL", () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "stagepilot-rc-manifest-"));
  try {
    const version = "2.42.0-rc.1";
    for (const file of [
      `StagePilot_${version}_aarch64.app.tar.gz`,
      `StagePilot_${version}_x64.app.tar.gz`,
      `StagePilot_${version}_x64-setup.exe`,
    ]) {
      fs.writeFileSync(path.join(directory, file), "artifact");
      fs.writeFileSync(path.join(directory, `${file}.sig`), `signature-${file}`);
    }
    const base = "https://github.com/tage-ilot/stagepilot/releases/download";
    execFileSync(process.execPath, [path.join(root, "scripts/generate_updater_manifest.mjs"), directory, `v${version}`, base]);
    execFileSync(process.execPath, [path.join(root, "scripts/validate_updater_manifest.mjs"), path.join(directory, "latest.json"), directory, base]);
    const manifest = JSON.parse(fs.readFileSync(path.join(directory, "latest.json"), "utf8"));
    assert.equal(manifest.version, version);
    for (const entry of Object.values(manifest.platforms)) {
      assert.ok(entry.url.startsWith(`${base}/v${version}/`), "assets are addressed by the pre-release's own tag");
    }
    // latest.json is itself a release asset of v2.42.0-rc.1: it is served from
    // /releases/download/v2.42.0-rc.1/latest.json, and /releases/latest/download/latest.json only
    // ever resolves to the newest non-draft, non-pre-release release.
    const audit = (v) => execFileSync(process.execPath, [path.join(root, "scripts/audit_release.mjs"), "assets", directory, v], { stdio: "pipe" });
    fs.writeFileSync(path.join(directory, "release-notes.md"), "notes");
    for (const name of [`StagePilot_${version}_aarch64.dmg`, `StagePilot_${version}_x64.dmg`]) fs.writeFileSync(path.join(directory, name), "dmg");
    audit(version);
    assert.throws(() => audit("2.42.0-nightly.1"), /version is required/);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test("release workflow builds the same app for stable and pre-release tags", () => {
  const workflow = read(".github/workflows/release-macos.yml");
  assert.match(workflow, /\(-\(alpha\|beta\|rc\)\\\.\[0-9\]\+\)\?\$ \]\]/, "stable and pre-release tags are both accepted");
  assert.doesNotMatch(workflow, /variant/i, "there is one installer build");
  assert.doesNotMatch(workflow, /setup-uv|uv sync/, "no Python project is installed");
  // The validation commands live in one script shared by every release path.
  const validation = read("scripts/validate_application.sh");
  assert.match(validation, /npm --prefix desktop run build:runtime\n/);
  assert.doesNotMatch(validation, /uv run|pytest|ruff|mypy|backend\//);
  for (const file of ["release-macos.yml", "release-windows.yml", "release-validate.yml"]) {
    assert.match(read(`.github/workflows/${file}`), /scripts\/validate_application\.sh/, file);
  }
  assert.match(read("scripts/create-release.ps1"), /validate_application\.sh/);
  assert.match(workflow, /npm --prefix desktop run \$\{\{ matrix\.command \}\}/);
  assert.match(workflow, /command: build:mac\n/);
  assert.match(workflow, /verify_macos_release_bundle\.sh --app .* --no-launch/);
});

test("installer configs bundle no sidecar, keep cloudflared and the frontend, and leave the updater unchanged", () => {
  const pkg = JSON.parse(read("desktop/package.json")).scripts;
  const configs = {
    "tauri.release.conf.json": "resources/cloudflared.exe",
    "tauri.ci.conf.json": "resources/cloudflared.exe",
    "tauri.macos.conf.json": "resources/cloudflared",
    "tauri.macos.ci.conf.json": "resources/cloudflared",
  };
  for (const [file, cloudflared] of Object.entries(configs)) {
    const config = JSON.parse(read(`desktop/src-tauri/${file}`));
    assert.equal(config.bundle.externalBin, undefined, `${file} must not declare a sidecar`);
    assert.ok(config.bundle.resources[cloudflared], `${file} must stage cloudflared`);
    assert.equal(config.bundle.resources["../../frontend/dist/"], "stagepilot_web/", `${file} must embed the frontend`);
    assert.doesNotMatch(JSON.stringify(config), /stagepilot-backend/);
    const isCi = file.includes(".ci.");
    if (isCi) assert.equal(config.bundle.createUpdaterArtifacts, false, "CI builds carry no updater signing");
    else assert.equal(config.plugins.updater.endpoints[0], "https://github.com/tage-ilot/stagepilot/releases/latest/download/latest.json");
  }
  for (const name of ["build", "build:ci", "build:mac", "build:mac:ci", "build:runtime"]) {
    assert.doesNotMatch(pkg[name], /sidecar|build_backend|pyinstaller|--features/i, name);
  }
  assert.equal(pkg["build:runtime"], "npm --prefix ../frontend run build && npm run build:cloudflared");
  assert.match(pkg.build, /build:runtime && tauri build --config src-tauri\/tauri\.release\.conf\.json/);
  assert.match(pkg["build:ci"], /build:runtime && tauri build --config src-tauri\/tauri\.ci\.conf\.json/);
  assert.match(pkg["build:mac"], /build:runtime && tauri build --config src-tauri\/tauri\.macos\.conf\.json/);
  assert.match(pkg["build:mac:ci"], /build:runtime && tauri build --config src-tauri\/tauri\.macos\.ci\.conf\.json/);
  for (const name of Object.keys(pkg)) assert.ok(!/rust|sidecar/.test(name), `${name}: no variant-specific scripts remain`);
});

test("installer CI is manual, unpublished, secret-free and short-lived", () => {
  const workflow = read(".github/workflows/rust-installer-ci.yml");
  assert.match(workflow, /on:\s+workflow_dispatch:[\s\S]*?permissions:\s+contents: read/);
  assert.doesNotMatch(workflow, /pull_request|schedule:|release:|tags:/);
  assert.doesNotMatch(workflow, /secrets\./);
  assert.doesNotMatch(workflow, /environment:/);
  assert.doesNotMatch(workflow, /TAURI_SIGNING|publish_release|gh release/);
  assert.doesNotMatch(workflow, /self-hosted/);
  assert.match(workflow, /retention-days: 3/);
  assert.match(workflow, /build:mac:ci/);
  assert.match(workflow, /verify_macos_release_bundle\.sh --app .* --no-launch/);
});

test("macOS verifier requires the in-process server and rejects any Python backend", () => {
  const verifier = read("scripts/verify_macos_release_bundle.sh");
  assert.doesNotMatch(verifier, /VARIANT|--variant/);
  assert.match(verifier, /separate backend executable or Python runtime files/);
  assert.match(verifier, /stagepilot_web\/index\.html/);
  assert.match(verifier, /A Python backend process was spawned by StagePilot/);
  assert.match(verifier, /Application unexpectedly has the Hardened Runtime flag/);
});
