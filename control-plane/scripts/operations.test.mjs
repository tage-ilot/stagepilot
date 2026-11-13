import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { describe, it } from "vitest";

import {
  RUNTIME_SECRET_NAMES,
  validateDeploymentEnvironment,
  verifyRuntimeSecrets,
} from "./deployment-config.mjs";

function deploymentEnvironment(overrides = {}) {
  return {
    CLOUDFLARE_ACCOUNT_ID: "a".repeat(32),
    CLOUDFLARE_ZONE_ID: "b".repeat(32),
    REMOTE_HOST_SUFFIX: "remote.example.com",
    REMOTE_PORT: "18766",
    ENROLLMENT_ENABLED: "true",
    BETA_INSTALLATION_LIMIT: "500",
    BETA_RELEASE_VERSIONS: "1.1.103-beta.1,1.1.103-beta.2",
    BETA_LATEST_RELEASE_VERSION: "1.1.103-beta.1",
    CLOUDFLARE_API_TOKEN: "provider-token-with-narrow-scope",
    ADMIN_API_TOKEN: "admin-token-with-at-least-thirty-two-characters",
    INSTALLATION_SIGNING_KEY: "independent-signing-key-at-least-thirty-two-characters",
    PLANNING_CENTER_CLIENT_ID: "pco-client-id",
    PLANNING_CENTER_CLIENT_SECRET: "pco-client-secret",
    ...overrides,
  };
}


describe("deployment configuration", () => {
  it("validates protected deployment values and all independent runtime secrets", () => {
    assert.deepEqual(validateDeploymentEnvironment(deploymentEnvironment()), {
      accountId: "a".repeat(32),
      zoneId: "b".repeat(32),
      hostnameSuffix: "remote.example.com",
      remotePort: 18766,
      enrollmentEnabled: "true",
      installationLimit: 500,
      releaseVersions: ["1.1.103-beta.1", "1.1.103-beta.2"],
      latestReleaseVersion: "1.1.103-beta.1",
      enrollmentExemptSources: "",
      testingSources: "",
      enrollmentWindowSeconds: undefined,
    });
  });

  it("accepts a comma-separated developer-network enrollment exemption list", () => {
    const config = validateDeploymentEnvironment(
      deploymentEnvironment({ ENROLLMENT_EXEMPT_SOURCES: "192.0.2.10,2001:db8:1:4::/64" }),
    );
    assert.equal(config.enrollmentExemptSources, "192.0.2.10,2001:db8:1:4::/64");
  });

  it("accepts a comma-separated testing-source list and defaults to none", () => {
    const config = validateDeploymentEnvironment(
      deploymentEnvironment({ TESTING_SOURCES: "192.0.2.10,2001:db8:1:4::/64" }),
    );
    assert.equal(config.testingSources, "192.0.2.10,2001:db8:1:4::/64");
  });

  it("accepts an explicit enrollment quota window override", () => {
    const config = validateDeploymentEnvironment(
      deploymentEnvironment({ ENROLLMENT_WINDOW_SECONDS: "3600" }),
    );
    assert.equal(config.enrollmentWindowSeconds, 3600);
  });

  it("rejects an enrollment quota window under 60 seconds", () => {
    assert.throws(
      () => validateDeploymentEnvironment(deploymentEnvironment({ ENROLLMENT_WINDOW_SECONDS: "30" })),
      /ENROLLMENT_WINDOW_SECONDS/,
    );
  });

  it("fails before deployment when any runtime secret is missing", () => {
    for (const name of RUNTIME_SECRET_NAMES) {
      assert.throws(
        () => validateDeploymentEnvironment(deploymentEnvironment({ [name]: "" })),
        new RegExp(`Missing required deployment value: ${name}`),
      );
    }
  });

  it("requires Wrangler read-back to contain every runtime secret name", () => {
    const complete = JSON.stringify(RUNTIME_SECRET_NAMES.map((name) => ({ name, type: "secret_text" })));
    assert.deepEqual(verifyRuntimeSecrets(complete), RUNTIME_SECRET_NAMES);
    assert.throws(() => verifyRuntimeSecrets(JSON.stringify([{ name: RUNTIME_SECRET_NAMES[0] }])), /Missing Worker runtime secret bindings/);
  });

  it("keeps deployment manual and supplies every protected value without tracked placeholders", () => {
    const repository = path.resolve(import.meta.dirname, "../..");
    const workflow = fs.readFileSync(path.join(repository, ".github/workflows/deploy-control-plane.yml"), "utf8");
    const wrangler = fs.readFileSync(path.join(repository, "control-plane/wrangler.toml"), "utf8");
    assert.match(workflow, /workflow_dispatch:/);
    assert.doesNotMatch(workflow, /\bpush:/);
    for (const name of ["CLOUDFLARE_ACCOUNT_ID", "CLOUDFLARE_ZONE_ID", "REMOTE_HOST_SUFFIX", "REMOTE_PORT", "ENROLLMENT_ENABLED", "BETA_INSTALLATION_LIMIT", "BETA_RELEASE_VERSIONS", "BETA_LATEST_RELEASE_VERSION", "ENROLLMENT_EXEMPT_SOURCES", "TESTING_SOURCES"]) {
      assert.match(workflow, new RegExp(`vars\\.${name}`));
      assert.match(workflow, new RegExp(`--var ${name}:`));
    }
    for (const name of RUNTIME_SECRET_NAMES) {
      assert.match(workflow, new RegExp(`secrets\\.${name}`));
      assert.match(workflow, new RegExp(`^ {12}${name}$`, "m"));
    }
    // The release broker/in-app updater are deferred for this beta: no
    // STAGEPILOT_RELEASE_TOKEN is issued, and GITHUB_RELEASE_TOKEN must
    // never be a required deployment value.
    assert.doesNotMatch(workflow, /STAGEPILOT_RELEASE_TOKEN/);
    assert.doesNotMatch(workflow, /GITHUB_RELEASE_TOKEN/);
    assert.match(workflow, /secret list --format json/);
    assert.doesNotMatch(wrangler, /REPLACE_WITH_|example\.invalid/);
  });
});
