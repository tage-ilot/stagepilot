interface Env {
  REGISTRY: DurableObjectNamespace;
  CLOUDFLARE_API_TOKEN: string;
  CLOUDFLARE_ACCOUNT_ID: string;
  CLOUDFLARE_ZONE_ID: string;
  REMOTE_HOST_SUFFIX: string;
  ADMIN_API_TOKEN: string;
  INSTALLATION_SIGNING_KEY: string;
  PLANNING_CENTER_CLIENT_ID: string;
  PLANNING_CENTER_CLIENT_SECRET: string;
  // Optional: the release-broker/in-app updater is deferred for this beta
  // (see docs/native-completion-runbook.md). No STAGEPILOT_RELEASE_TOKEN is
  // issued, so this binding is absent in production; release-asset routes
  // then respond 503 instead of ever making an unauthenticated GitHub call.
  GITHUB_RELEASE_TOKEN?: string;
  REMOTE_PORT?: string;
  ENROLLMENT_ENABLED?: string;
  BETA_INSTALLATION_LIMIT?: string;
  BETA_RELEASE_VERSIONS?: string;
  BETA_LATEST_RELEASE_VERSION?: string;
  ENROLLMENT_EXEMPT_SOURCES?: string;
  // Configurable enrollment-quota window in seconds. Defaults to 24h
  // (86400) when unset. TEMPORARY OPERATOR-TESTING OVERRIDE: this is
  // currently deployed as 3600 (1 hour) for the active beta test period so
  // the operator does not have to wait ~24h between exhausted-quota test
  // cycles. This MUST be reverted to 86400 (or longer) before this beta is
  // promoted to stable or opened to real friend-beta users -- do not ship
  // the 1-hour window as the permanent default.
  ENROLLMENT_WINDOW_SECONDS?: string;
  // Static asset binding for the fleet admin panel (control-plane/public),
  // configured via the [assets] block in wrangler.toml.
  ASSETS?: Fetcher;
  // R2 bucket for installation-uploaded diagnostic bundles (frontend/backend
  // logs + app/version info). Optional/deferred the same way GITHUB_RELEASE_TOKEN
  // is: if the deploying CLOUDFLARE_API_TOKEN lacks R2 permissions this binding
  // can be left unprovisioned and the diagnostics routes respond 503 instead of
  // ever throwing on a missing binding.
  DIAGNOSTICS?: R2Bucket;
}

type Phase = 'disabled' | 'enabling' | 'provisioned' | 'revoking';

interface Installation {
  id: string;
  hostname: string;
  label: string;
  phase: Phase;
  desiredEnabled: boolean;
  generation?: string;
  lastGeneration?: string;
  tunnelId?: string;
  revoked: boolean;
  createdAt: string;
  updatedAt: string;
  statusRate?: RateWindow;
  mutationRate?: RateWindow;
  providerConfirmedAt?: number;
  // Bumped whenever a revoked installation is reactivated so its
  // installation credential rotates to a fresh value even though the
  // durable id/hostname stay the same.
  credentialGeneration?: number;
  // Coarse, non-identifying location derived from Cloudflare's edge
  // geolocation (`request.cf`) on the installation's own authenticated
  // status/provision/reconcile requests -- never from a raw IP address
  // (see docs/remote-control-plane.md). Refreshed on every such request,
  // so a relocated installation's recorded location updates on its next
  // check-in rather than staying stuck on stale data.
  lastCity?: string;
  lastRegion?: string;
  lastCountry?: string;
  // Admin-panel-only "hide from the default list" flag. Deliberately
  // independent of `revoked`/`phase`/credentials: an archived installation
  // keeps checking in, reconciling, and renewing exactly as before --
  // archiving only declutters the operator's view, it is not a lifecycle
  // or access-control state. Use `revoked` (via the existing revoke
  // endpoint) to actually cut off an installation's access.
  archived?: boolean;
  // Self-reported device hostname (Python `platform.node()`), sent on the
  // always-on lightweight check-in and on the authenticated status/
  // provision/reconcile requests. UNTRUSTED and display-only: never use
  // for auth, matching, or any security decision -- an installation can
  // claim any value here. See docs/remote-control-plane.md.
  deviceName?: string;
  // Set true the first time this installation completes an actual
  // provisioning side effect (tunnel/DNS created via ensureProvisioned()).
  // Distinguishes "never enrolled Remote Access" (lightweight-check-in-only
  // installations always have phase 'disabled' with this unset) from "was
  // enabled, now off" (phase 'disabled' but this is true) for the admin
  // panel's phaseLabel().
  everProvisioned?: boolean;
  // Rate-limit window for diagnostic bundle uploads, tracked separately from
  // statusRate/mutationRate so a crash loop hammering /diagnostics cannot
  // also exhaust the installation's normal status/mutation quota.
  diagnosticsRate?: RateWindow;
}

// Metadata for one uploaded diagnostic bundle (frontend log + backend log +
// app/version info, built client-side by the Tauri "send logs" command). The
// bundle bytes themselves live in R2, keyed by installation id + timestamp;
// this is only the small per-upload record the admin list/listing reads.
interface DiagnosticsBundleMeta {
  installationId: string;
  key: string;
  uploadedAt: string;
  size: number;
}

interface RateWindow {
  startedAt: number;
  count: number;
}

// Fleet alert forwarded from an installation's already-deduplicated local
// critical-alert store (see services/critical_alerts.py in the beta app).
// The Worker only stores/surfaces these for admin visibility; it does not
// reimplement the 15-minute local cooldown/dedup decision.
interface AlertRecord {
  id: string;
  installationId: string;
  category: string;
  subject: string;
  message: string;
  severity: 'warning' | 'critical';
  firstSeen: string;
  lastSeen: string;
  count: number;
  status: 'open' | 'acknowledged';
  createdAt: string;
  updatedAt: string;
  // Display-ready location string, e.g. "Austin, TX" or "Unknown", copied
  // from the installation's lastCity/lastRegion/lastCountry (Cloudflare
  // edge geolocation) at the time the alert was ingested/re-forwarded. Not
  // retroactively updated on older records -- matches "current state at
  // time of event" semantics used elsewhere in this system.
  location: string;
}

// Fleet admin action queued for one installation. The Worker cannot reach
// into an installation's own machine, so an admin action is stored here as
// a pending desired action and is picked up by that installation the next
// time it performs its existing authenticated status check (the same
// poll/reconcile shape the control plane already uses for desired state).
// The installation acknowledges it once applied, which removes it.
type PendingActionKind = 'rate_limit_reset';

interface PendingAction {
  id: string;
  installationId: string;
  kind: PendingActionKind;
  requestedAt: string;
}

// Generic operator sign-off request raised BY one installation. Deliberately
// minimal: a reason string plus an approved/denied outcome the requesting
// installation can observe on its own status check. This is not a gate on
// installations existing -- enrollment stays ungated (docs/remote-control-plane.md).
interface ApprovalRequest {
  id: string;
  installationId: string;
  reason: string;
  status: 'pending' | 'approved' | 'denied';
  createdAt: string;
  updatedAt: string;
}

interface RegistryStats {
  activeInstallations: number;
  enrollments: number;
  enrollmentDenied: number;
  statusDenied: number;
  mutationDenied: number;
  providerDenied: number;
  // Running total of bytes currently stored under the diagnostics/ prefix in
  // R2, maintained incrementally (incremented on upload, decremented on the
  // lazy-expiry sweep in uploadDiagnostics()) rather than computed by
  // listing the bucket on every request. This is the enforcement mechanism
  // for DIAGNOSTICS_STORAGE_CAP_BYTES -- R2 itself has no hard storage quota,
  // only billing past the free tier, so this counter is what actually stops
  // writes once the operator's chosen ceiling is reached.
  diagnosticsBytesUsed: number;
}

interface SourceQuota extends RateWindow {
  lastSeenAt: number;
}

interface CloudflareEnvelope<T> {
  success: boolean;
  result: T;
}

interface GitHubReleaseAsset {
  name?: unknown;
  url?: unknown;
  size?: unknown;
  state?: unknown;
  content_type?: unknown;
}

interface GitHubRelease {
  tag_name?: unknown;
  draft?: unknown;
  assets?: GitHubReleaseAsset[];
}

const JSON_HEADERS = {
  'content-type': 'application/json; charset=utf-8',
  'cache-control': 'no-store',
};
const ID = /^[a-f0-9]{32}$/;
const GENERATION = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const IDEMPOTENCY_KEY = /^[A-Za-z0-9._:-]{8,128}$/;
const ENROLLMENTS_PER_SOURCE = 3;
const DEFAULT_ENROLLMENT_WINDOW_SECONDS = 86_400;
const MAX_SOURCE_QUOTAS = 2_000;
const STATUS_REQUESTS_PER_MINUTE = 120;
const MUTATION_REQUESTS_PER_MINUTE = 20;
const RECONCILE_CACHE_SECONDS = 30;
const PROVIDER_WINDOW_SECONDS = 300;
const PROVIDER_TOTAL_BUDGET = 600;
const PROVIDER_NORMAL_BUDGET = 480;
const BETA_RELEASE_REPOSITORY = 'tage-ilot/stagepilot-beta';
const RELEASE_VERSION = /^\d+\.\d+\.\d+-beta\.\d+$/;
const RELEASE_METADATA_PER_MINUTE = 30;
const RELEASE_DOWNLOADS_PER_MINUTE = 6;
const MAX_RELEASE_SOURCES = 2_000;
const MAX_MANIFEST_BYTES = 64 * 1024;
const MAX_UPDATER_BYTES = 512 * 1024 * 1024;
const BETA_RELEASE_ORIGIN = 'https://stagepilot-beta-control-plane.stagepilot-illuminary-beta.workers.dev';
const MAX_RELEASE_REDIRECTS = 5;
const PCO_TOKEN_ENDPOINT = 'https://api.planningcenteronline.com/oauth/token';
const PCO_OAUTH_FLOW_ID_BYTES = 16; // 32 hex chars
const OAUTH_FLOW_ID = /^[a-f0-9]{32}$/;
const OAUTH_FLOWS_PER_MINUTE = 10;
const OAUTH_TOKEN_REQUESTS_PER_MINUTE = 20;
const MAX_OAUTH_RATE_SOURCES = 2_000;
const MAX_DIAGNOSTICS_BUNDLE_BYTES = 2 * 1024 * 1024;
const DIAGNOSTICS_UPLOADS_PER_HOUR = 6;
// Operator-requested hard ceiling, enforced here in code -- NOT by an R2
// bucket lifecycle rule or any Cloudflare-side storage quota. Neither R2
// nor the Cloudflare REST API offers a real "stop accepting writes" quota;
// R2 only bills past its free tier. The operator's instruction was "100%
// sure the free limits are not surpassed", so this is a real enforced cap,
// checked synchronously before every write (see uploadDiagnostics()), not
// a best-effort retention policy. Set well under the 10 GiB/month R2 free
// tier (8 GiB) to leave real headroom for measurement slack and any other
// account R2 usage. If this is ever hit, uploads fail closed (503) rather
// than silently incurring billable overage.
const DIAGNOSTICS_STORAGE_CAP_BYTES = 8 * 1024 * 1024 * 1024;
// Objects older than this are deleted lazily (swept opportunistically on
// each upload, not via a bucket lifecycle rule or a cron trigger) so the
// running `diagnosticsBytesUsed` counter above stays accurate and storage
// doesn't grow unbounded between uploads. Also keeps data retention tight,
// but the storage cap above is the real safety backstop regardless of
// whether this sweep runs often enough to do its own job.
const DIAGNOSTICS_RETENTION_DAYS = 30;

function exactLengthStream(
  body: ReadableStream<Uint8Array>,
  expected: number,
  maximum: number,
): ReadableStream<Uint8Array> {
  let received = 0;
  return body.pipeThrough(new TransformStream<Uint8Array, Uint8Array>({
    transform(chunk, controller) {
      received += chunk.byteLength;
      if (received > expected || received > maximum) {
        throw new Error('release asset exceeded declared size');
      }
      controller.enqueue(chunk);
    },
    flush() {
      if (received !== expected) throw new Error('release asset size mismatch');
    },
  }));
}

function parseUrl(value: unknown): URL | undefined {
  if (typeof value !== 'string') return undefined;
  try {
    return new URL(value);
  } catch {
    return undefined;
  }
}

class Limited extends Error {
  constructor(
    readonly status: 429 | 503,
    readonly retryAfter: number,
    message: string,
  ) {
    super(message);
  }
}

function reply(body: unknown, status = 200, retryAfter?: number): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { ...JSON_HEADERS, ...(retryAfter ? { 'retry-after': String(retryAfter) } : {}) },
  });
}

function randomHex(bytes: number): string {
  const value = crypto.getRandomValues(new Uint8Array(bytes));
  return Array.from(value, (part) => part.toString(16).padStart(2, '0')).join('');
}

const NEW_ID_BYTES = 4; // 8 hex chars (32 bits) for new-enrollment / reenroll ids.
const MAX_ID_COLLISION_ATTEMPTS = 5;

// id lineage: installations minted furthest in the past used a 32-hex-char
// id (16 random bytes); a later generation shortened that to 16 hex chars
// (8 random bytes); this generation shortens it again to 8 hex chars (4
// random bytes, ~4.3 billion possibilities) for a shorter Remote URL.
// Every existing installation keeps whatever id/hostname it was minted
// with -- only brand-new enrollments and reenroll() calls made after this
// change get the shorter 8-char id. Route matching therefore has to accept
// all three lengths indefinitely (see the {8}|{16}|{32} regexes above).
async function generateUniqueInstallationId(
  storage: DurableObjectStorage,
): Promise<string> {
  for (let attempt = 0; attempt < MAX_ID_COLLISION_ATTEMPTS; attempt++) {
    const candidate = randomHex(NEW_ID_BYTES);
    const existing = await storage.get(`installation:${candidate}`);
    if (existing === undefined) return candidate;
  }
  throw new Limited(503, 5, 'installation id allocation failed');
}

function encodeBase64Url(value: Uint8Array): string {
  let binary = '';
  for (const byte of value) binary += String.fromCharCode(byte);
  return btoa(binary).replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/, '');
}

async function digest(value: string): Promise<Uint8Array> {
  return new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(value)));
}

async function equalSecret(actual: string, expected: string): Promise<boolean> {
  const [left, right] = await Promise.all([digest(actual), digest(expected)]);
  let difference = left.length ^ right.length;
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    difference |= (left[index] ?? 0) ^ (right[index] ?? 0);
  }
  return difference === 0;
}

async function keyedHash(keyMaterial: string, value: string): Promise<string> {
  const key = await crypto.subtle.importKey(
    'raw', new TextEncoder().encode(keyMaterial), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign'],
  );
  return encodeBase64Url(new Uint8Array(
    await crypto.subtle.sign('HMAC', key, new TextEncoder().encode(value)),
  ));
}

function normalizeSourceAddress(value: string): string | undefined {
  const ipv4 = value.split('.');
  if (ipv4.length === 4 && ipv4.every((part) => /^\d{1,3}$/.test(part) && Number(part) <= 255)) {
    return ipv4.map(Number).join('.');
  }
  const lowered = value.toLowerCase().split('%', 1)[0];
  if (!lowered.includes(':') || !/^[0-9a-f:]+$/.test(lowered) || (lowered.match(/::/g)?.length ?? 0) > 1) {
    return undefined;
  }
  const sides = lowered.split('::');
  const left = sides[0] ? sides[0].split(':') : [];
  const right = sides[1] ? sides[1].split(':') : [];
  if ([...left, ...right].some((part) => !/^[0-9a-f]{1,4}$/.test(part))) return undefined;
  const missing = 8 - left.length - right.length;
  if ((sides.length === 1 && missing !== 0) || (sides.length === 2 && missing < 1)) return undefined;
  const words = [...left, ...Array(Math.max(0, missing)).fill('0'), ...right]
    .map((part) => Number.parseInt(part, 16));
  if (words.length !== 8) return undefined;
  return `${words.slice(0, 4).map((part) => part.toString(16)).join(':')}::/64`;
}

function exemptSources(value: string | undefined): Set<string> {
  const set = new Set<string>();
  for (const raw of (value ?? '').split(',')) {
    const trimmed = raw.trim();
    if (!trimmed) continue;
    const normalized = trimmed.endsWith('::/64')
      ? normalizeSourceAddress(`${trimmed.slice(0, -'::/64'.length)}::`)
      : normalizeSourceAddress(trimmed);
    // Only accept config entries that are already in exact canonical form;
    // anything that round-trips differently (a malformed or spoofed variant)
    // is silently dropped rather than treated as an exemption.
    if (normalized === trimmed) set.add(normalized);
  }
  return set;
}

function bearer(request: Request): string {
  const value = request.headers.get('authorization') ?? '';
  return value.startsWith('Bearer ') ? value.slice(7) : '';
}

async function body(request: Request): Promise<Record<string, unknown>> {
  const length = Number(request.headers.get('content-length') ?? '0');
  if (length > 4096) throw new Error('invalid request');
  const text = await request.text();
  if (text.length > 4096) throw new Error('invalid request');
  const value: unknown = JSON.parse(text);
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('invalid request');
  return value as Record<string, unknown>;
}

// Self-reported, UNTRUSTED, display-only device hostname validation. Never
// used for auth/matching/any security decision -- see the `deviceName`
// field comment on Installation. Rejects non-string/empty-after-trim,
// strips control characters, and caps length at 253 (matches the Python
// side's sanitize_device_name truncation).
const CONTROL_CHARS = /[\x00-\x1f\x7f]/g;

function sanitizeDeviceName(value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined;
  const cleaned = value.replace(CONTROL_CHARS, '').trim();
  if (!cleaned) return undefined;
  return cleaned.slice(0, 253);
}

// Dedicated, much larger size cap for diagnostic bundle uploads only -- the
// 4096-byte cap enforced by body() above is unchanged for every other route.
// Returns the raw bytes (the bundle is an opaque JSON/text blob built by the
// Tauri "send logs" command; the Worker never needs to parse its contents,
// only size-check and store it), so this intentionally does not JSON.parse.
async function sizedBody(request: Request, maxBytes: number): Promise<Uint8Array> {
  const length = Number(request.headers.get('content-length') ?? '0');
  if (!Number.isFinite(length) || length <= 0 || length > maxBytes) throw new Error('invalid request');
  const bytes = new Uint8Array(await request.arrayBuffer());
  if (bytes.byteLength > maxBytes) throw new Error('invalid request');
  return bytes;
}

function publicInstallation(installation: Installation): Record<string, unknown> {
  return {
    installationId: installation.id,
    hostname: installation.hostname,
    phase: installation.phase,
    generation: installation.generation ?? null,
    revoked: installation.revoked,
    createdAt: installation.createdAt,
    updatedAt: installation.updatedAt,
    deviceName: installation.deviceName ?? null,
  };
}

function formatInstallationLocation(installation: Installation): string {
  return installation.lastCity
    ?? installation.lastRegion
    ?? installation.lastCountry
    ?? 'Unknown';
}

function publicAlert(alert: AlertRecord): Record<string, unknown> {
  return {
    alertId: alert.id,
    installationId: alert.installationId,
    category: alert.category,
    subject: alert.subject,
    message: alert.message,
    severity: alert.severity,
    firstSeen: alert.firstSeen,
    lastSeen: alert.lastSeen,
    count: alert.count,
    status: alert.status,
    createdAt: alert.createdAt,
    updatedAt: alert.updatedAt,
    location: alert.location,
  };
}

function publicPendingAction(action: PendingAction): Record<string, unknown> {
  return {
    actionId: action.id,
    installationId: action.installationId,
    kind: action.kind,
    requestedAt: action.requestedAt,
  };
}

function publicApprovalRequest(approval: ApprovalRequest): Record<string, unknown> {
  return {
    requestId: approval.id,
    installationId: approval.installationId,
    reason: approval.reason,
    status: approval.status,
    createdAt: approval.createdAt,
    updatedAt: approval.updatedAt,
  };
}

export class Registry {
  private serial: Promise<void> = Promise.resolve();
  private readonly tokenCache = new Map<string, { token: string; expiresAt: number }>();
  private readonly releaseCache = new Map<string, { release: GitHubRelease; expiresAt: number }>();
  private providerLane: 'normal' | 'recovery' = 'normal';

  constructor(
    private readonly state: DurableObjectState,
    private readonly env: Env,
  ) {}

  async fetch(request: Request): Promise<Response> {
    let release = (): void => {};
    const previous = this.serial;
    this.serial = new Promise((resolve) => { release = resolve; });
    await previous;
    try {
      return await this.handle(request);
    } finally {
      release();
    }
  }

  private async handle(request: Request): Promise<Response> {
    try {
      this.validateConfiguration();
      const url = new URL(request.url);
      if (request.method === 'POST' && url.pathname === '/v1/installations/enroll') {
        return await this.enroll(request);
      }
      if (request.method === 'POST' && url.pathname === '/v1/planning-center/oauth/flow') {
        return await this.pcoOauthFlow(request);
      }
      if (request.method === 'POST' && url.pathname === '/v1/planning-center/oauth/token') {
        return await this.pcoOauthExchange(request, 'authorization_code');
      }
      if (request.method === 'POST' && url.pathname === '/v1/planning-center/oauth/refresh') {
        return await this.pcoOauthExchange(request, 'refresh_token');
      }
      if (request.method === 'GET' && url.pathname === '/v1/releases/latest.json') {
        return await this.releaseAsset(request, this.latestReleaseVersion(), 'latest.json');
      }
      const releaseAsset = url.pathname.match(/^\/v1\/releases\/v([^/]+)\/([^/]+)$/);
      if (request.method === 'GET' && releaseAsset) {
        return await this.releaseAsset(request, releaseAsset[1], releaseAsset[2]);
      }
      if (request.method === 'GET' && url.pathname === '/v1/admin/metrics') {
        if (!(await this.isAdmin(request))) return reply({ error: 'unauthorized' }, 401);
        return reply(await this.stats());
      }
      if (request.method === 'GET' && url.pathname === '/v1/admin/cities') {
        if (!(await this.isAdmin(request))) return reply({ error: 'unauthorized' }, 401);
        return await this.adminListCities();
      }
      if (request.method === 'GET' && url.pathname === '/v1/admin/installations') {
        if (!(await this.isAdmin(request))) return reply({ error: 'unauthorized' }, 401);
        return await this.adminListInstallations();
      }
      const adminSetLabel = url.pathname.match(/^\/v1\/admin\/installations\/([a-f0-9]{8}|[a-f0-9]{16}|[a-f0-9]{32})\/label$/);
      if (request.method === 'PATCH' && adminSetLabel) {
        if (!(await this.isAdmin(request))) return reply({ error: 'unauthorized' }, 401);
        return await this.adminSetLabel(request, adminSetLabel[1]);
      }
      const adminSetArchived = url.pathname.match(/^\/v1\/admin\/installations\/([a-f0-9]{8}|[a-f0-9]{16}|[a-f0-9]{32})\/archive$/);
      if (request.method === 'PATCH' && adminSetArchived) {
        if (!(await this.isAdmin(request))) return reply({ error: 'unauthorized' }, 401);
        return await this.adminSetArchived(request, adminSetArchived[1]);
      }
      const adminRevoke = url.pathname.match(/^\/v1\/admin\/installations\/([a-f0-9]{8}|[a-f0-9]{16}|[a-f0-9]{32})\/revoke$/);
      if (request.method === 'POST' && adminRevoke) {
        if (!(await this.isAdmin(request))) return reply({ error: 'unauthorized' }, 401);
        return await this.withProviderLane('recovery', () => this.adminRevoke(adminRevoke[1]));
      }
      if (request.method === 'GET' && url.pathname === '/v1/admin/alerts') {
        if (!(await this.isAdmin(request))) return reply({ error: 'unauthorized' }, 401);
        return await this.adminListAlerts(url);
      }
      const adminAcknowledge = url.pathname.match(/^\/v1\/admin\/alerts\/([^/]{1,200})\/acknowledge$/);
      if (request.method === 'POST' && adminAcknowledge) {
        if (!(await this.isAdmin(request))) return reply({ error: 'unauthorized' }, 401);
        return await this.adminAcknowledgeAlert(adminAcknowledge[1]);
      }
      const adminDiagnosticsList = url.pathname.match(/^\/v1\/admin\/installations\/([a-f0-9]{8}|[a-f0-9]{16}|[a-f0-9]{32})\/diagnostics$/);
      if (request.method === 'GET' && adminDiagnosticsList) {
        if (!(await this.isAdmin(request))) return reply({ error: 'unauthorized' }, 401);
        return await this.adminListDiagnostics(adminDiagnosticsList[1]);
      }
      const adminDiagnosticsGet = url.pathname.match(
        /^\/v1\/admin\/installations\/([a-f0-9]{8}|[a-f0-9]{16}|[a-f0-9]{32})\/diagnostics\/([0-9]{8}T[0-9]{6}Z-[a-f0-9]{8})$/,
      );
      if (request.method === 'GET' && adminDiagnosticsGet) {
        if (!(await this.isAdmin(request))) return reply({ error: 'unauthorized' }, 401);
        return await this.adminGetDiagnostic(adminDiagnosticsGet[1], adminDiagnosticsGet[2]);
      }
      const adminRateLimitReset = url.pathname.match(/^\/v1\/installations\/([a-f0-9]{8}|[a-f0-9]{16}|[a-f0-9]{32})\/rate-limit\/reset$/);
      if (request.method === 'POST' && adminRateLimitReset) {
        if (!(await this.isAdmin(request))) return reply({ error: 'unauthorized' }, 401);
        return await this.adminQueueRateLimitReset(adminRateLimitReset[1]);
      }
      if (request.method === 'GET' && url.pathname === '/v1/admin/approval-requests') {
        if (!(await this.isAdmin(request))) return reply({ error: 'unauthorized' }, 401);
        return await this.adminListApprovalRequests(url);
      }
      const adminDecide = url.pathname.match(/^\/v1\/admin\/approval-requests\/([^/]{1,200})\/decide$/);
      if (request.method === 'POST' && adminDecide) {
        if (!(await this.isAdmin(request))) return reply({ error: 'unauthorized' }, 401);
        return await this.adminDecideApprovalRequest(request, adminDecide[1]);
      }
      const installationApproval = url.pathname.match(/^\/v1\/installations\/([a-f0-9]{8}|[a-f0-9]{16}|[a-f0-9]{32})\/approval-requests$/);
      if (installationApproval && (request.method === 'POST' || request.method === 'GET')) {
        const installation = await this.state.storage.get<Installation>(`installation:${installationApproval[1]}`);
        if (!installation || installation.revoked || !(await this.isInstallation(request, installation))) {
          return reply({ error: 'unauthorized' }, 401);
        }
        if (request.method === 'GET') {
          await this.takeInstallationRate(installation, 'status');
          return await this.installationApprovalRequests(installation);
        }
        await this.takeInstallationRate(installation, 'mutation');
        return await this.createApprovalRequest(request, installation);
      }
      const actionAck = url.pathname.match(
        /^\/v1\/installations\/([a-f0-9]{8}|[a-f0-9]{16}|[a-f0-9]{32})\/pending-actions\/([^/]{1,200})\/ack$/,
      );
      if (request.method === 'POST' && actionAck) {
        const installation = await this.state.storage.get<Installation>(`installation:${actionAck[1]}`);
        if (!installation || installation.revoked || !(await this.isInstallation(request, installation))) {
          return reply({ error: 'unauthorized' }, 401);
        }
        await this.takeInstallationRate(installation, 'mutation');
        return await this.acknowledgePendingAction(installation, actionAck[2]);
      }
      const installationAlert = url.pathname.match(/^\/v1\/installations\/([a-f0-9]{8}|[a-f0-9]{16}|[a-f0-9]{32})\/alerts$/);
      if (request.method === 'POST' && installationAlert) {
        const installation = await this.state.storage.get<Installation>(`installation:${installationAlert[1]}`);
        if (!installation || installation.revoked || !(await this.isInstallation(request, installation))) {
          return reply({ error: 'unauthorized' }, 401);
        }
        await this.takeInstallationRate(installation, 'mutation');
        return await this.ingestAlert(request, installation);
      }
      // Diagnostic bundle upload (opaque JSON blob: frontend log + backend
      // log + app/version info, built client-side by the Tauri "send logs"
      // command). Authenticated exactly like /alerts above, but deliberately
      // rate-limited on its OWN dedicated window (not takeInstallationRate's
      // mutation bucket) so a crash loop retrying this one route repeatedly
      // cannot also exhaust the installation's normal status/mutation quota.
      const installationDiagnostics = url.pathname.match(/^\/v1\/installations\/([a-f0-9]{8}|[a-f0-9]{16}|[a-f0-9]{32})\/diagnostics$/);
      if (request.method === 'POST' && installationDiagnostics) {
        const installation = await this.state.storage.get<Installation>(`installation:${installationDiagnostics[1]}`);
        if (!installation || installation.revoked || !(await this.isInstallation(request, installation))) {
          return reply({ error: 'unauthorized' }, 401);
        }
        await this.takeDiagnosticsRate(installation);
        return await this.uploadDiagnostics(request, installation);
      }
      const reenrollRoute = url.pathname.match(/^\/v1\/installations\/([a-f0-9]{8}|[a-f0-9]{16}|[a-f0-9]{32})\/reenroll$/);
      if (request.method === 'POST' && reenrollRoute) {
        const installation = await this.state.storage.get<Installation>(`installation:${reenrollRoute[1]}`);
        if (!installation || installation.revoked || !(await this.isInstallation(request, installation))) {
          return reply({ error: 'unauthorized' }, 401);
        }
        await this.takeInstallationRate(installation, 'mutation');
        return await this.withProviderLane('recovery', () => this.reenroll(installation));
      }
      // Authenticated Enable on a known-but-disabled-or-revoked installation:
      // unlike the other routes below, this deliberately allows a revoked
      // installation through (that is the whole point) as long as the
      // caller can still present its installation credential.
      const reactivateRoute = url.pathname.match(/^\/v1\/installations\/([a-f0-9]{8}|[a-f0-9]{16}|[a-f0-9]{32})\/reactivate$/);
      if (request.method === 'POST' && reactivateRoute) {
        const installation = await this.state.storage.get<Installation>(`installation:${reactivateRoute[1]}`);
        if (!installation || !(await this.isInstallation(request, installation))) {
          return reply({ error: 'unauthorized' }, 401);
        }
        await this.takeInstallationRate(installation, 'mutation');
        return await this.withProviderLane('recovery', () => this.reactivate(installation));
      }
      const route = url.pathname.match(/^\/v1\/installations\/([a-f0-9]{8}|[a-f0-9]{16}|[a-f0-9]{32})\/(status|provision|disable|revoke|reconcile)$/);
      if (!route) return reply({ error: 'not found' }, 404);
      const installation = await this.state.storage.get<Installation>(`installation:${route[1]}`);
      if (!installation || installation.revoked || !(await this.isInstallation(request, installation))) {
        return reply({ error: 'unauthorized' }, 401);
      }
      const action = route[2];
      if (action === 'status' || action === 'provision' || action === 'reconcile') {
        await this.recordRequestGeo(request, installation);
      }
      if (action === 'status' && request.method === 'GET') {
        await this.takeInstallationRate(installation, 'status');
        // deviceName on the GET status route travels as a query parameter,
        // not a body: GET requests are not permitted to carry a body in
        // the standard Fetch API, and this status route must remain a
        // normal body-less GET for the many plain status polls that never
        // send deviceName at all.
        if (this.applyDeviceName(installation, url.searchParams.get('deviceName'))) {
          installation.updatedAt = new Date().toISOString();
          await this.save(installation);
        }
        return reply({
          ...publicInstallation(installation),
          pendingActions: await this.pendingActions(installation.id),
        });
      }
      if (request.method !== 'POST') return reply({ error: 'method not allowed' }, 405);
      await this.takeInstallationRate(installation, 'mutation');
      if (action === 'provision') return await this.provision(request, installation);
      if (action === 'disable') return await this.withProviderLane('recovery', () => this.disable(installation, false));
      if (action === 'revoke') return await this.withProviderLane('recovery', () => this.disable(installation, true));
      if (action === 'reconcile') {
        const lane = installation.desiredEnabled ? 'normal' : 'recovery';
        return await this.withProviderLane(lane, () => this.reconcile(request, installation));
      }
      return reply({ error: 'method not allowed' }, 405);
    } catch (error) {
      if (error instanceof Limited) {
        return reply({ error: error.message }, error.status, error.retryAfter);
      }
      // Internal messages are deliberately generic and never include provider
      // response bodies, request headers, or credential values.
      const message = error instanceof Error ? error.message : 'unknown';
      console.error('control-plane request failed', message);
      const response: Record<string, string> = { error: 'operation incomplete; retry reconciliation' };
      const safeDiagnostics = new Set([
        'missing generation',
        'hostname ownership conflict',
        'tunnel creation not confirmed',
        'hostname route not confirmed',
        'installation credential unavailable',
        'hostname removal not confirmed',
        'tunnel revocation not confirmed',
        'ambiguous tunnel ownership',
        'invalid tunnel ownership',
        'ambiguous hostname ownership',
        'tunnel configuration not confirmed',
      ]);
      if (message.startsWith('provider ') || safeDiagnostics.has(message)) {
        response.diagnostic = message;
      }
      return reply(response, 503);
    }
  }

  private validateConfiguration(): void {
    if (!ID.test(this.env.CLOUDFLARE_ACCOUNT_ID) || !ID.test(this.env.CLOUDFLARE_ZONE_ID)) {
      throw new Error('invalid provider configuration');
    }
    if (typeof this.env.CLOUDFLARE_API_TOKEN !== 'string' || this.env.CLOUDFLARE_API_TOKEN.length < 20
      || typeof this.env.ADMIN_API_TOKEN !== 'string' || this.env.ADMIN_API_TOKEN.length < 32
      || typeof this.env.INSTALLATION_SIGNING_KEY !== 'string' || this.env.INSTALLATION_SIGNING_KEY.length < 32
      || typeof this.env.PLANNING_CENTER_CLIENT_ID !== 'string' || this.env.PLANNING_CENTER_CLIENT_ID.length < 1
      || typeof this.env.PLANNING_CENTER_CLIENT_SECRET !== 'string' || this.env.PLANNING_CENTER_CLIENT_SECRET.length < 1
      || (this.env.GITHUB_RELEASE_TOKEN !== undefined && this.env.GITHUB_RELEASE_TOKEN.length < 20)
      || this.env.ADMIN_API_TOKEN === this.env.INSTALLATION_SIGNING_KEY) {
      throw new Error('invalid authentication configuration');
    }
    if (!['true', 'false'].includes(this.env.ENROLLMENT_ENABLED ?? 'true')
      || !Number.isInteger(this.installationLimit()) || this.installationLimit() < 1) {
      throw new Error('invalid enrollment configuration');
    }
    const suffix = this.env.REMOTE_HOST_SUFFIX.toLowerCase();
    if (!/^[a-z0-9](?:[a-z0-9.-]{1,251}[a-z0-9])$/.test(suffix) || suffix.includes('..')) {
      throw new Error('invalid hostname suffix');
    }
    this.remotePort();
    this.releaseVersions();
  }

  private releaseVersions(): string[] {
    const values = (this.env.BETA_RELEASE_VERSIONS ?? '').split(',').map((value) => value.trim()).filter(Boolean);
    if (values.length < 1 || values.length > 20 || new Set(values).size !== values.length
      || values.some((value) => !RELEASE_VERSION.test(value))) {
      throw new Error('invalid beta release allowlist');
    }
    if (!values.includes(this.latestReleaseVersion())) throw new Error('invalid beta latest release');
    return values;
  }

  private latestReleaseVersion(): string {
    const value = this.env.BETA_LATEST_RELEASE_VERSION ?? '';
    if (!RELEASE_VERSION.test(value)) throw new Error('invalid beta latest release');
    return value;
  }

  private allowedReleaseAsset(version: string, filename: string): boolean {
    if (!this.releaseVersions().includes(version)) return false;
    return new Set([
      `StagePilot_${version}_aarch64.dmg`,
      `StagePilot_${version}_x64.dmg`,
      `StagePilot_${version}_aarch64.app.tar.gz`,
      `StagePilot_${version}_x64.app.tar.gz`,
      `StagePilot_${version}_x64-setup.exe`,
    ]).has(filename) || (version === this.latestReleaseVersion() && filename === 'latest.json');
  }

  private async takeReleaseRate(request: Request, kind: 'metadata' | 'download'): Promise<void> {
    const source = normalizeSourceAddress(request.headers.get('cf-connecting-ip') ?? '');
    if (!source) throw new Limited(429, 60, 'release request rate limited');
    const now = Math.floor(Date.now() / 1000);
    const hash = await keyedHash(this.env.INSTALLATION_SIGNING_KEY, `release-source:${source}`);
    const subject = `${kind}:${hash}`;
    const indexKey = 'release-source-index';
    const index = await this.state.storage.get<string[]>(indexKey) ?? [];
    const retained: string[] = [];
    for (const candidate of index) {
      const stored = await this.state.storage.get<RateWindow>(`release-source:${candidate}`);
      if (stored && now - stored.startedAt < 60) retained.push(candidate);
      else await this.state.storage.delete(`release-source:${candidate}`);
    }
    if (!retained.includes(subject) && retained.length >= MAX_RELEASE_SOURCES) {
      throw new Limited(503, 60, 'release service unavailable');
    }
    const key = `release-source:${subject}`;
    const current = await this.state.storage.get<RateWindow>(key);
    const window = !current || now - current.startedAt >= 60 ? { startedAt: now, count: 0 } : current;
    const limit = kind === 'metadata' ? RELEASE_METADATA_PER_MINUTE : RELEASE_DOWNLOADS_PER_MINUTE;
    if (window.count >= limit) throw new Limited(429, Math.max(1, 60 - (now - window.startedAt)), 'release request rate limited');
    await this.state.storage.put({
      [key]: { ...window, count: window.count + 1 },
      [indexKey]: retained.includes(subject) ? retained : [...retained, subject],
    });
  }

  private async releaseAsset(request: Request, version: string, filename: string): Promise<Response> {
    if (!this.allowedReleaseAsset(version, filename)) return reply({ error: 'not found' }, 404);
    if (typeof this.env.GITHUB_RELEASE_TOKEN !== 'string' || this.env.GITHUB_RELEASE_TOKEN.length < 20) {
      // The release broker is deferred for this beta: no GITHUB_RELEASE_TOKEN
      // is configured, so respond 503 without ever calling GitHub.
      return reply({ error: 'release unavailable' }, 503, 60);
    }
    await this.takeReleaseRate(request, filename === 'latest.json' ? 'metadata' : 'download');
    const release = await this.githubRelease(version);
    if (!release) return reply({ error: 'release unavailable' }, 503, 60);
    if (release.tag_name !== `v${version}` || release.draft !== false || !Array.isArray(release.assets)) {
      return reply({ error: 'release unavailable' }, 503, 60);
    }
    const matches = release.assets.filter((asset) => asset.name === filename && asset.state === 'uploaded');
    const asset = matches.length === 1 ? matches[0] : undefined;
    const maximum = filename === 'latest.json' ? MAX_MANIFEST_BYTES : MAX_UPDATER_BYTES;
    const assetUrl = parseUrl(asset?.url);
    const expectedAssetPath = new RegExp(
      `^/repos/${BETA_RELEASE_REPOSITORY.replace('/', '\\/')}/releases/assets/[1-9][0-9]*$`,
    );
    if (!asset || !assetUrl || typeof asset.size !== 'number'
      || asset.size < 1 || asset.size > maximum || assetUrl.protocol !== 'https:'
      || assetUrl.hostname !== 'api.github.com' || assetUrl.search !== ''
      || !expectedAssetPath.test(assetUrl.pathname)) {
      return reply({ error: 'release unavailable' }, 503, 60);
    }
    const response = await this.downloadGitHubAsset(assetUrl.toString());
    if (!response.ok || !response.body) return reply({ error: 'release unavailable' }, 503, 60);
    const length = Number(response.headers.get('content-length') ?? asset.size);
    if (!Number.isFinite(length) || length !== asset.size || length > maximum) {
      return reply({ error: 'release unavailable' }, 503, 60);
    }
    const boundedBody = exactLengthStream(response.body, asset.size, maximum);
    if (filename === 'latest.json') {
      let manifest: string;
      try {
        manifest = await new Response(boundedBody).text();
      } catch {
        return reply({ error: 'release unavailable' }, 503, 60);
      }
      if (!this.validManifest(manifest, version)) {
        return reply({ error: 'release unavailable' }, 503, 60);
      }
      return new Response(manifest, {
        headers: {
          'content-type': 'application/json; charset=utf-8',
          'content-length': String(asset.size),
          'cache-control': 'public, max-age=60, s-maxage=300',
          'x-content-type-options': 'nosniff',
        },
      });
    }
    return new Response(boundedBody, {
      status: 200,
      headers: {
        'content-type': typeof asset.content_type === 'string' ? asset.content_type : 'application/octet-stream',
        'content-length': String(length),
        'cache-control': 'public, max-age=31536000, immutable',
        'x-content-type-options': 'nosniff',
      },
    });
  }

  private validManifest(content: string, version: string): boolean {
    try {
      const manifest = JSON.parse(content) as {
        version?: unknown;
        pub_date?: unknown;
        platforms?: Record<string, { url?: unknown; signature?: unknown }>;
      };
      if (manifest.version !== version || typeof manifest.pub_date !== 'string'
        || !Number.isFinite(Date.parse(manifest.pub_date)) || !manifest.platforms) return false;
      const filenames: Record<string, string> = {
        'darwin-aarch64': `StagePilot_${version}_aarch64.app.tar.gz`,
        'darwin-x86_64': `StagePilot_${version}_x64.app.tar.gz`,
        'windows-x86_64': `StagePilot_${version}_x64-setup.exe`,
      };
      if (Object.keys(manifest.platforms).sort().join(',') !== Object.keys(filenames).sort().join(',')) return false;
      return Object.entries(filenames).every(([platform, filename]) => {
        const entry = manifest.platforms?.[platform];
        return entry?.url === `${BETA_RELEASE_ORIGIN}/v1/releases/v${version}/${filename}`
          && typeof entry.signature === 'string' && entry.signature.length > 0 && entry.signature.length <= 4096;
      });
    } catch {
      return false;
    }
  }

  private async githubRelease(version: string): Promise<GitHubRelease | undefined> {
    const now = Math.floor(Date.now() / 1000);
    const cached = this.releaseCache.get(version);
    if (cached && cached.expiresAt >= now) return cached.release;
    const response = await fetch(
      `https://api.github.com/repos/${BETA_RELEASE_REPOSITORY}/releases/tags/v${version}`,
      { headers: this.githubHeaders('application/vnd.github+json') },
    );
    if (!response.ok) return undefined;
    const release = await response.json() as GitHubRelease;
    this.releaseCache.set(version, { release, expiresAt: now + 300 });
    return release;
  }

  private async downloadGitHubAsset(assetUrl: string): Promise<Response> {
    let response = await fetch(assetUrl, {
      headers: this.githubHeaders('application/octet-stream'),
      redirect: 'manual',
    });
    let current = new URL(assetUrl);
    for (let redirects = 0; [301, 302, 303, 307, 308].includes(response.status); redirects += 1) {
      if (redirects >= MAX_RELEASE_REDIRECTS) return new Response(null, { status: 502 });
      const location = response.headers.get('location');
      if (!location) return new Response(null, { status: 502 });
      const download = new URL(location, current);
      if (download.protocol !== 'https:' || !download.hostname.endsWith('.githubusercontent.com')) {
        return new Response(null, { status: 502 });
      }
      current = download;
      response = await fetch(download, { redirect: 'manual' });
    }
    return response;
  }

  private githubHeaders(accept: string): HeadersInit {
    return {
      accept,
      authorization: `Bearer ${this.env.GITHUB_RELEASE_TOKEN}`,
      'user-agent': 'stagepilot-beta-release-broker/1',
      'x-github-api-version': '2022-11-28',
    };
  }

  private async isAdmin(request: Request): Promise<boolean> {
    const token = bearer(request);
    return token.length >= 32 && equalSecret(token, this.env.ADMIN_API_TOKEN);
  }


  private async credential(id: string, generation = 0): Promise<string> {
    const key = await crypto.subtle.importKey(
      'raw',
      new TextEncoder().encode(this.env.INSTALLATION_SIGNING_KEY),
      { name: 'HMAC', hash: 'SHA-256' },
      false,
      ['sign'],
    );
    const signature = new Uint8Array(
      await crypto.subtle.sign(
        'HMAC',
        key,
        new TextEncoder().encode(`stagepilot-installation:${id}:${generation}`),
      ),
    );
    return `spi_${id}.${encodeBase64Url(signature)}`;
  }

  private async isInstallation(request: Request, installation: Installation): Promise<boolean> {
    const token = bearer(request);
    if (!token.startsWith(`spi_${installation.id}.`)) return false;
    return equalSecret(token, await this.credential(installation.id, installation.credentialGeneration ?? 0));
  }

  // Authenticated reactivation for plain Enable on a previously-known but
  // disabled/revoked installation: the caller already proved ownership of
  // `installation` via its still-valid installation credential (isInstallation),
  // so this never touches the anonymous, per-network ENROLLMENTS_PER_SOURCE
  // quota that /v1/installations/enroll enforces for first-time/unknown
  // clients. Reprovisions the SAME hostname/installation id with a fresh
  // credential generation -- identical outcome to the nonce-replay branch of
  // enroll(), just reached through a proof-of-ownership bearer token instead
  // of a replayed anonymous enrollment nonce.
  private async reactivate(installation: Installation): Promise<Response> {
    if (!installation.revoked) {
      return reply({
        ...publicInstallation(installation),
        installationCredential: await this.credential(installation.id, installation.credentialGeneration ?? 0),
      });
    }
    const stats = await this.stats();
    if ((this.env.ENROLLMENT_ENABLED ?? 'true') !== 'true') {
      await this.bumpDenied(stats, 'enrollmentDenied');
      throw new Limited(503, 300, 'enrollment unavailable');
    }
    if (stats.activeInstallations >= this.installationLimit()) {
      await this.bumpDenied(stats, 'enrollmentDenied');
      throw new Limited(503, 300, 'enrollment unavailable');
    }
    installation.revoked = false;
    installation.phase = 'disabled';
    installation.desiredEnabled = false;
    delete installation.generation;
    delete installation.tunnelId;
    delete installation.providerConfirmedAt;
    installation.credentialGeneration = (installation.credentialGeneration ?? 0) + 1;
    installation.updatedAt = new Date().toISOString();
    stats.activeInstallations += 1;
    await this.state.storage.put({
      [`installation:${installation.id}`]: installation,
      'registry:stats': stats,
    });
    return reply({
      ...publicInstallation(installation),
      installationCredential: await this.credential(installation.id, installation.credentialGeneration ?? 0),
    });
  }

  private async enroll(request: Request): Promise<Response> {
    const input = await body(request);
    const idempotencyKey = input.nonce;
    const allowedKeys = new Set(['nonce', 'deviceName']);
    if (
      !Object.keys(input).every((key) => allowedKeys.has(key))
      || typeof idempotencyKey !== 'string'
      || !IDEMPOTENCY_KEY.test(idempotencyKey)
    ) {
      return reply({ error: 'invalid request' }, 400);
    }
    const requestKey = `enrollment:${String(idempotencyKey)}`;
    let id = await this.state.storage.get<string>(requestKey);
    let installation = id
      ? await this.state.storage.get<Installation>(`installation:${id}`)
      : undefined;
    if (installation?.revoked) {
      // The rightful owner (the only party who knows this installation's
      // enrollment nonce) is re-enabling after a prior disable/revoke.
      // Reprovision the SAME hostname with a fresh generation on next
      // /provision, instead of minting a brand-new installation identity.
      // A different owner can never reach this branch: they would need to
      // know this exact nonce, which is never exposed by any API response.
      const stats = await this.stats();
      if ((this.env.ENROLLMENT_ENABLED ?? 'true') !== 'true') {
        await this.bumpDenied(stats, 'enrollmentDenied');
        throw new Limited(503, 300, 'enrollment unavailable');
      }
      if (stats.activeInstallations >= this.installationLimit()) {
        await this.bumpDenied(stats, 'enrollmentDenied');
        throw new Limited(503, 300, 'enrollment unavailable');
      }
      installation.revoked = false;
      installation.phase = 'disabled';
      installation.desiredEnabled = false;
      delete installation.generation;
      delete installation.tunnelId;
      delete installation.providerConfirmedAt;
      installation.credentialGeneration = (installation.credentialGeneration ?? 0) + 1;
      installation.updatedAt = new Date().toISOString();
      this.applyDeviceName(installation, input.deviceName);
      stats.activeInstallations += 1;
      await this.state.storage.put({
        [`installation:${id}`]: installation,
        'registry:stats': stats,
      });
    }
    if (!installation) {
      const stats = await this.stats();
      if ((this.env.ENROLLMENT_ENABLED ?? 'true') !== 'true') {
        await this.bumpDenied(stats, 'enrollmentDenied');
        throw new Limited(503, 300, 'enrollment unavailable');
      }
      if (stats.activeInstallations >= this.installationLimit()) {
        await this.bumpDenied(stats, 'enrollmentDenied');
        throw new Limited(503, 300, 'enrollment unavailable');
      }
      const source = normalizeSourceAddress(request.headers.get('cf-connecting-ip') ?? '');
      if (!source) return reply({ error: 'invalid request' }, 400);
      const exempt = exemptSources(this.env.ENROLLMENT_EXEMPT_SOURCES).has(source);
      const sourceHash = await keyedHash(this.env.INSTALLATION_SIGNING_KEY, `enrollment-source:${source}`);
      const nowSeconds = Math.floor(Date.now() / 1000);
      const sourceKey = `enrollment-source:${sourceHash}`;
      const windowSeconds = this.enrollmentWindowSeconds();
      let quota = await this.state.storage.get<SourceQuota>(sourceKey);
      if (quota && nowSeconds - quota.startedAt >= windowSeconds) quota = undefined;
      if (!exempt && quota && quota.count >= ENROLLMENTS_PER_SOURCE) {
        await this.bumpDenied(stats, 'enrollmentDenied');
        throw new Limited(429, Math.max(1, windowSeconds - (nowSeconds - quota.startedAt)), 'enrollment rate limited');
      }
      const index = await this.pruneSourceQuotas(nowSeconds);
      if (!quota && !index.includes(sourceHash) && index.length >= MAX_SOURCE_QUOTAS) {
        await this.bumpDenied(stats, 'enrollmentDenied');
        throw new Limited(503, 300, 'enrollment unavailable');
      }
      // id lineage: 32-char (legacy) -> 16-char -> now 8-char for new
      // enrollments; see generateUniqueInstallationId's comment above.
      // Existing 16-char/32-char installations are untouched.
      id = await generateUniqueInstallationId(this.state.storage);
      const now = new Date().toISOString();
      installation = {
        id,
        hostname: `sp-${id}.${this.env.REMOTE_HOST_SUFFIX.toLowerCase()}`,
        label: '',
        phase: 'disabled',
        desiredEnabled: false,
        revoked: false,
        createdAt: now,
        updatedAt: now,
      };
      this.applyDeviceName(installation, input.deviceName);
      const nextQuota: SourceQuota = {
        startedAt: quota?.startedAt ?? nowSeconds,
        count: (quota?.count ?? 0) + 1,
        lastSeenAt: nowSeconds,
      };
      stats.activeInstallations += 1;
      stats.enrollments += 1;
      await this.state.storage.put({
        [requestKey]: id,
        [`installation:${id}`]: installation,
        [sourceKey]: nextQuota,
        'enrollment-source-index': index.includes(sourceHash) ? index : [...index, sourceHash],
        'registry:stats': stats,
      });
    }
    if (installation && !installation.revoked) {
      // Idempotent replay of an already-known, active installation (most
      // common case for the lightweight check-in that calls this route on
      // every run() start): still allow deviceName to be refreshed/stored
      // without touching credential/phase/quota bookkeeping.
      if (this.applyDeviceName(installation, input.deviceName)) {
        installation.updatedAt = new Date().toISOString();
        await this.save(installation);
      }
    }
    return reply({
      ...publicInstallation(installation),
      installationCredential: await this.credential(installation.id, installation.credentialGeneration ?? 0),
    }, 201);
  }

  // Per-source-IP rate limiting for the anonymous PCO OAuth routes, mirroring
  // the anonymous enroll() per-source quota pattern (keyed hash of the
  // source IP so raw addresses are never persisted).
  private async takeOauthRate(request: Request, kind: 'flow' | 'token'): Promise<void> {
    const source = normalizeSourceAddress(request.headers.get('cf-connecting-ip') ?? '');
    if (!source) throw new Limited(429, 60, 'oauth request rate limited');
    const now = Math.floor(Date.now() / 1000);
    const hash = await keyedHash(this.env.INSTALLATION_SIGNING_KEY, `oauth-source:${source}`);
    const subject = `${kind}:${hash}`;
    const indexKey = 'oauth-source-index';
    const index = await this.state.storage.get<string[]>(indexKey) ?? [];
    const retained: string[] = [];
    for (const candidate of index) {
      const stored = await this.state.storage.get<RateWindow>(`oauth-source:${candidate}`);
      if (stored && now - stored.startedAt < 60) retained.push(candidate);
      else await this.state.storage.delete(`oauth-source:${candidate}`);
    }
    if (!retained.includes(subject) && retained.length >= MAX_OAUTH_RATE_SOURCES) {
      throw new Limited(503, 60, 'oauth service unavailable');
    }
    const key = `oauth-source:${subject}`;
    const current = await this.state.storage.get<RateWindow>(key);
    const window = !current || now - current.startedAt >= 60 ? { startedAt: now, count: 0 } : current;
    const limit = kind === 'flow' ? OAUTH_FLOWS_PER_MINUTE : OAUTH_TOKEN_REQUESTS_PER_MINUTE;
    if (window.count >= limit) throw new Limited(429, Math.max(1, 60 - (now - window.startedAt)), 'oauth request rate limited');
    await this.state.storage.put({
      [key]: { ...window, count: window.count + 1 },
      [indexKey]: retained.includes(subject) ? retained : [...retained, subject],
    });
  }

  // Mints a per-flow HMAC ticket bound to a fresh flow_id, per "Approved
  // implementation decisions" §3 in docs/planning-center-oauth-design.md:
  // reachable by any legitimate desktop instance mid-OAuth-flow (not gated
  // behind an existing installation credential), protected only by the same
  // anonymous per-source-IP rate limiting as enroll().
  private async pcoOauthFlow(request: Request): Promise<Response> {
    await this.takeOauthRate(request, 'flow');
    const flowId = randomHex(PCO_OAUTH_FLOW_ID_BYTES);
    const ticket = await keyedHash(this.env.INSTALLATION_SIGNING_KEY, `pco-oauth-flow:${flowId}`);
    return reply({ flow_id: flowId, ticket }, 201);
  }

  private async verifyOauthTicket(flowId: unknown, ticket: unknown): Promise<boolean> {
    if (typeof flowId !== 'string' || !OAUTH_FLOW_ID.test(flowId) || typeof ticket !== 'string' || ticket.length === 0) {
      return false;
    }
    const expected = await keyedHash(this.env.INSTALLATION_SIGNING_KEY, `pco-oauth-flow:${flowId}`);
    return equalSecret(ticket, expected);
  }

  // Relays an authorization_code or refresh_token grant to Planning
  // Center's token endpoint, holding client_secret server-side per the
  // approved design (desktop never sees it). Requires a valid {flow_id,
  // ticket} minted by pcoOauthFlow() above -- see the "Route auth/abuse-
  // control decision" in the design doc.
  private async pcoOauthExchange(request: Request, grantType: 'authorization_code' | 'refresh_token'): Promise<Response> {
    await this.takeOauthRate(request, 'token');
    const input = await body(request);
    if (!(await this.verifyOauthTicket(input.flow_id, input.ticket))) {
      return reply({ error: 'invalid or expired flow ticket' }, 401);
    }
    const form = new URLSearchParams({
      grant_type: grantType,
      client_id: this.env.PLANNING_CENTER_CLIENT_ID,
      client_secret: this.env.PLANNING_CENTER_CLIENT_SECRET,
    });
    if (grantType === 'authorization_code') {
      const code = input.code;
      const codeVerifier = input.code_verifier;
      const redirectUri = input.redirect_uri;
      if (typeof code !== 'string' || code.length === 0
        || typeof codeVerifier !== 'string' || codeVerifier.length === 0
        || typeof redirectUri !== 'string' || codeVerifier.length === 0) {
        return reply({ error: 'invalid request' }, 400);
      }
      form.set('code', code);
      form.set('code_verifier', codeVerifier);
      form.set('redirect_uri', redirectUri);
    } else {
      const refreshToken = input.refresh_token;
      if (typeof refreshToken !== 'string' || refreshToken.length === 0) {
        return reply({ error: 'invalid request' }, 400);
      }
      form.set('refresh_token', refreshToken);
    }
    let response: Response;
    try {
      response = await fetch(PCO_TOKEN_ENDPOINT, {
        method: 'POST',
        headers: { 'content-type': 'application/x-www-form-urlencoded', accept: 'application/json' },
        body: form.toString(),
      });
    } catch {
      return reply({ error: 'planning center unavailable' }, 503, 30);
    }
    let payload: unknown;
    try {
      payload = await response.json();
    } catch {
      return reply({ error: 'planning center returned an invalid response' }, 502);
    }
    if (!response.ok) {
      // Relay PCO's own error shape (e.g. { error: "invalid_grant" }) so the
      // desktop/backend can distinguish transient vs. permanent (revoked/
      // expired refresh token) failures, without leaking client_secret or
      // any StagePilot-side diagnostics.
      const errorPayload = payload && typeof payload === 'object' ? payload : { error: 'token exchange failed' };
      return reply(errorPayload, response.status === 400 || response.status === 401 ? 400 : 502);
    }
    return reply(payload as Record<string, unknown>, 200);
  }

  private installationLimit(): number {
    return Number(this.env.BETA_INSTALLATION_LIMIT ?? '500');
  }

  private enrollmentWindowSeconds(): number {
    const configured = Number(this.env.ENROLLMENT_WINDOW_SECONDS ?? '');
    return Number.isFinite(configured) && configured > 0
      ? configured
      : DEFAULT_ENROLLMENT_WINDOW_SECONDS;
  }

  private async stats(): Promise<RegistryStats> {
    const existing = await this.state.storage.get<RegistryStats>('registry:stats');
    if (existing) {
      // Backward-compat: registry:stats already exists in production from
      // before diagnosticsBytesUsed was added, so it would be `undefined`
      // here rather than correctly defaulting to 0 -- patch and persist it
      // once rather than leaving the cap check doing arithmetic on
      // `undefined` on every single upload.
      if (existing.diagnosticsBytesUsed === undefined) {
        existing.diagnosticsBytesUsed = 0;
        await this.state.storage.put('registry:stats', existing);
      }
      return existing;
    }
    const rows = await this.state.storage.list<Installation>({ prefix: 'installation:' });
    const stats: RegistryStats = {
      activeInstallations: [...rows.values()].filter((row) => !row.revoked).length,
      enrollments: rows.size,
      enrollmentDenied: 0,
      statusDenied: 0,
      mutationDenied: 0,
      providerDenied: 0,
      diagnosticsBytesUsed: 0,
    };
    await this.state.storage.put('registry:stats', stats);
    return stats;
  }

  private async bumpDenied(stats: RegistryStats, field: 'enrollmentDenied' | 'statusDenied' | 'mutationDenied' | 'providerDenied'): Promise<void> {
    stats[field] += 1;
    await this.state.storage.put('registry:stats', stats);
  }

  private async pruneSourceQuotas(now: number): Promise<string[]> {
    const index = await this.state.storage.get<string[]>('enrollment-source-index') ?? [];
    const retained: string[] = [];
    for (const hash of index) {
      const key = `enrollment-source:${hash}`;
      const quota = await this.state.storage.get<SourceQuota>(key);
      if (quota && now - quota.lastSeenAt < this.enrollmentWindowSeconds()) retained.push(hash);
      else await this.state.storage.delete(key);
    }
    return retained;
  }

  private async takeInstallationRate(installation: Installation, kind: 'status' | 'mutation'): Promise<void> {
    const now = Math.floor(Date.now() / 1000);
    const field = kind === 'status' ? 'statusRate' : 'mutationRate';
    const limit = kind === 'status' ? STATUS_REQUESTS_PER_MINUTE : MUTATION_REQUESTS_PER_MINUTE;
    const current = installation[field];
    const window = !current || now - current.startedAt >= 60
      ? { startedAt: now, count: 0 }
      : current;
    if (window.count >= limit) {
      await this.bumpDenied(await this.stats(), kind === 'status' ? 'statusDenied' : 'mutationDenied');
      throw new Limited(429, Math.max(1, 60 - (now - window.startedAt)), `${kind} rate limited`);
    }
    installation[field] = { ...window, count: window.count + 1 };
    await this.save(installation);
  }

  private async provision(request: Request, installation: Installation): Promise<Response> {
    const input = await body(request);
    const generation = input.generation;
    if (typeof generation !== 'string' || !GENERATION.test(generation)) {
      return reply({ error: 'invalid generation' }, 400);
    }
    if (installation.phase === 'revoking') return reply({ error: 'disable must finish first' }, 409);
    if (installation.generation && installation.generation !== generation) {
      return reply({ error: 'generation conflict' }, 409);
    }
    if (installation.generation === generation && installation.phase === 'provisioned') {
      const cached = this.cachedProvision(installation);
      if (cached) return cached;
    }
    if (!installation.generation && installation.lastGeneration === generation) {
      return reply({ error: 'a new generation is required' }, 409);
    }
    installation.generation = generation;
    installation.desiredEnabled = true;
    installation.phase = 'enabling';
    installation.updatedAt = new Date().toISOString();
    this.applyDeviceName(installation, input.deviceName);
    await this.save(installation);
    return this.ensureProvisioned(installation);
  }

  private async reconcile(request: Request, installation: Installation): Promise<Response> {
    let deviceNameInput: unknown;
    try {
      deviceNameInput = (await body(request)).deviceName;
    } catch {
      deviceNameInput = undefined;
    }
    if (this.applyDeviceName(installation, deviceNameInput)) {
      installation.updatedAt = new Date().toISOString();
      await this.save(installation);
    }
    if (installation.desiredEnabled && installation.generation) {
      const cached = this.cachedProvision(installation);
      if (cached) return cached;
      return this.ensureProvisioned(installation);
    }
    if (installation.phase === 'disabled') return reply(publicInstallation(installation));
    return this.disable(installation, false);
  }

  private async ensureProvisioned(installation: Installation): Promise<Response> {
    const generation = installation.generation;
    if (!generation) throw new Error('missing generation');
    const cached = this.cachedProvision(installation);
    if (cached) return cached;
    const name = this.tunnelName(installation, generation);
    let tunnel = await this.tunnel(name);
    let record = await this.dns(installation.hostname);
    if (record && (!tunnel || !this.owned(record, name, installation.hostname, tunnel.id))) {
      // Only a provably stale resource of an older generation of THIS
      // installation is replaced; everything else still fails closed.
      const stale = await this.staleOwnGeneration(installation, record, generation);
      if (!stale) throw new Error('hostname ownership conflict');
      await this.removeStale(installation, stale);
      record = undefined;
    }
    if (!tunnel) {
      await this.cf('POST', this.tunnelsPath(), { name, config_src: 'cloudflare' });
      tunnel = await this.tunnel(name);
      if (!tunnel) throw new Error('tunnel creation not confirmed');
    }
    await this.configure(tunnel.id, installation.hostname);
    if (!record) {
      await this.cf('POST', this.recordsPath(), {
        type: 'CNAME',
        name: installation.hostname,
        content: `${tunnel.id}.cfargotunnel.com`,
        proxied: true,
        ttl: 1,
        comment: name,
      });
      record = await this.dns(installation.hostname);
    }
    if (!record || !this.owned(record, name, installation.hostname, tunnel.id)) {
      throw new Error('hostname route not confirmed');
    }
    const token = await this.cf<unknown>('GET', `${this.tunnelsPath()}/${tunnel.id}/token`);
    if (typeof token !== 'string' || token.length < 20 || /\s/.test(token)) {
      throw new Error('installation credential unavailable');
    }
    installation.tunnelId = tunnel.id;
    installation.phase = 'provisioned';
    installation.providerConfirmedAt = Math.floor(Date.now() / 1000);
    installation.updatedAt = new Date().toISOString();
    installation.everProvisioned = true;
    await this.save(installation);
    this.tokenCache.set(installation.id, {
      token,
      expiresAt: Math.floor(Date.now() / 1000) + RECONCILE_CACHE_SECONDS,
    });
    return reply({
      ...publicInstallation(installation),
      tunnelId: tunnel.id,
      tunnelToken: token,
    });
  }

  private cachedProvision(installation: Installation): Response | undefined {
    const cached = this.tokenCache.get(installation.id);
    const now = Math.floor(Date.now() / 1000);
    if (installation.phase !== 'provisioned' || !installation.tunnelId
      || !installation.providerConfirmedAt || now - installation.providerConfirmedAt > RECONCILE_CACHE_SECONDS
      || !cached || cached.expiresAt < now) return undefined;
    return reply({
      ...publicInstallation(installation),
      tunnelId: installation.tunnelId,
      tunnelToken: cached.token,
    });
  }

  private async disable(installation: Installation, revoke: boolean): Promise<Response> {
    const wasRevoked = installation.revoked;
    installation.desiredEnabled = false;
    installation.phase = 'revoking';
    installation.revoked ||= revoke;
    installation.updatedAt = new Date().toISOString();
    await this.save(installation);
    const generation = installation.generation;
    if (generation) {
      const name = this.tunnelName(installation, generation);
      const tunnel = await this.tunnel(name);
      const tunnelId = tunnel?.id ?? installation.tunnelId;
      const record = await this.dns(installation.hostname);
      if (record) {
        if (!tunnelId || !this.owned(record, name, installation.hostname, tunnelId)) {
          const stale = await this.staleOwnGeneration(installation, record, generation);
          if (!stale) throw new Error('hostname ownership conflict');
          await this.removeStale(installation, stale);
        } else {
          await this.cf('DELETE', `${this.recordsPath()}/${String(record.id)}`);
          if (await this.dns(installation.hostname)) throw new Error('hostname removal not confirmed');
        }
      }
      if (tunnel) {
        await this.configure(tunnel.id);
        await this.cf('DELETE', `${this.tunnelsPath()}/${tunnel.id}/connections`);
        await this.cf('DELETE', `${this.tunnelsPath()}/${tunnel.id}`);
        if (await this.tunnel(name)) throw new Error('tunnel revocation not confirmed');
      }
    }
    installation.phase = 'disabled';
    installation.lastGeneration = installation.generation;
    delete installation.generation;
    delete installation.tunnelId;
    delete installation.providerConfirmedAt;
    this.tokenCache.delete(installation.id);
    installation.updatedAt = new Date().toISOString();
    await this.save(installation);
    if (revoke && !wasRevoked) {
      const stats = await this.stats();
      stats.activeInstallations = Math.max(0, stats.activeInstallations - 1);
      await this.state.storage.put('registry:stats', stats);
    }
    return reply(publicInstallation(installation));
  }

  private async adminRevoke(id: string): Promise<Response> {
    const installation = await this.state.storage.get<Installation>(`installation:${id}`);
    if (!installation) return reply({ error: 'not found' }, 404);
    if (installation.revoked && installation.phase === 'disabled') {
      return reply(publicInstallation(installation));
    }
    return this.disable(installation, true);
  }

  // Deterministic dedup key derived from the same (category, subject) pair
  // the installation's own local cooldown/dedup engine already keys on
  // (see services/critical_alerts.py). Re-forwarding the same open alert
  // updates the existing record instead of creating a duplicate.
  private async alertKey(category: string, subject: string): Promise<string> {
    return await keyedHash(this.env.INSTALLATION_SIGNING_KEY, `${category}\u0000${subject}`);
  }

  private async ingestAlert(request: Request, installation: Installation): Promise<Response> {
    const input = await body(request);
    const { category, subject, message, severity, first_seen: firstSeen, last_seen: lastSeen, count } = input;
    if (
      typeof category !== 'string' || category.length < 1 || category.length > 200
      || typeof subject !== 'string' || subject.length < 1 || subject.length > 500
      || typeof message !== 'string' || message.length < 1 || message.length > 4000
      || (severity !== 'warning' && severity !== 'critical')
      || typeof firstSeen !== 'string'
      || typeof lastSeen !== 'string'
      || typeof count !== 'number' || !Number.isInteger(count) || count < 1
    ) {
      return reply({ error: 'invalid request' }, 400);
    }
    const hash = await this.alertKey(category, subject);
    const key = `alert:${installation.id}:${category}:${hash}`;
    const now = new Date().toISOString();
    const existing = await this.state.storage.get<AlertRecord>(key);
    const location = formatInstallationLocation(installation);
    const record: AlertRecord = existing
      ? {
        ...existing,
        message,
        severity,
        firstSeen,
        lastSeen,
        count,
        // A fresh forwarded occurrence reopens a previously acknowledged alert,
        // matching the local store's own "acknowledged then new occurrence ->
        // notify fresh" semantics.
        status: 'open',
        updatedAt: now,
        location,
      }
      : {
        id: `${installation.id}:${category}:${hash}`,
        installationId: installation.id,
        category,
        subject,
        message,
        severity,
        firstSeen,
        lastSeen,
        count,
        status: 'open',
        createdAt: now,
        updatedAt: now,
        location,
      };
    await this.state.storage.put(key, record);
    return reply(publicAlert(record), existing ? 200 : 201);
  }

  // Per-installation detail list for the fleet admin panel. Distinct from
  // publicInstallation()'s status-check shape: this also includes the
  // operator-settable `label` and the location fields already captured for
  // the cities summary, so the operator can tell installations apart (which
  // is "online today", which is a stale/duplicate dev enrollment, etc.)
  // instead of only ever seeing a raw aggregate count. Revoked installations
  // are included (filterable by the caller) rather than silently hidden, so
  // a past revoke's effect stays visible/auditable.
  private async adminListInstallations(): Promise<Response> {
    const rows = await this.state.storage.list<Installation>({ prefix: 'installation:' });
    const installations = [...rows.values()]
      .sort((left, right) => right.updatedAt.localeCompare(left.updatedAt))
      .map((installation) => ({
        ...publicInstallation(installation),
        label: installation.label || '',
        archived: installation.archived ?? false,
        location: formatInstallationLocation(installation),
        everProvisioned: installation.everProvisioned ?? false,
      }));
    return reply({ installations });
  }

  // Operator-settable human-readable name only -- never touches identity,
  // credentials, or lifecycle state. A blank label is valid (clears it back
  // to unlabeled) and the only other accepted shape; this is intentionally
  // the single narrow mutation surface for installation metadata, not a
  // general-purpose installation-editing endpoint.
  private async adminSetLabel(request: Request, id: string): Promise<Response> {
    const installation = await this.state.storage.get<Installation>(`installation:${id}`);
    if (!installation) return reply({ error: 'not found' }, 404);
    const input = await body(request);
    const { label } = input;
    if (typeof label !== 'string' || label.length > 200) {
      return reply({ error: 'invalid request' }, 400);
    }
    const updated: Installation = { ...installation, label, updatedAt: new Date().toISOString() };
    await this.state.storage.put(`installation:${id}`, updated);
    return reply({ ...publicInstallation(updated), label: updated.label, location: formatInstallationLocation(updated) });
  }

  // Hide/unhide from the default admin-panel list WITHOUT touching access.
  // Unlike revoke, this never changes `revoked`/`phase`/credentials and
  // never calls out to Cloudflare -- it is a pure metadata flag, safe to
  // flip on an installation that is still actively in use. This is the
  // explicit alternative to revoke for "I don't need to look at this one
  // regularly but it should keep working."
  private async adminSetArchived(request: Request, id: string): Promise<Response> {
    const installation = await this.state.storage.get<Installation>(`installation:${id}`);
    if (!installation) return reply({ error: 'not found' }, 404);
    const input = await body(request);
    const { archived } = input;
    if (typeof archived !== 'boolean') {
      return reply({ error: 'invalid request' }, 400);
    }
    const updated: Installation = { ...installation, archived, updatedAt: new Date().toISOString() };
    await this.state.storage.put(`installation:${id}`, updated);
    return reply({ ...publicInstallation(updated), label: updated.label || '', archived: updated.archived ?? false, location: formatInstallationLocation(updated) });
  }

  // Simple passive "which cities are running an installation" list for the
  // fleet admin panel. Location comes only from `lastCity`/`lastRegion`/
  // `lastCountry`, themselves populated from Cloudflare edge geolocation
  // (never a raw IP) on routine status/provision/reconcile requests. This
  // is intentionally computed on read rather than cached, matching the
  // expected scale of a private beta fleet.
  private async adminListCities(): Promise<Response> {
    const rows = await this.state.storage.list<Installation>({ prefix: 'installation:' });
    const counts = new Map<string, number>();
    for (const installation of rows.values()) {
      if (installation.revoked) continue;
      const location = formatInstallationLocation(installation);
      counts.set(location, (counts.get(location) ?? 0) + 1);
    }
    const cities = [...counts.entries()]
      .map(([location, count]) => ({ location, count }))
      .sort((left, right) => right.count - left.count || left.location.localeCompare(right.location));
    return reply({ cities });
  }

  private async adminListAlerts(url: URL): Promise<Response> {
    const statusFilter = url.searchParams.get('status');
    if (statusFilter !== null && statusFilter !== 'open' && statusFilter !== 'acknowledged') {
      return reply({ error: 'invalid status filter' }, 400);
    }
    const rows = await this.state.storage.list<AlertRecord>({ prefix: 'alert:' });
    const alerts = [...rows.values()]
      .filter((alert) => !statusFilter || alert.status === statusFilter)
      .sort((left, right) => right.updatedAt.localeCompare(left.updatedAt))
      .map(publicAlert);
    return reply({ alerts });
  }

  private async adminAcknowledgeAlert(alertId: string): Promise<Response> {
    const rows = await this.state.storage.list<AlertRecord>({ prefix: 'alert:' });
    const entry = [...rows.entries()].find(([, alert]) => alert.id === alertId);
    if (!entry) return reply({ error: 'not found' }, 404);
    const [key, alert] = entry;
    const updated: AlertRecord = { ...alert, status: 'acknowledged', updatedAt: new Date().toISOString() };
    await this.state.storage.put(key, updated);
    return reply(publicAlert(updated));
  }

  // --- Diagnostic bundle uploads -------------------------------------------
  //
  // Bundles are opaque JSON/text blobs (frontend log + backend log + app and
  // version info) built client-side by the Tauri "send logs" command. The
  // Worker never parses the contents -- it only size-checks, rate-limits,
  // and stores the bytes in R2 -- so a change to the bundle's internal shape
  // never requires a control-plane deploy. R2Object metadata (key, size,
  // uploadedAt) is mirrored into DO storage so the admin list route doesn't
  // need an R2 list call (and so it survives even if R2 lifecycle rules have
  // not yet expired a bundle that storage has already forgotten -- not
  // expected to diverge, but DO storage stays the single source of truth for
  // what the admin panel shows).

  private async takeDiagnosticsRate(installation: Installation): Promise<void> {
    const now = Math.floor(Date.now() / 1000);
    const current = installation.diagnosticsRate;
    const window = !current || now - current.startedAt >= 3600
      ? { startedAt: now, count: 0 }
      : current;
    if (window.count >= DIAGNOSTICS_UPLOADS_PER_HOUR) {
      throw new Limited(429, Math.max(1, 3600 - (now - window.startedAt)), 'diagnostics upload rate limited');
    }
    installation.diagnosticsRate = { ...window, count: window.count + 1 };
    await this.save(installation);
  }

  private diagnosticsKey(installationId: string, uploadedAt: string, suffix: string): string {
    // e.g. diagnostics/<installationId>/20260401T120000Z-a1b2c3d4 -- sortable
    // by time and collision-resistant without leaking any bundle contents in
    // the key itself.
    const stamp = uploadedAt.replace(/[-:]/g, '').replace(/\.\d+Z$/, 'Z');
    return `diagnostics/${installationId}/${stamp}-${suffix}`;
  }

  private async uploadDiagnostics(request: Request, installation: Installation): Promise<Response> {
    if (!this.env.DIAGNOSTICS) return reply({ error: 'diagnostics storage unavailable' }, 503);
    let bytes: Uint8Array;
    try {
      bytes = await sizedBody(request, MAX_DIAGNOSTICS_BUNDLE_BYTES);
    } catch {
      return reply({ error: 'invalid request' }, 400);
    }
    const stats = await this.stats();
    // Opportunistic lazy expiry: reclaim space from bundles past their
    // retention window before deciding whether this new upload fits under
    // the cap. Runs on every upload rather than a separate cron trigger, so
    // it needs no extra scheduling infrastructure; worst case an expired
    // bundle lingers a little past its exact expiry if uploads stop
    // entirely, which is fine -- the cap check below is the real guarantee,
    // this sweep only keeps it from being needlessly conservative.
    await this.expireOldDiagnostics(stats);
    if (stats.diagnosticsBytesUsed + bytes.byteLength > DIAGNOSTICS_STORAGE_CAP_BYTES) {
      return reply({ error: 'diagnostics storage cap reached; try again later' }, 503);
    }
    const uploadedAt = new Date().toISOString();
    const suffix = randomHex(4);
    const objectKey = this.diagnosticsKey(installation.id, uploadedAt, suffix);
    await this.env.DIAGNOSTICS.put(objectKey, bytes, {
      httpMetadata: { contentType: 'application/json' },
    });
    const meta: DiagnosticsBundleMeta = {
      installationId: installation.id,
      key: objectKey,
      uploadedAt,
      size: bytes.byteLength,
    };
    await this.state.storage.put(`diagnostics-meta:${installation.id}:${uploadedAt}:${suffix}`, meta);
    stats.diagnosticsBytesUsed += bytes.byteLength;
    await this.state.storage.put('registry:stats', stats);
    return reply({ uploadedAt, size: meta.size }, 201);
  }

  // Deletes R2 objects (and their metadata records) past
  // DIAGNOSTICS_RETENTION_DAYS and decrements the running byte total
  // accordingly. Bounded to a modest batch per call (worst case on a cold
  // bucket with a long-idle Worker) so a single upload request's latency
  // doesn't balloon; any remainder is cleaned up on the next upload.
  private async expireOldDiagnostics(stats: RegistryStats): Promise<void> {
    const cutoff = Date.now() - DIAGNOSTICS_RETENTION_DAYS * 24 * 60 * 60 * 1000;
    const rows = await this.state.storage.list<DiagnosticsBundleMeta>({ prefix: 'diagnostics-meta:' });
    let swept = 0;
    for (const [metaKey, meta] of rows) {
      if (swept >= 25) break;
      if (new Date(meta.uploadedAt).getTime() > cutoff) continue;
      if (this.env.DIAGNOSTICS) await this.env.DIAGNOSTICS.delete(meta.key);
      await this.state.storage.delete(metaKey);
      stats.diagnosticsBytesUsed = Math.max(0, stats.diagnosticsBytesUsed - meta.size);
      swept += 1;
    }
  }

  // The "id" segment in the admin GET route is the `<stamp>-<suffix>` tail of
  // the stored object key (not the full R2 key, which also embeds the
  // installation id already present in the URL path).
  private async findDiagnosticsMeta(installationId: string, idSegment: string): Promise<DiagnosticsBundleMeta | undefined> {
    const rows = await this.state.storage.list<DiagnosticsBundleMeta>({ prefix: `diagnostics-meta:${installationId}:` });
    return [...rows.values()].find((meta) => meta.key.endsWith(`/${idSegment}`));
  }

  private async adminListDiagnostics(installationId: string): Promise<Response> {
    const installation = await this.state.storage.get<Installation>(`installation:${installationId}`);
    if (!installation) return reply({ error: 'not found' }, 404);
    const rows = await this.state.storage.list<DiagnosticsBundleMeta>({ prefix: `diagnostics-meta:${installationId}:` });
    const bundles = [...rows.values()]
      .sort((left, right) => right.uploadedAt.localeCompare(left.uploadedAt))
      .map((meta) => ({
        id: meta.key.split('/').at(-1),
        uploadedAt: meta.uploadedAt,
        size: meta.size,
      }));
    return reply({ bundles });
  }

  private async adminGetDiagnostic(installationId: string, idSegment: string): Promise<Response> {
    if (!this.env.DIAGNOSTICS) return reply({ error: 'diagnostics storage unavailable' }, 503);
    const meta = await this.findDiagnosticsMeta(installationId, idSegment);
    if (!meta) return reply({ error: 'not found' }, 404);
    const object = await this.env.DIAGNOSTICS.get(meta.key);
    if (!object) return reply({ error: 'not found' }, 404);
    // Stream the object straight through the Worker under the same admin
    // bearer-token auth already enforced above -- simpler and just as safe
    // as a presigned URL for this codebase (no separate R2 S3-compatible
    // credential/signing setup needed), and the object is capped at 2MB so
    // buffering it through the Worker is cheap.
    return new Response(object.body, {
      status: 200,
      headers: {
        'content-type': 'application/json; charset=utf-8',
        'cache-control': 'no-store',
        'content-disposition': `attachment; filename="${meta.uploadedAt}-${installationId}.json"`,
      },
    });
  }

  // --- Fleet admin actions ------------------------------------------------
  //
  // Design note (documented in the PR): an installation's login rate-limit
  // state (the `attempts` table in services/remote_auth.py) lives on that
  // installation's own machine and is unreachable from the Worker -- the
  // control plane never dials an installation, installations always dial it.
  // The Worker therefore records a pending desired action, exactly mirroring
  // the existing desired-state pattern (`desiredEnabled` + the installation's
  // own reconcile/status poll), and the installation applies it and acks it.
  // Queuing is idempotent per (installation, kind): re-requesting a reset that
  // has not been applied yet returns the same pending action rather than
  // stacking duplicates.

  private pendingActionKey(installationId: string, kind: PendingActionKind): string {
    return `pending-action:${installationId}:${kind}`;
  }

  private async pendingActions(installationId: string): Promise<Record<string, unknown>[]> {
    const rows = await this.state.storage.list<PendingAction>({ prefix: `pending-action:${installationId}:` });
    return [...rows.values()]
      .sort((left, right) => left.requestedAt.localeCompare(right.requestedAt))
      .map(publicPendingAction);
  }

  private async adminQueueRateLimitReset(id: string): Promise<Response> {
    const installation = await this.state.storage.get<Installation>(`installation:${id}`);
    if (!installation || installation.revoked) return reply({ error: 'not found' }, 404);
    const key = this.pendingActionKey(installation.id, 'rate_limit_reset');
    const existing = await this.state.storage.get<PendingAction>(key);
    if (existing) return reply(publicPendingAction(existing));
    const action: PendingAction = {
      id: `${installation.id}:rate_limit_reset:${randomHex(8)}`,
      installationId: installation.id,
      kind: 'rate_limit_reset',
      requestedAt: new Date().toISOString(),
    };
    await this.state.storage.put(key, action);
    return reply(publicPendingAction(action), 201);
  }

  // Only the installation the action was queued for can clear it: the key is
  // derived from the authenticated installation's own id, so an action can
  // never be acknowledged (and thereby suppressed) on another's behalf.
  private async acknowledgePendingAction(installation: Installation, actionId: string): Promise<Response> {
    const rows = await this.state.storage.list<PendingAction>({ prefix: `pending-action:${installation.id}:` });
    const entry = [...rows.entries()].find(([, action]) => action.id === actionId);
    if (!entry) return reply({ error: 'not found' }, 404);
    await this.state.storage.delete(entry[0]);
    return reply({ acknowledged: actionId });
  }

  // --- Operator approval requests ----------------------------------------
  //
  // Request/response plumbing only: an installation asks for operator
  // sign-off with a free-text reason, the operator approves or denies, and
  // the requesting installation observes the outcome by polling its own
  // approval-requests list. Deliberately carries NO business logic about what
  // an approval grants -- that belongs to whatever feature raises the request.

  private async createApprovalRequest(request: Request, installation: Installation): Promise<Response> {
    const input = await body(request);
    const reason = input.reason;
    if (typeof reason !== 'string' || reason.length < 1 || reason.length > 1000) {
      return reply({ error: 'invalid request' }, 400);
    }
    const now = new Date().toISOString();
    const approval: ApprovalRequest = {
      id: `${installation.id}:${randomHex(8)}`,
      installationId: installation.id,
      reason,
      status: 'pending',
      createdAt: now,
      updatedAt: now,
    };
    await this.state.storage.put(`approval:${installation.id}:${approval.id}`, approval);
    return reply(publicApprovalRequest(approval), 201);
  }

  private async installationApprovalRequests(installation: Installation): Promise<Response> {
    const rows = await this.state.storage.list<ApprovalRequest>({ prefix: `approval:${installation.id}:` });
    const requests = [...rows.values()]
      .sort((left, right) => right.updatedAt.localeCompare(left.updatedAt))
      .map(publicApprovalRequest);
    return reply({ requests });
  }

  private async adminListApprovalRequests(url: URL): Promise<Response> {
    const statusFilter = url.searchParams.get('status') ?? 'pending';
    if (!['pending', 'approved', 'denied', 'all'].includes(statusFilter)) {
      return reply({ error: 'invalid status filter' }, 400);
    }
    const rows = await this.state.storage.list<ApprovalRequest>({ prefix: 'approval:' });
    const requests = [...rows.values()]
      .filter((approval) => statusFilter === 'all' || approval.status === statusFilter)
      .sort((left, right) => right.updatedAt.localeCompare(left.updatedAt))
      .map(publicApprovalRequest);
    return reply({ requests });
  }

  private async adminDecideApprovalRequest(request: Request, requestId: string): Promise<Response> {
    const input = await body(request);
    const approved = input.approved;
    if (typeof approved !== 'boolean') return reply({ error: 'invalid request' }, 400);
    const rows = await this.state.storage.list<ApprovalRequest>({ prefix: 'approval:' });
    const entry = [...rows.entries()].find(([, approval]) => approval.id === requestId);
    if (!entry) return reply({ error: 'not found' }, 404);
    const [key, approval] = entry;
    // A decision is final: re-deciding an already-resolved request is
    // rejected rather than silently flipping an outcome the installation
    // may already have acted on.
    if (approval.status !== 'pending') return reply({ error: 'already decided' }, 409);
    const updated: ApprovalRequest = {
      ...approval,
      status: approved ? 'approved' : 'denied',
      updatedAt: new Date().toISOString(),
    };
    await this.state.storage.put(key, updated);
    return reply(publicApprovalRequest(updated));
  }

  // Authenticated re-enrollment for "Regenerate Remote link": the caller
  // already proved ownership of `installation` (via isInstallation), so
  // this never touches the anonymous-enrollment source quota that protects
  // /v1/installations/enroll from arbitrary Internet clients. Fully revokes
  // the old installation's tunnel/DNS/credential first (same teardown as
  // disable(..., revoke=true)), then mints a brand-new installation record
  // -- so a legitimate owner can regenerate repeatedly without ever being
  // rate-limited by ENROLLMENTS_PER_SOURCE, while an attacker who does not
  // hold the installation credential still cannot reach this path at all.
  private async reenroll(installation: Installation): Promise<Response> {
    await this.disable(installation, true);
    const stats = await this.stats();
    if ((this.env.ENROLLMENT_ENABLED ?? 'true') !== 'true') {
      await this.bumpDenied(stats, 'enrollmentDenied');
      throw new Limited(503, 300, 'enrollment unavailable');
    }
    if (stats.activeInstallations >= this.installationLimit()) {
      await this.bumpDenied(stats, 'enrollmentDenied');
      throw new Limited(503, 300, 'enrollment unavailable');
    }
    const id = await generateUniqueInstallationId(this.state.storage);
    const now = new Date().toISOString();
    const fresh: Installation = {
      id,
      hostname: `sp-${id}.${this.env.REMOTE_HOST_SUFFIX.toLowerCase()}`,
      label: '',
      phase: 'disabled',
      desiredEnabled: false,
      revoked: false,
      createdAt: now,
      updatedAt: now,
    };
    stats.activeInstallations += 1;
    stats.enrollments += 1;
    await this.state.storage.put({
      [`installation:${id}`]: fresh,
      'registry:stats': stats,
    });
    return reply({
      ...publicInstallation(fresh),
      installationCredential: await this.credential(fresh.id, fresh.credentialGeneration ?? 0),
    }, 201);
  }

  private async save(installation: Installation): Promise<void> {
    await this.state.storage.put(`installation:${installation.id}`, installation);
  }

  // Records coarse, non-identifying location from Cloudflare's edge
  // geolocation on `request.cf` -- never the raw source IP -- so the fleet
  // admin panel can show a passive "which cities are running" list. Called
  // on every authenticated status/provision/reconcile request so a moved
  // installation's location updates after its next check-in.
  // Applies a self-reported deviceName from a request body: validates,
  // stores it if changed, and auto-fills `label` from it ONLY the very
  // first time a label has never been set (tracked by label being
  // currently empty -- once an operator sets any label, even later
  // cleared, this never overwrites it again because clearing a label is
  // an explicit operator action elsewhere, not something this path does).
  // Untrusted/display-only: never used for auth, matching, or any security
  // decision. Caller is responsible for persisting via save().
  private applyDeviceName(installation: Installation, rawDeviceName: unknown): boolean {
    const deviceName = sanitizeDeviceName(rawDeviceName);
    if (deviceName === undefined || deviceName === installation.deviceName) return false;
    const hadDeviceNameBefore = installation.deviceName !== undefined;
    installation.deviceName = deviceName;
    if (!hadDeviceNameBefore && !installation.label) {
      installation.label = deviceName;
    }
    return true;
  }

  private async recordRequestGeo(request: Request, installation: Installation): Promise<void> {
    const cf = (request as Request & { cf?: IncomingRequestCfProperties }).cf;
    const city = typeof cf?.city === 'string' && cf.city.length > 0 ? cf.city : undefined;
    const region = typeof cf?.region === 'string' && cf.region.length > 0 ? cf.region : undefined;
    const country = typeof cf?.country === 'string' && cf.country.length > 0 ? cf.country : undefined;
    if (installation.lastCity === city && installation.lastRegion === region && installation.lastCountry === country) {
      return;
    }
    installation.lastCity = city;
    installation.lastRegion = region;
    installation.lastCountry = country;
    await this.save(installation);
  }

  private tunnelName(installation: Installation, generation: string): string {
    return `stagepilot-${installation.id}-${generation}`;
  }

  private tunnelsPath(): string {
    return `/accounts/${this.env.CLOUDFLARE_ACCOUNT_ID}/cfd_tunnel`;
  }

  private recordsPath(): string {
    return `/zones/${this.env.CLOUDFLARE_ZONE_ID}/dns_records`;
  }

  private async withProviderLane<T>(lane: 'normal' | 'recovery', operation: () => Promise<T>): Promise<T> {
    const previous = this.providerLane;
    this.providerLane = lane;
    try {
      return await operation();
    } finally {
      this.providerLane = previous;
    }
  }

  private async takeProviderBudget(): Promise<void> {
    const now = Math.floor(Date.now() / 1000);
    const current = await this.state.storage.get<RateWindow>('provider:budget');
    const window = !current || now - current.startedAt >= PROVIDER_WINDOW_SECONDS
      ? { startedAt: now, count: 0 }
      : current;
    const ceiling = this.providerLane === 'recovery' ? PROVIDER_TOTAL_BUDGET : PROVIDER_NORMAL_BUDGET;
    if (window.count >= ceiling) {
      await this.bumpDenied(await this.stats(), 'providerDenied');
      throw new Limited(503, Math.max(1, PROVIDER_WINDOW_SECONDS - (now - window.startedAt)), 'provider capacity unavailable');
    }
    await this.state.storage.put('provider:budget', { ...window, count: window.count + 1 });
  }

  private async cf<T>(method: string, path: string, requestBody?: unknown): Promise<T> {
    await this.takeProviderBudget();
    const response = await fetch(`https://api.cloudflare.com/client/v4${path}`, {
      method,
      headers: {
        authorization: `Bearer ${this.env.CLOUDFLARE_API_TOKEN}`,
        'content-type': 'application/json',
      },
      body: requestBody === undefined ? undefined : JSON.stringify(requestBody),
    });
    const operation = path.includes('/dns_records')
      ? 'DNS'
      : path.endsWith('/token')
        ? 'tunnel-token'
        : path.endsWith('/configurations')
          ? 'tunnel-configuration'
          : 'tunnel-lifecycle';
    if (response.status === 429) {
      const retryAfter = Number(response.headers.get('retry-after') ?? '60');
      throw new Limited(503, Number.isFinite(retryAfter) ? Math.max(1, Math.ceil(retryAfter)) : 60, 'provider capacity unavailable');
    }
    if (!response.ok) throw new Error(`provider ${operation} request failed (HTTP ${response.status})`);
    let envelope: CloudflareEnvelope<T>;
    try {
      envelope = (await response.json()) as CloudflareEnvelope<T>;
    } catch {
      throw new Error(`provider ${operation} returned invalid response`);
    }
    if (envelope.success !== true || !('result' in envelope)) {
      throw new Error(`provider rejected ${operation} request`);
    }
    return envelope.result;
  }

  private async tunnel(name: string): Promise<{ id: string; name: string; config_src: string } | undefined> {
    const query = new URLSearchParams({ name, is_deleted: 'false' });
    const rows = await this.cf<Array<{ id: string; name: string; config_src: string; deleted_at?: string }>>(
      'GET', `${this.tunnelsPath()}?${query}`,
    );
    const matches = rows.filter((row) => row.name === name && !row.deleted_at);
    if (matches.length > 1) throw new Error('ambiguous tunnel ownership');
    if (matches[0] && (matches[0].config_src !== 'cloudflare' || !/^[0-9a-f-]{36}$/i.test(matches[0].id))) {
      throw new Error('invalid tunnel ownership');
    }
    return matches[0];
  }

  private async dns(hostname: string): Promise<Record<string, unknown> | undefined> {
    const query = new URLSearchParams({ name: hostname });
    const rows = await this.cf<Array<Record<string, unknown>>>('GET', `${this.recordsPath()}?${query}`);
    const matches = rows.filter((row) => row.name === hostname);
    if (matches.length > 1) throw new Error('ambiguous hostname ownership');
    return matches[0];
  }

  // Returns the stale tunnel/record pair only with full evidence that it
  // belongs to an older generation of this same installation and nothing is
  // connected to it. Any doubt returns undefined (caller fails closed).
  private async staleOwnGeneration(
    installation: Installation,
    record: Record<string, unknown>,
    requestedGeneration: string,
  ): Promise<{ record: Record<string, unknown>; tunnel: { id: string; name: string } } | undefined> {
    const comment = record.comment;
    const prefix = `stagepilot-${installation.id}-`;
    if (typeof comment !== 'string' || !comment.startsWith(prefix)) return undefined;
    const oldGeneration = comment.slice(prefix.length);
    if (!GENERATION.test(oldGeneration) || oldGeneration.toLowerCase() === requestedGeneration.toLowerCase()) {
      return undefined;
    }
    if (record.name !== installation.hostname || record.type !== 'CNAME' || record.proxied !== true) {
      return undefined;
    }
    const tunnel = await this.tunnel(comment);
    if (!tunnel || record.content !== `${tunnel.id}.cfargotunnel.com`) return undefined;
    const connections = await this.cf<unknown>('GET', `${this.tunnelsPath()}/${tunnel.id}/connections`);
    if (!Array.isArray(connections) || connections.length > 0) return undefined;
    return { record, tunnel };
  }

  private async removeStale(
    installation: Installation,
    stale: { record: Record<string, unknown>; tunnel: { id: string; name: string } },
  ): Promise<void> {
    await this.cf('DELETE', `${this.recordsPath()}/${String(stale.record.id)}`);
    if (await this.dns(installation.hostname)) throw new Error('hostname removal not confirmed');
    await this.configure(stale.tunnel.id);
    await this.cf('DELETE', `${this.tunnelsPath()}/${stale.tunnel.id}/connections`);
    await this.cf('DELETE', `${this.tunnelsPath()}/${stale.tunnel.id}`);
    if (await this.tunnel(stale.tunnel.name)) throw new Error('tunnel revocation not confirmed');
  }

  private owned(record: Record<string, unknown>, name: string, hostname: string, tunnelId: string): boolean {
    return record.comment === name && record.name === hostname && record.type === 'CNAME'
      && record.content === `${tunnelId}.cfargotunnel.com` && record.proxied === true;
  }

  private async configure(tunnelId: string, hostname?: string): Promise<void> {
    const ingress = hostname
      ? [
          { hostname, service: `http://127.0.0.1:${this.remotePort()}` },
          { service: 'http_status:404' },
        ]
      : [{ service: 'http_status:404' }];
    const path = `${this.tunnelsPath()}/${tunnelId}/configurations`;
    const config = { ingress, 'warp-routing': { enabled: false } };
    await this.cf('PUT', path, { config });
    const actual = await this.cf<{ config?: { ingress?: unknown; 'warp-routing'?: { enabled?: unknown } } }>('GET', path);
    const actualIngress = actual.config?.ingress;
    const sameIngress = Array.isArray(actualIngress) && actualIngress.length === ingress.length
      && ingress.every((expected, index) => {
        const received = actualIngress[index];
        return received !== null && typeof received === 'object' && !Array.isArray(received)
          && Object.keys(received).length === Object.keys(expected).length
          && Object.entries(expected).every(([key, value]) => (received as Record<string, unknown>)[key] === value);
      });
    if (!sameIngress || actual.config?.['warp-routing']?.enabled !== false) {
      throw new Error('tunnel configuration not confirmed');
    }
  }

  private remotePort(): number {
    const value = Number(this.env.REMOTE_PORT ?? '8766');
    if (!Number.isInteger(value) || value < 1024 || value > 65535 || value === 8765) {
      throw new Error('invalid remote port');
    }
    return value;
  }
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    if (request.method === 'GET' && url.pathname === '/health') return reply({ status: 'ok' });
    // Fleet admin panel: a static page served straight from the assets
    // binding, kept separate from the Registry Durable Object (which only
    // ever serves the JSON /v1/... API the panel's own JS calls).
    if (request.method === 'GET' && (url.pathname === '/admin' || url.pathname === '/admin/')) {
      const assetUrl = new URL('/admin/index.html', url);
      return env.ASSETS ? await env.ASSETS.fetch(new Request(assetUrl.toString(), request)) : reply({ error: 'not found' }, 404);
    }
    if (request.method === 'GET' && url.pathname.startsWith('/admin/') && env.ASSETS) {
      return await env.ASSETS.fetch(request);
    }
    const id = env.REGISTRY.idFromName('stagepilot-private-beta-v1');
    return env.REGISTRY.get(id).fetch(request);
  },
};
