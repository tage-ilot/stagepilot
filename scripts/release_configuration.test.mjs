import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

const root = path.resolve(import.meta.dirname, "..");
const read = (file) => fs.readFileSync(path.join(root, file), "utf8");

test("Tauri updater configuration preserves stable identity and trusted endpoint", () => {
  const config = JSON.parse(read("desktop/src-tauri/tauri.conf.json"));
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

test("release workflow requires secrets and publishes latest.json last", () => {
  const workflow = read(".github/workflows/release-macos.yml");
  assert.match(workflow, /secrets\.TAURI_SIGNING_PRIVATE_KEY/);
  assert.match(workflow, /secrets\.TAURI_SIGNING_PRIVATE_KEY_PASSWORD/);
  assert.match(workflow, /generate_updater_manifest\.mjs/);
  assert.match(workflow, /validate_updater_manifest\.mjs/);
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
  assert.match(
    workflow,
    /find release-assets .* ! -name '\*\.sig'/,
    "standalone updater signatures must not be published as user-facing release assets",
  );
  assert.ok(
    workflow.indexOf('! -name latest.json') < workflow.indexOf("release-assets/latest.json --clobber"),
  );
  assert.ok(
    workflow.indexOf("release-assets/latest.json --clobber") < workflow.indexOf("--draft=false --latest"),
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

test("release automation suggests the next published patch and confirms with YES", () => {
  const releaseScript = read("scripts/create-release.ps1");
  assert.match(
    releaseScript,
    /gh api repos\/tage-ilot\/stagepilot\/tags --paginate --jq '\.\[\]\.name'/,
  );
  assert.match(releaseScript, /Sort-Object -Property Version -Descending/);
  assert.match(
    releaseScript,
    /\$LatestGitHubVersion\.Version\.Build \+ 1/,
  );
  assert.match(releaseScript, /New StagePilot version \(default \$SuggestedTag\)/);
  assert.match(releaseScript, /Type YES to begin the unattended release/);
  assert.match(releaseScript, /ToUpperInvariant\(\) -ne "YES"/);
  assert.doesNotMatch(releaseScript, /Type RELEASE \$Tag TO \$TargetBranch/);
});

test("macOS final-artifact verifier rejects hardened sidecars and starts packaged backends", () => {
  const verifier = read("scripts/verify_macos_release_bundle.sh");
  assert.match(verifier, /codesign --verify --deep --strict/);
  assert.match(verifier, /flags=\.\*runtime/);
  assert.match(verifier, /TeamIdentifier/);
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

test("PyInstaller uses ordinary ad-hoc signing without a custom identity or entitlements", () => {
  const specification = read("backend/stagepilot.spec");
  assert.match(specification, /codesign_identity=None/);
  assert.match(specification, /entitlements_file=None/);
  assert.doesNotMatch(specification, /Developer ID Application/);
  assert.doesNotMatch(specification, /disable-library-validation/);
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
    ]);
    execFileSync(process.execPath, [
      path.join(root, "scripts/validate_updater_manifest.mjs"),
      path.join(directory, "latest.json"),
      directory,
    ]);
    const manifest = JSON.parse(fs.readFileSync(path.join(directory, "latest.json"), "utf8"));
    assert.equal(manifest.version, "1.2.0");
    assert.deepEqual(Object.keys(manifest.platforms).sort(), [
      "darwin-aarch64",
      "darwin-x86_64",
      "windows-x86_64",
    ]);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test("repository does not track private updater key files", () => {
  const tracked = execFileSync("git", ["ls-files"], { cwd: root, encoding: "utf8" });
  assert.doesNotMatch(tracked, /\.(key|pem)$/m);
});
