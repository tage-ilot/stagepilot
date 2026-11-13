const ID = /^[a-f0-9]{32}$/;
const HOSTNAME = /^[a-z0-9](?:[a-z0-9.-]{1,251}[a-z0-9])$/;

// GITHUB_RELEASE_TOKEN is deliberately excluded: the release broker/in-app
// updater are deferred for this beta (see docs/native-completion-runbook.md).
// No STAGEPILOT_RELEASE_TOKEN Actions secret is issued, so the deploy
// pipeline must not require or push a GITHUB_RELEASE_TOKEN Worker secret.
export const RUNTIME_SECRET_NAMES = Object.freeze([
  "CLOUDFLARE_API_TOKEN",
  "ADMIN_API_TOKEN",
  "INSTALLATION_SIGNING_KEY",
  "PLANNING_CENTER_CLIENT_ID",
  "PLANNING_CENTER_CLIENT_SECRET",
]);

function required(environment, name) {
  const value = environment[name];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`Missing required deployment value: ${name}`);
  }
  return value;
}

export function validateDeploymentEnvironment(environment) {
  const accountId = required(environment, "CLOUDFLARE_ACCOUNT_ID");
  const zoneId = required(environment, "CLOUDFLARE_ZONE_ID");
  const hostnameSuffix = required(environment, "REMOTE_HOST_SUFFIX").toLowerCase();
  const remotePortText = required(environment, "REMOTE_PORT");
  const enrollmentEnabled = required(environment, "ENROLLMENT_ENABLED");
  const installationLimitText = required(environment, "BETA_INSTALLATION_LIMIT");
  const releaseVersionsText = required(environment, "BETA_RELEASE_VERSIONS");
  const latestReleaseVersion = required(environment, "BETA_LATEST_RELEASE_VERSION");
  // Optional: comma-separated already-normalized developer-network enrollment
  // exemptions. Empty/unset means "no exemptions" so production behaviour is
  // unchanged by default; the Worker itself re-validates each entry.
  const enrollmentExemptSources = environment.ENROLLMENT_EXEMPT_SOURCES ?? "";
  // Optional: comma-separated testing-network sources (same format as the
  // exemption list). Empty/unset disables marking. The value is never logged.
  const testingSources = environment.TESTING_SOURCES ?? "";
  // Optional: anonymous-enrollment quota window in seconds (defaults to
  // 86400 / 24h inside the Worker when unset or blank). TEMPORARY
  // OPERATOR-TESTING OVERRIDE: set to 3600 (1h) for the active beta test
  // period; must be reverted to 86400+ before stable/friend-beta release.
  const enrollmentWindowSecondsText = environment.ENROLLMENT_WINDOW_SECONDS ?? "";
  const providerToken = required(environment, "CLOUDFLARE_API_TOKEN");
  const adminToken = required(environment, "ADMIN_API_TOKEN");
  const signingKey = required(environment, "INSTALLATION_SIGNING_KEY");
  const pcoClientId = required(environment, "PLANNING_CENTER_CLIENT_ID");
  const pcoClientSecret = required(environment, "PLANNING_CENTER_CLIENT_SECRET");

  if (!ID.test(accountId)) throw new Error("CLOUDFLARE_ACCOUNT_ID must be 32 lowercase hexadecimal characters");
  if (!ID.test(zoneId)) throw new Error("CLOUDFLARE_ZONE_ID must be 32 lowercase hexadecimal characters");
  if (!HOSTNAME.test(hostnameSuffix) || hostnameSuffix.includes("..") || hostnameSuffix.endsWith(".invalid")) {
    throw new Error("REMOTE_HOST_SUFFIX must be a deployable lowercase DNS suffix");
  }
  const remotePort = Number(remotePortText);
  if (!Number.isInteger(remotePort) || remotePort < 1024 || remotePort > 65535 || remotePort === 8765) {
    throw new Error("REMOTE_PORT must be an integer from 1024 through 65535 other than 8765");
  }
  if (!["true", "false"].includes(enrollmentEnabled)) {
    throw new Error("ENROLLMENT_ENABLED must be true or false");
  }
  const installationLimit = Number(installationLimitText);
  if (!Number.isInteger(installationLimit) || installationLimit < 1 || installationLimit > 10000) {
    throw new Error("BETA_INSTALLATION_LIMIT must be an integer from 1 through 10000");
  }
  if (providerToken.length < 20) throw new Error("CLOUDFLARE_API_TOKEN is too short");
  if (adminToken.length < 32) throw new Error("ADMIN_API_TOKEN must contain at least 32 characters");
  if (signingKey.length < 32) throw new Error("INSTALLATION_SIGNING_KEY must contain at least 32 characters");
  if (adminToken === signingKey) throw new Error("ADMIN_API_TOKEN and INSTALLATION_SIGNING_KEY must be independent");
  if (pcoClientId.length < 1) throw new Error("PLANNING_CENTER_CLIENT_ID must not be empty");
  if (pcoClientSecret.length < 1) throw new Error("PLANNING_CENTER_CLIENT_SECRET must not be empty");

  const releaseVersions = releaseVersionsText.split(",").map((value) => value.trim()).filter(Boolean);
  const releasePattern = /^\d+\.\d+\.\d+-beta\.\d+$/;
  if (releaseVersions.length < 1 || releaseVersions.length > 20
    || new Set(releaseVersions).size !== releaseVersions.length
    || releaseVersions.some((value) => !releasePattern.test(value))) {
    throw new Error("BETA_RELEASE_VERSIONS must be a unique comma-separated beta version allowlist");
  }
  if (!releaseVersions.includes(latestReleaseVersion)) {
    throw new Error("BETA_LATEST_RELEASE_VERSION must be in BETA_RELEASE_VERSIONS");
  }
  let enrollmentWindowSeconds;
  if (enrollmentWindowSecondsText.trim() !== "") {
    enrollmentWindowSeconds = Number(enrollmentWindowSecondsText);
    if (!Number.isInteger(enrollmentWindowSeconds) || enrollmentWindowSeconds < 60) {
      throw new Error("ENROLLMENT_WINDOW_SECONDS must be an integer of at least 60 when set");
    }
  }

  return { accountId, zoneId, hostnameSuffix, remotePort, enrollmentEnabled, installationLimit, releaseVersions, latestReleaseVersion, enrollmentExemptSources, testingSources, enrollmentWindowSeconds };
}

export function installedSecretNames(output) {
  const start = output.indexOf("[");
  const end = output.lastIndexOf("]");
  if (start < 0 || end < start) throw new Error("Wrangler secret list did not return a JSON array");
  const rows = JSON.parse(output.slice(start, end + 1));
  if (!Array.isArray(rows)) throw new Error("Wrangler secret list did not return a JSON array");
  return new Set(rows.map((row) => row?.name).filter((name) => typeof name === "string"));
}

export function verifyRuntimeSecrets(output) {
  const installed = installedSecretNames(output);
  const missing = RUNTIME_SECRET_NAMES.filter((name) => !installed.has(name));
  if (missing.length > 0) throw new Error(`Missing Worker runtime secret bindings: ${missing.join(", ")}`);
  return RUNTIME_SECRET_NAMES;
}

function main() {
  const command = process.argv[2];
  if (command === "validate") {
    const config = validateDeploymentEnvironment(process.env);
    const exemptCount = config.enrollmentExemptSources.split(",").map((value) => value.trim()).filter(Boolean).length;
    const testingCount = config.testingSources.split(",").map((value) => value.trim()).filter(Boolean).length;
    const windowText = config.enrollmentWindowSeconds ? `${config.enrollmentWindowSeconds}s` : "default (86400s)";
    console.log(
      `Deployment configuration valid: account/zone IDs present, suffix=${config.hostnameSuffix}, Remote port=${config.remotePort}, enrollment=${config.enrollmentEnabled}, installation limit=${config.installationLimit}, latest beta=${config.latestReleaseVersion}, enrollment exemptions=${exemptCount}, testing sources=${testingCount}, enrollment window=${windowText}; 5 runtime secrets present.`,
    );
    return;
  }
  if (command === "verify-secrets") {
    const names = verifyRuntimeSecrets(required(process.env, "WRANGLER_SECRET_LIST"));
    console.log(`Verified Worker runtime secret bindings: ${names.join(", ")}. Values were not read.`);
    return;
  }
  throw new Error("Usage: node scripts/deployment-config.mjs validate|verify-secrets");
}

if (import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : "Deployment validation failed");
    process.exitCode = 1;
  }
}
