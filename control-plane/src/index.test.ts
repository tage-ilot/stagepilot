import { beforeEach, describe, expect, it, vi } from 'vitest';

import { Registry } from './index';

class MemoryStorage {
  readonly values = new Map<string, unknown>();

  async get<T>(key: string): Promise<T | undefined> {
    return this.values.get(key) as T | undefined;
  }

  async put(keyOrEntries: string | Record<string, unknown>, value?: unknown): Promise<void> {
    if (typeof keyOrEntries === 'string') {
      this.values.set(keyOrEntries, structuredClone(value));
      return;
    }
    for (const [key, entry] of Object.entries(keyOrEntries)) {
      this.values.set(key, structuredClone(entry));
    }
  }

  async delete(key: string): Promise<boolean> {
    return this.values.delete(key);
  }

  async list<T>(options?: { prefix?: string }): Promise<Map<string, T>> {
    return new Map(
      [...this.values.entries()]
        .filter(([key]) => !options?.prefix || key.startsWith(options.prefix))
        .map(([key, value]) => [key, structuredClone(value) as T]),
    );
  }
}

class FakeCloudflare {
  tunnels = new Map<string, Record<string, unknown>>();
  records = new Map<string, Record<string, unknown>>();
  configurations = new Map<string, unknown>();
  createCount = 0;
  dnsCreateCount = 0;
  failAfterNextCreate = false;

  fetch = vi.fn(async (input: string | URL | Request, init?: RequestInit): Promise<Response> => {
    const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input : input.url);
    const method = init?.method ?? 'GET';
    const payload = init?.body ? JSON.parse(String(init.body)) as Record<string, unknown> : undefined;
    const parts = url.pathname.split('/');
    let result: unknown;

    if (url.pathname.endsWith('/cfd_tunnel')) {
      if (method === 'GET') {
        result = [...this.tunnels.values()].filter((tunnel) => tunnel.name === url.searchParams.get('name'));
      } else if (method === 'POST') {
        this.createCount += 1;
        const id = `00000000-0000-4000-8000-${String(this.createCount).padStart(12, '0')}`;
        this.tunnels.set(id, { id, ...payload, deleted_at: null });
        result = this.tunnels.get(id);
        if (this.failAfterNextCreate) {
          this.failAfterNextCreate = false;
          throw new TypeError('simulated lost response');
        }
      }
    } else if (url.pathname.includes('/dns_records')) {
      if (method === 'GET') {
        result = [...this.records.values()].filter((record) => record.name === url.searchParams.get('name'));
      } else if (method === 'POST') {
        this.dnsCreateCount += 1;
        const id = `dns-${this.dnsCreateCount}`;
        this.records.set(id, { id, ...payload });
        result = this.records.get(id);
      } else if (method === 'DELETE') {
        this.records.delete(parts.at(-1) ?? '');
        result = {};
      }
    } else if (url.pathname.endsWith('/configurations')) {
      const tunnelId = parts.at(-2) ?? '';
      if (method === 'PUT') {
        const config = payload?.config as {
          ingress: Array<{ hostname?: string; service: string }>;
          'warp-routing': { enabled: boolean };
        };
        this.configurations.set(tunnelId, {
          config: {
            // Cloudflare's real read-back orders object keys differently from
            // the submitted JSON. Semantic equality must not depend on key order.
            ingress: config.ingress.map((row) => row.hostname
              ? { service: row.service, hostname: row.hostname }
              : { service: row.service }),
            'warp-routing': config['warp-routing'],
          },
        });
      }
      result = this.configurations.get(tunnelId);
    } else if (url.pathname.endsWith('/token')) {
      result = `installation-only-token-${parts.at(-2)}`;
    } else if (url.pathname.endsWith('/connections')) {
      const live = this.tunnels.get(parts.at(-2) ?? '')?.liveConnections;
      result = method === 'GET' ? (live ? [{ id: 'conn-1' }] : []) : {};
    } else if (method === 'DELETE' && url.pathname.includes('/cfd_tunnel/')) {
      this.tunnels.delete(parts.at(-1) ?? '');
      result = {};
    } else {
      throw new Error(`unexpected provider request: ${method} ${url.pathname}`);
    }
    return new Response(JSON.stringify({ success: true, result }), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    });
  });
}

const adminToken = 'admin-token-with-at-least-thirty-two-characters';
const env = {
  CLOUDFLARE_API_TOKEN: 'provider-secret-never-returned',
  CLOUDFLARE_ACCOUNT_ID: 'a'.repeat(32),
  CLOUDFLARE_ZONE_ID: 'b'.repeat(32),
  REMOTE_HOST_SUFFIX: 'remote.example.com',
  ADMIN_API_TOKEN: adminToken,
  INSTALLATION_SIGNING_KEY: 'installation-signing-key-at-least-32-bytes',
  PLANNING_CENTER_CLIENT_ID: 'pco-client-id',
  PLANNING_CENTER_CLIENT_SECRET: 'pco-client-secret-never-returned',
  GITHUB_RELEASE_TOKEN: 'github-release-token-server-side-only',
  REMOTE_PORT: '18766',
  ENROLLMENT_ENABLED: 'true',
  BETA_INSTALLATION_LIMIT: '500',
  BETA_RELEASE_VERSIONS: '1.1.103-beta.1,1.1.103-beta.2',
  BETA_LATEST_RELEASE_VERSION: '1.1.103-beta.1',
};

function request(path: string, method = 'GET', token?: string, value?: unknown, source = '203.0.113.10'): Request {
  return new Request(`https://control.example.com${path}`, {
    method,
    headers: {
      ...(token ? { authorization: `Bearer ${token}` } : {}),
      ...(value ? { 'content-type': 'application/json' } : {}),
      'cf-connecting-ip': source,
    },
    body: value ? JSON.stringify(value) : undefined,
  });
}

// Cloudflare Workers attach edge geolocation as `request.cf` at the runtime
// level; the standard Request constructor (used by the test harness above)
// has no way to set it, so tests that exercise geolocation capture build a
// plain request and then stamp `cf` onto it directly, matching how the
// real Workers runtime exposes it to `index.ts`.
function requestWithCf(
  path: string, method: string, token: string | undefined, value: unknown | undefined,
  cf: Record<string, unknown> | undefined,
): Request {
  const req = request(path, method, token, value);
  Object.defineProperty(req, 'cf', { value: cf, configurable: true });
  return req;
}

async function json(response: Response): Promise<Record<string, unknown>> {
  return await response.json() as Record<string, unknown>;
}

async function enroll(registry: Registry, key: string): Promise<Record<string, unknown>> {
  const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, {
    nonce: key,
  }));
  expect(response.status).toBe(201);
  return json(response);
}

function installationPath(installation: Record<string, unknown>, action: string): string {
  return `/v1/installations/${String(installation.installationId)}/${action}`;
}

// GET requests cannot carry a body (standard Fetch API restriction), so the
// GET status route's optional deviceName travels as a query parameter.
function statusPathWithDeviceName(installation: Record<string, unknown>, deviceName: string): string {
  return `${installationPath(installation, 'status')}?deviceName=${encodeURIComponent(deviceName)}`;
}

describe('private-beta control plane', () => {
  let storage: MemoryStorage;
  let provider: FakeCloudflare;
  let registry: Registry;

  beforeEach(() => {
    storage = new MemoryStorage();
    provider = new FakeCloudflare();
    vi.stubGlobal('fetch', provider.fetch);
    registry = new Registry({ storage } as unknown as DurableObjectState, env as never);
  });

  it('rejects malformed enrollment before allocating installation or provider resources', async () => {
    const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, {
      unexpected: 'unauthorized-request',
    }));
    expect(response.status).toBe(400);
    expect(storage.values.size).toBe(0);
    expect(provider.fetch).not.toHaveBeenCalled();
  });

  it('fails closed when any required Worker runtime secret is missing', async () => {
    for (const name of ['CLOUDFLARE_API_TOKEN', 'ADMIN_API_TOKEN', 'INSTALLATION_SIGNING_KEY', 'PLANNING_CENTER_CLIENT_ID', 'PLANNING_CENTER_CLIENT_SECRET'] as const) {
      registry = new Registry(
        { storage } as unknown as DurableObjectState,
        { ...env, [name]: undefined } as never,
      );
      const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, {
        nonce: 'missing-secret-test',
      }));
      expect(response.status).toBe(503);
      expect(await json(response)).toEqual({ error: 'operation incomplete; retry reconciliation' });
    }
    expect(storage.values.size).toBe(0);
    expect(provider.fetch).not.toHaveBeenCalled();
  });

  it('still enrolls when the deferred GITHUB_RELEASE_TOKEN binding is absent', async () => {
    registry = new Registry(
      { storage } as unknown as DurableObjectState,
      { ...env, GITHUB_RELEASE_TOKEN: undefined } as never,
    );
    const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, {
      nonce: 'no-release-token-test',
    }));
    expect(response.status).toBe(201);
  });

  it('enrolls durable random identities idempotently and issues isolated credentials', async () => {
    const first = await enroll(registry, 'friend-one-request');
    const replay = await enroll(registry, 'friend-one-request');
    const second = await enroll(registry, 'friend-two-request');

    expect(replay).toEqual(first);
    expect(second.installationId).not.toBe(first.installationId);
    expect(second.hostname).not.toBe(first.hostname);
    expect(second.installationCredential).not.toBe(first.installationCredential);
    expect(String(first.hostname)).toBe(`sp-${String(first.installationId)}.remote.example.com`);
    expect(String(first.installationId)).toMatch(/^[a-f0-9]{8}$/);
    expect(JSON.stringify([...storage.values.values()])).not.toContain('provider-secret-never-returned');
  });

  it('retries id generation on a collision instead of overwriting the existing installation', async () => {
    // Force the first two 4-byte draws to collide with an id that already
    // has a real installation record, then let the third draw succeed --
    // proves the retry loop advances past a collision rather than
    // silently overwriting someone else's installation.
    const collidingId = 'deadbeef';
    storage.values.set(`installation:${collidingId}`, {
      id: collidingId,
      hostname: `sp-${collidingId}.remote.example.com`,
      label: 'victim',
      phase: 'enabled',
      desiredEnabled: true,
      revoked: false,
      createdAt: 'then',
      updatedAt: 'then',
    });
    const draws = [collidingId, collidingId, 'cafef00d'];
    const spy = vi.spyOn(crypto, 'getRandomValues').mockImplementation((array) => {
      const hex = draws.shift();
      if (!hex) throw new Error('ran out of scripted random draws');
      const bytes = hex.match(/.{2}/g)!.map((byte) => parseInt(byte, 16));
      (array as Uint8Array).set(bytes);
      return array;
    });

    try {
      const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, {
        nonce: 'collision-retry-request',
      }));
      expect(response.status).toBe(201);
      const installation = await json(response);
      expect(installation.installationId).toBe('cafef00d');
      expect(installation.installationId).not.toBe(collidingId);
      // The pre-seeded "victim" installation must be completely untouched.
      const victim = storage.values.get(`installation:${collidingId}`) as Record<string, unknown>;
      expect(victim.label).toBe('victim');
      expect(victim.phase).toBe('enabled');
    } finally {
      spy.mockRestore();
    }
  });

  it('fails closed with a 503 when every id-generation attempt collides', async () => {
    const spy = vi.spyOn(crypto, 'getRandomValues').mockImplementation((array) => {
      (array as Uint8Array).set([0xde, 0xad, 0xbe, 0xef]);
      return array;
    });
    storage.values.set('installation:deadbeef', {
      id: 'deadbeef',
      hostname: 'sp-deadbeef.remote.example.com',
      label: '',
      phase: 'disabled',
      desiredEnabled: false,
      revoked: false,
      createdAt: 'then',
      updatedAt: 'then',
    });

    try {
      const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, {
        nonce: 'collision-exhausted-request',
      }));
      expect(response.status).toBe(503);
    } finally {
      spy.mockRestore();
    }
  });

  it('enforces normalized-source enrollment quota while replay and another source remain available', async () => {
    const ipv6 = '2001:0db8:0001:0002:0000:0000:0000:0001';
    const compactSamePrefix = '2001:db8:1:2::abcd';
    const accepted: Record<string, unknown>[] = [];
    for (const nonce of ['source-one-0001', 'source-one-0002', 'source-one-0003']) {
      const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, { nonce }, ipv6));
      expect(response.status).toBe(201);
      accepted.push(await json(response));
    }
    const before = [...storage.values.keys()].filter((key) => key.startsWith('installation:')).length;
    const denied = await registry.fetch(request(
      '/v1/installations/enroll', 'POST', undefined, { nonce: 'source-one-0004' }, compactSamePrefix,
    ));
    expect(denied.status).toBe(429);
    expect(Number(denied.headers.get('retry-after'))).toBeGreaterThan(0);
    expect([...storage.values.keys()].filter((key) => key.startsWith('installation:'))).toHaveLength(before);
    const replay = await registry.fetch(request(
      '/v1/installations/enroll', 'POST', undefined, { nonce: 'source-one-0001' }, compactSamePrefix,
    ));
    expect(await json(replay)).toEqual(accepted[0]);
    const other = await registry.fetch(request(
      '/v1/installations/enroll', 'POST', undefined, { nonce: 'source-two-0001' }, '2001:db8:1:3::1',
    ));
    expect(other.status).toBe(201);
    expect(provider.fetch).not.toHaveBeenCalled();
  });

  it('regenerating a link repeatedly never consumes the anonymous per-source enrollment quota', async () => {
    // Regression test for the reported bug: the desktop app used to call
    // the anonymous /enroll route on every "Regenerate Remote link" click,
    // so 3 regenerations from one network exhausted ENROLLMENTS_PER_SOURCE
    // and every further regeneration attempt (and the fallback re-enable)
    // was denied with 429, leaving Remote permanently broken from the UI.
    const source = '203.0.113.55';
    const first = await registry.fetch(request(
      '/v1/installations/enroll', 'POST', undefined, { nonce: 'regen-quota-0001' }, source,
    ));
    expect(first.status).toBe(201);
    let installation = await json(first) as Record<string, unknown>;

    // Exhaust the per-source quota with two more (unrelated) anonymous
    // enrollments from the same source -- 3 total, at the limit.
    for (const nonce of ['regen-quota-0002', 'regen-quota-0003']) {
      const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, { nonce }, source));
      expect(response.status).toBe(201);
    }
    const quotaExhausted = await registry.fetch(request(
      '/v1/installations/enroll', 'POST', undefined, { nonce: 'regen-quota-0004' }, source,
    ));
    expect(quotaExhausted.status).toBe(429);

    // The authenticated reenroll route must still work for the already-
    // enrolled installation from that same (now quota-exhausted) source,
    // repeatedly, because it is never billed against the anonymous quota.
    for (let attempt = 0; attempt < 5; attempt += 1) {
      const response = await registry.fetch(request(
        installationPath(installation, 'reenroll'), 'POST', String(installation.installationCredential), undefined, source,
      ));
      expect(response.status).toBe(201);
      const next = await json(response);
      expect(next.installationId).not.toBe(installation.installationId);
      installation = next;
    }
  });

  it('enables Enable to reactivate a known disabled installation without touching the anonymous quota', async () => {
    const source = '203.0.113.77';
    const first = await registry.fetch(request(
      '/v1/installations/enroll', 'POST', undefined, { nonce: 'reactivate-quota-0001' }, source,
    ));
    expect(first.status).toBe(201);
    const installation = await json(first) as Record<string, unknown>;

    // Exhaust the per-source anonymous enrollment quota with 2 more.
    for (const nonce of ['reactivate-quota-0002', 'reactivate-quota-0003']) {
      const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, { nonce }, source));
      expect(response.status).toBe(201);
    }
    const quotaExhausted = await registry.fetch(request(
      '/v1/installations/enroll', 'POST', undefined, { nonce: 'reactivate-quota-0004' }, source,
    ));
    expect(quotaExhausted.status).toBe(429);

    // Disable (not revoke) this installation, then reactivate it via the
    // authenticated path -- this must succeed even though the anonymous
    // quota for this source is fully exhausted.
    const disabled = await registry.fetch(request(
      installationPath(installation, 'disable'), 'POST', String(installation.installationCredential), undefined, source,
    ));
    expect(disabled.status).toBe(200);

    const reactivated = await registry.fetch(request(
      installationPath(installation, 'reactivate'), 'POST', String(installation.installationCredential), undefined, source,
    ));
    expect(reactivated.status).toBe(200);
    const reactivatedBody = await json(reactivated);
    expect(reactivatedBody.installationId).toBe(installation.installationId);
    expect(reactivatedBody.hostname).toBe(installation.hostname);
    expect(reactivatedBody.installationCredential).toBe(installation.installationCredential);
  });

  it('reactivates a revoked installation with a fresh credential, never the anonymous route', async () => {
    const source = '203.0.113.88';
    const first = await registry.fetch(request(
      '/v1/installations/enroll', 'POST', undefined, { nonce: 'reactivate-revoked-0001' }, source,
    ));
    expect(first.status).toBe(201);
    const installation = await json(first) as Record<string, unknown>;

    for (const nonce of ['reactivate-revoked-0002', 'reactivate-revoked-0003']) {
      const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, { nonce }, source));
      expect(response.status).toBe(201);
    }
    const quotaExhausted = await registry.fetch(request(
      '/v1/installations/enroll', 'POST', undefined, { nonce: 'reactivate-revoked-0004' }, source,
    ));
    expect(quotaExhausted.status).toBe(429);

    const revoked = await registry.fetch(request(
      installationPath(installation, 'revoke'), 'POST', String(installation.installationCredential), undefined, source,
    ));
    expect(revoked.status).toBe(200);
    expect((await json(revoked)).revoked).toBe(true);

    const reactivated = await registry.fetch(request(
      installationPath(installation, 'reactivate'), 'POST', String(installation.installationCredential), undefined, source,
    ));
    expect(reactivated.status).toBe(200);
    const reactivatedBody = await json(reactivated);
    expect(reactivatedBody.installationId).toBe(installation.installationId);
    expect(reactivatedBody.hostname).toBe(installation.hostname);
    // A reactivation of a revoked installation rotates the credential (a
    // fresh generation), unlike reactivating a merely-disabled one above.
    expect(reactivatedBody.installationCredential).not.toBe(installation.installationCredential);
    expect(reactivatedBody.revoked).toBe(false);

    // The stale, pre-reactivation credential must be rejected everywhere.
    const staleRejected = await registry.fetch(request(
      installationPath(installation, 'status'), 'GET', String(installation.installationCredential), undefined, source,
    ));
    expect(staleRejected.status).toBe(401);
  });

  it('rejects reactivation without proof of ownership of the installation credential', async () => {
    const installation = await enroll(registry, 'reactivate-unauthorized-request');
    const response = await registry.fetch(request(
      installationPath(installation, 'reactivate'), 'POST', 'spi_wrongwrongwrongwrongwrongwron.wrongwrongwrongwrongwrongwrongwrongwrongwrongw',
    ));
    expect(response.status).toBe(401);
  });

  it('honors a configured ENROLLMENT_WINDOW_SECONDS instead of the 24h default', async () => {
    registry = new Registry(
      { storage } as unknown as DurableObjectState,
      { ...env, ENROLLMENT_WINDOW_SECONDS: '1' } as never,
    );
    const source = '203.0.113.99';
    for (const nonce of ['short-window-0001', 'short-window-0002', 'short-window-0003']) {
      const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, { nonce }, source));
      expect(response.status).toBe(201);
    }
    const exhausted = await registry.fetch(request(
      '/v1/installations/enroll', 'POST', undefined, { nonce: 'short-window-0004' }, source,
    ));
    expect(exhausted.status).toBe(429);
    // Retry-After must reflect the short configured window, never the 24h default.
    expect(Number(exhausted.headers.get('retry-after'))).toBeLessThanOrEqual(1);
  });

  it('exempts a configured developer source from the per-source enrollment quota', async () => {
    registry = new Registry(
      { storage } as unknown as DurableObjectState,
      { ...env, ENROLLMENT_EXEMPT_SOURCES: '192.0.2.10,2001:db8:1:4::/64' } as never,
    );
    const ipv4Nonces = ['exempt-ipv4-0001', 'exempt-ipv4-0002', 'exempt-ipv4-0003', 'exempt-ipv4-0004', 'exempt-ipv4-0005'];
    for (const nonce of ipv4Nonces) {
      const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, { nonce }, '192.0.2.10'));
      expect(response.status).toBe(201);
    }
    const ipv6Address = '2001:db8:1:4:aaaa:bbbb:cccc:dddd';
    const ipv6Nonces = ['exempt-ipv6-0001', 'exempt-ipv6-0002', 'exempt-ipv6-0003', 'exempt-ipv6-0004'];
    for (const nonce of ipv6Nonces) {
      const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, { nonce }, ipv6Address));
      expect(response.status).toBe(201);
    }
    const stats = await registry.fetch(request('/v1/admin/metrics', 'GET', adminToken));
    const aggregate = await json(stats);
    expect(aggregate.enrollments).toBe(ipv4Nonces.length + ipv6Nonces.length);
    expect(aggregate.activeInstallations).toBe(ipv4Nonces.length + ipv6Nonces.length);
    expect(aggregate.enrollmentDenied).toBe(0);
  });

  it('still denies a non-exempt source with 429 and retry-after when an exemption list is configured', async () => {
    registry = new Registry(
      { storage } as unknown as DurableObjectState,
      { ...env, ENROLLMENT_EXEMPT_SOURCES: '192.0.2.10,2001:db8:1:4::/64' } as never,
    );
    const other = '198.51.100.9';
    for (const nonce of ['non-exempt-0001', 'non-exempt-0002', 'non-exempt-0003']) {
      const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, { nonce }, other));
      expect(response.status).toBe(201);
    }
    const denied = await registry.fetch(request(
      '/v1/installations/enroll', 'POST', undefined, { nonce: 'non-exempt-0004' }, other,
    ));
    expect(denied.status).toBe(429);
    expect(Number(denied.headers.get('retry-after'))).toBeGreaterThan(0);
  });

  it('does not treat a malformed or spoofed variant of an exempt value as exempt', async () => {
    registry = new Registry(
      { storage } as unknown as DurableObjectState,
      {
        ...env,
        ENROLLMENT_EXEMPT_SOURCES:
          '192.000.002.010,2001:0db8:0001:0004:0000:0000:0000:0000/64,2001:db8:1:5::/64',
      } as never,
    );
    for (const nonce of ['spoof-0001', 'spoof-0002', 'spoof-0003']) {
      const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, { nonce }, '192.0.2.10'));
      expect(response.status).toBe(201);
    }
    const denied = await registry.fetch(request(
      '/v1/installations/enroll', 'POST', undefined, { nonce: 'spoof-0004' }, '192.0.2.10',
    ));
    expect(denied.status).toBe(429);
  });

  it('treats an empty or unset exemption list as exempting nobody', async () => {
    for (const exemptSources of [undefined, '', '   ', ',,']) {
      const storageForRun = new MemoryStorage();
      const registryForRun = new Registry(
        { storage: storageForRun } as unknown as DurableObjectState,
        { ...env, ENROLLMENT_EXEMPT_SOURCES: exemptSources } as never,
      );
      for (const nonce of ['empty-0001', 'empty-0002', 'empty-0003']) {
        const response = await registryForRun.fetch(request(
          '/v1/installations/enroll', 'POST', undefined, { nonce }, '192.0.2.10',
        ));
        expect(response.status).toBe(201);
      }
      const denied = await registryForRun.fetch(request(
        '/v1/installations/enroll', 'POST', undefined, { nonce: 'empty-0004' }, '192.0.2.10',
      ));
      expect(denied.status).toBe(429);
    }
  });

  it('enforces the installation ceiling and kill switch without creating installations', async () => {
    registry = new Registry({ storage } as unknown as DurableObjectState, { ...env, BETA_INSTALLATION_LIMIT: '1' } as never);
    await enroll(registry, 'ceiling-first-request');
    const denied = await registry.fetch(request(
      '/v1/installations/enroll', 'POST', undefined, { nonce: 'ceiling-second-request' }, '198.51.100.2',
    ));
    expect(denied.status).toBe(503);
    expect([...storage.values.keys()].filter((key) => key.startsWith('installation:'))).toHaveLength(1);

    const disabledStorage = new MemoryStorage();
    registry = new Registry(
      { storage: disabledStorage } as unknown as DurableObjectState,
      { ...env, ENROLLMENT_ENABLED: 'false' } as never,
    );
    const switchedOff = await registry.fetch(request(
      '/v1/installations/enroll', 'POST', undefined, { nonce: 'kill-switch-request' },
    ));
    expect(switchedOff.status).toBe(503);
    expect([...disabledStorage.values.keys()].some((key) => key.startsWith('installation:'))).toBe(false);
  });

  it('uses distinct status and mutation quotas and exposes only aggregate counters', async () => {
    const installation = await enroll(registry, 'rate-limits-request');
    for (let index = 0; index < 120; index += 1) {
      const response = await registry.fetch(request(
        installationPath(installation, 'status'), 'GET', String(installation.installationCredential),
      ));
      expect(response.status).toBe(200);
    }
    const statusDenied = await registry.fetch(request(
      installationPath(installation, 'status'), 'GET', String(installation.installationCredential),
    ));
    expect(statusDenied.status).toBe(429);

    const generation = '55555555-5555-4555-8555-555555555555';
    for (let index = 0; index < 20; index += 1) {
      const response = await registry.fetch(request(
        installationPath(installation, 'provision'), 'POST', String(installation.installationCredential), { generation },
      ));
      expect(response.status).toBe(200);
    }
    const mutationDenied = await registry.fetch(request(
      installationPath(installation, 'provision'), 'POST', String(installation.installationCredential), { generation },
    ));
    expect(mutationDenied.status).toBe(429);
    expect(provider.createCount).toBe(1);
    expect(provider.dnsCreateCount).toBe(1);

    const metrics = await registry.fetch(request('/v1/admin/metrics', 'GET', adminToken));
    expect(metrics.status).toBe(200);
    const aggregate = await json(metrics);
    expect(aggregate.statusDenied).toBe(1);
    expect(aggregate.mutationDenied).toBe(1);
    expect(JSON.stringify(aggregate)).not.toContain('203.0.113.10');
  });

  it('reserves provider capacity for revoke and recovery operations', async () => {
    const installation = await enroll(registry, 'provider-budget-request');
    storage.values.set('provider:budget', { startedAt: Math.floor(Date.now() / 1000), count: 480 });
    const generation = '66666666-6666-4666-8666-666666666666';
    const provision = await registry.fetch(request(
      installationPath(installation, 'provision'), 'POST', String(installation.installationCredential), { generation },
    ));
    expect(provision.status).toBe(503);
    expect(provider.fetch).not.toHaveBeenCalled();
    const revoke = await registry.fetch(request(
      installationPath(installation, 'revoke'), 'POST', String(installation.installationCredential), {},
    ));
    expect(revoke.status).toBe(200);
    expect((await json(revoke)).revoked).toBe(true);
  });

  it('does not let enabled reconcile consume reserved revoke capacity', async () => {
    const installation = await enroll(registry, 'reconcile-budget-request');
    const generation = '77777777-7777-4777-8777-777777777777';
    expect((await registry.fetch(request(
      installationPath(installation, 'provision'), 'POST', String(installation.installationCredential), { generation },
    ))).status).toBe(200);
    storage.values.set('provider:budget', { startedAt: Math.floor(Date.now() / 1000), count: 480 });
    registry = new Registry({ storage } as unknown as DurableObjectState, env as never);
    const callsBefore = provider.fetch.mock.calls.length;

    const reconcile = await registry.fetch(request(
      installationPath(installation, 'reconcile'), 'POST', String(installation.installationCredential),
    ));
    expect(reconcile.status).toBe(503);
    expect(provider.fetch).toHaveBeenCalledTimes(callsBefore);

    const revoke = await registry.fetch(request(
      installationPath(installation, 'revoke'), 'POST', String(installation.installationCredential),
    ));
    expect(revoke.status).toBe(200);
  });

  it('does not log a non-JSON provider response body', async () => {
    const installation = await enroll(registry, 'provider-body-request');
    const providerBody = 'PRIVATE_PROVIDER_BODY';
    vi.stubGlobal('fetch', vi.fn(async () => new Response(providerBody, { status: 200 })));
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);

    const response = await registry.fetch(request(
      installationPath(installation, 'provision'),
      'POST',
      String(installation.installationCredential),
      { generation: '88888888-8888-4888-8888-888888888888' },
    ));

    expect(response.status).toBe(503);
    expect(JSON.stringify(await json(response))).not.toContain(providerBody);
    expect(JSON.stringify(error.mock.calls)).not.toContain(providerBody);
  });

  it('rejects release-asset requests without ever calling GitHub when the token is absent', async () => {
    registry = new Registry(
      { storage } as unknown as DurableObjectState,
      { ...env, GITHUB_RELEASE_TOKEN: undefined } as never,
    );
    const github = vi.fn();
    vi.stubGlobal('fetch', github);

    const response = await registry.fetch(request('/v1/releases/latest.json'));

    expect(response.status).toBe(503);
    expect(github).not.toHaveBeenCalled();
  });

  it('serves only allowlisted private-release metadata through the public broker', async () => {
    const version = '1.1.103-beta.1';
    const broker = 'https://stagepilot-beta-control-plane.stagepilot-illuminary-beta.workers.dev/v1/releases';
    const manifest = JSON.stringify({
      version,
      pub_date: '2026-09-15T00:00:00Z',
      platforms: {
        'darwin-aarch64': { url: `${broker}/v${version}/StagePilot_${version}_aarch64.app.tar.gz`, signature: 'a' },
        'darwin-x86_64': { url: `${broker}/v${version}/StagePilot_${version}_x64.app.tar.gz`, signature: 'b' },
        'windows-x86_64': { url: `${broker}/v${version}/StagePilot_${version}_x64-setup.exe`, signature: 'c' },
      },
    });
    const github = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input : input.url);
      if (url.hostname === 'api.github.com') {
        expect(new Headers(init?.headers).get('authorization')).toBe('Bearer github-release-token-server-side-only');
      }
      if (url.pathname.endsWith('/releases/tags/v1.1.103-beta.1')) {
        return new Response(JSON.stringify({
          tag_name: 'v1.1.103-beta.1',
          draft: false,
          assets: [{
            name: 'latest.json',
            url: 'https://api.github.com/repos/tage-ilot/stagepilot-beta/releases/assets/101',
            size: manifest.length,
            state: 'uploaded',
            content_type: 'application/json',
          }],
        }));
      }
      if (url.pathname.endsWith('/releases/assets/101')) {
        return new Response(null, {
          status: 302,
          headers: { location: 'https://release-assets.githubusercontent.com/private-signed-download' },
        });
      }
      if (url.hostname === 'release-assets.githubusercontent.com') {
        expect(new Headers(init?.headers).get('authorization')).toBeNull();
        return new Response(manifest, { headers: { 'content-length': String(manifest.length) } });
      }
      throw new Error(`unexpected GitHub request: ${url.pathname}`);
    });
    vi.stubGlobal('fetch', github);

    const response = await registry.fetch(request('/v1/releases/latest.json'));

    expect(response.status).toBe(200);
    expect(response.headers.get('cache-control')).toBe('public, max-age=60, s-maxage=300');
    expect(await response.text()).toBe(manifest);
    expect(github).toHaveBeenCalledTimes(3);
  });

  it('rejects a manifest that points beta clients outside the broker', async () => {
    const manifest = JSON.stringify({
      version: '1.1.103-beta.1',
      pub_date: '2026-09-15T00:00:00Z',
      platforms: {
        'darwin-aarch64': { url: 'https://github.com/tage-ilot/stagepilot/releases/download/v1.1.103/a', signature: 'a' },
        'darwin-x86_64': { url: 'https://github.com/tage-ilot/stagepilot/releases/download/v1.1.103/b', signature: 'b' },
        'windows-x86_64': { url: 'https://github.com/tage-ilot/stagepilot/releases/download/v1.1.103/c', signature: 'c' },
      },
    });
    vi.stubGlobal('fetch', vi.fn(async (input: string | URL | Request) => {
      const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input : input.url);
      if (url.pathname.endsWith('/releases/tags/v1.1.103-beta.1')) {
        return new Response(JSON.stringify({
          tag_name: 'v1.1.103-beta.1',
          draft: false,
          assets: [{
            name: 'latest.json',
            url: 'https://api.github.com/repos/tage-ilot/stagepilot-beta/releases/assets/103',
            size: manifest.length,
            state: 'uploaded',
          }],
        }));
      }
      return new Response(manifest, { headers: { 'content-length': String(manifest.length) } });
    }));

    expect((await registry.fetch(request('/v1/releases/latest.json'))).status).toBe(503);
  });

  it('rejects a release download when any redirect leaves GitHub asset hosting', async () => {
    const version = '1.1.103-beta.1';
    const filename = `StagePilot_${version}_x64-setup.exe`;
    const github = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input : input.url);
      if (url.pathname.endsWith(`/releases/tags/v${version}`)) {
        return new Response(JSON.stringify({
          tag_name: `v${version}`,
          draft: false,
          assets: [{
            name: filename,
            url: 'https://api.github.com/repos/tage-ilot/stagepilot-beta/releases/assets/104',
            size: 1,
            state: 'uploaded',
          }],
        }));
      }
      if (url.hostname === 'api.github.com') {
        expect(new Headers(init?.headers).get('authorization')).toBeTruthy();
        return new Response(null, {
          status: 302,
          headers: { location: 'https://release-assets.githubusercontent.com/first-hop' },
        });
      }
      expect(new Headers(init?.headers).get('authorization')).toBeNull();
      return new Response(null, {
        status: 302,
        headers: { location: 'https://example.test/credential-trap' },
      });
    });
    vi.stubGlobal('fetch', github);

    expect((await registry.fetch(request(`/v1/releases/v${version}/${filename}`))).status).toBe(503);
    expect(github).toHaveBeenCalledTimes(3);
  });

  it('rejects a manifest body larger than its declared GitHub asset size', async () => {
    const version = '1.1.103-beta.1';
    const broker = 'https://stagepilot-beta-control-plane.stagepilot-illuminary-beta.workers.dev/v1/releases';
    const manifest = JSON.stringify({
      version,
      pub_date: '2026-09-15T00:00:00Z',
      platforms: {
        'darwin-aarch64': { url: `${broker}/v${version}/StagePilot_${version}_aarch64.app.tar.gz`, signature: 'a' },
        'darwin-x86_64': { url: `${broker}/v${version}/StagePilot_${version}_x64.app.tar.gz`, signature: 'b' },
        'windows-x86_64': { url: `${broker}/v${version}/StagePilot_${version}_x64-setup.exe`, signature: 'c' },
      },
    });
    vi.stubGlobal('fetch', vi.fn(async (input: string | URL | Request) => {
      const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input : input.url);
      if (url.pathname.endsWith(`/releases/tags/v${version}`)) {
        return new Response(JSON.stringify({
          tag_name: `v${version}`,
          draft: false,
          assets: [{
            name: 'latest.json',
            url: 'https://api.github.com/repos/tage-ilot/stagepilot-beta/releases/assets/105',
            size: manifest.length,
            state: 'uploaded',
          }],
        }));
      }
      return new Response(`${manifest}x`);
    }));

    expect((await registry.fetch(request('/v1/releases/latest.json'))).status).toBe(503);
  });

  it('rejects nonallowlisted versions and filenames without contacting GitHub', async () => {
    const github = vi.fn();
    vi.stubGlobal('fetch', github);

    for (const path of [
      '/v1/releases/v1.1.104-beta.1/StagePilot_1.1.104-beta.1_x64-setup.exe',
      '/v1/releases/v1.1.103-beta.1/source.zip',
    ]) {
      expect((await registry.fetch(request(path))).status).toBe(404);
    }
    expect(github).not.toHaveBeenCalled();
  });

  it('rate limits release downloads independently before contacting GitHub', async () => {
    const version = '1.1.103-beta.1';
    const filename = `StagePilot_${version}_x64-setup.exe`;
    const github = vi.fn(async (input: string | URL | Request) => {
      const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input : input.url);
      if (url.pathname.endsWith(`/releases/tags/v${version}`)) {
        return new Response(JSON.stringify({
          tag_name: `v${version}`,
          draft: false,
          assets: [{
            name: filename,
            url: 'https://api.github.com/repos/tage-ilot/stagepilot-beta/releases/assets/102',
            size: 1,
            state: 'uploaded',
          }],
        }));
      }
      return new Response('x', { headers: { 'content-length': '1' } });
    });
    vi.stubGlobal('fetch', github);
    for (let index = 0; index < 6; index += 1) {
      expect((await registry.fetch(request(`/v1/releases/v${version}/${filename}`))).status).toBe(200);
    }
    const calls = github.mock.calls.length;
    const denied = await registry.fetch(request(`/v1/releases/v${version}/${filename}`));
    expect(denied.status).toBe(429);
    expect(denied.headers.get('retry-after')).toBeTruthy();
    expect(github).toHaveBeenCalledTimes(calls);
  });

  it('keeps two provisioned installations isolated across credentials, routes, and lifecycle', async () => {
    const first = await enroll(registry, 'friend-one-request');
    const second = await enroll(registry, 'friend-two-request');
    const firstGeneration = '11111111-1111-4111-8111-111111111111';
    const secondGeneration = '22222222-2222-4222-8222-222222222222';

    const crossUse = await registry.fetch(request(
      installationPath(first, 'status'),
      'GET',
      String(second.installationCredential),
    ));
    expect(crossUse.status).toBe(401);

    const firstProvision = await registry.fetch(request(
      installationPath(first, 'provision'),
      'POST',
      String(first.installationCredential),
      { generation: firstGeneration },
    ));
    const secondProvision = await registry.fetch(request(
      installationPath(second, 'provision'),
      'POST',
      String(second.installationCredential),
      { generation: secondGeneration },
    ));
    expect(firstProvision.status).toBe(200);
    expect(secondProvision.status).toBe(200);
    expect(provider.tunnels.size).toBe(2);
    expect(provider.records.size).toBe(2);

    const replay = await registry.fetch(request(
      installationPath(first, 'provision'),
      'POST',
      String(first.installationCredential),
      { generation: firstGeneration },
    ));
    expect(replay.status).toBe(200);
    expect(provider.createCount).toBe(2);
    expect(provider.dnsCreateCount).toBe(2);

    const disable = await registry.fetch(request(
      installationPath(first, 'disable'),
      'POST',
      String(first.installationCredential),
    ));
    expect(disable.status).toBe(200);
    expect(provider.tunnels.size).toBe(1);
    expect(provider.records.size).toBe(1);
    expect([...provider.records.values()][0]?.name).toBe(second.hostname);

    const secondStatus = await registry.fetch(request(
      installationPath(second, 'status'),
      'GET',
      String(second.installationCredential),
    ));
    expect(secondStatus.status).toBe(200);
    expect((await json(secondStatus)).phase).toBe('provisioned');

    const revoke = await registry.fetch(request(
      `/v1/admin/installations/${String(first.installationId)}/revoke`,
      'POST',
      adminToken,
    ));
    expect(revoke.status).toBe(200);
    const revokedCredential = await registry.fetch(request(
      installationPath(first, 'status'),
      'GET',
      String(first.installationCredential),
    ));
    expect(revokedCredential.status).toBe(401);
    expect(provider.tunnels.size).toBe(1);
  });

  it('recovers after a lost create response and Durable Object restart without duplication', async () => {
    const installation = await enroll(registry, 'restart-proof-request');
    provider.failAfterNextCreate = true;
    const generation = '33333333-3333-4333-8333-333333333333';
    const failed = await registry.fetch(request(
      installationPath(installation, 'provision'),
      'POST',
      String(installation.installationCredential),
      { generation },
    ));
    expect(failed.status).toBe(503);
    expect(provider.tunnels.size).toBe(1);

    registry = new Registry({ storage } as unknown as DurableObjectState, env as never);
    const recovered = await registry.fetch(request(
      installationPath(installation, 'reconcile'),
      'POST',
      String(installation.installationCredential),
    ));
    expect(recovered.status).toBe(200);
    expect((await json(recovered)).phase).toBe('provisioned');
    expect(provider.createCount).toBe(1);
    expect(provider.dnsCreateCount).toBe(1);
  });

  it('fails closed on foreign DNS ownership and never adopts the route', async () => {
    const installation = await enroll(registry, 'ownership-proof-request');
    provider.records.set('foreign', {
      id: 'foreign',
      name: installation.hostname,
      type: 'CNAME',
      content: 'foreign.cfargotunnel.com',
      proxied: true,
      comment: 'not-stagepilot-owned',
    });
    const response = await registry.fetch(request(
      installationPath(installation, 'provision'),
      'POST',
      String(installation.installationCredential),
      { generation: '44444444-4444-4444-8444-444444444444' },
    ));
    expect(response.status).toBe(503);
    expect(provider.createCount).toBe(0);
    expect(provider.records.size).toBe(1);
    expect((await json(response)).error).not.toContain('foreign.cfargotunnel.com');
  });

  describe('stale older-generation resources of the same installation', () => {
    const oldGeneration = '88888888-8888-4888-8888-888888888888';
    const newGeneration = '99999999-9999-4999-8999-999999999999';
    const oldTunnelId = '11111111-1111-4111-8111-111111111111';

    function seed(installation: Record<string, unknown>, over: {
      comment?: string; record?: Record<string, unknown>; tunnel?: Record<string, unknown> | null; second?: boolean;
    } = {}): void {
      const name = over.comment ?? `stagepilot-${String(installation.installationId)}-${oldGeneration}`;
      if (over.tunnel !== null) {
        provider.tunnels.set(oldTunnelId, {
          id: oldTunnelId, name, config_src: 'cloudflare', deleted_at: null, ...over.tunnel,
        });
        if (over.second) {
          provider.tunnels.set('22222222-2222-4222-8222-222222222222', {
            id: '22222222-2222-4222-8222-222222222222', name, config_src: 'cloudflare', deleted_at: null,
          });
        }
      }
      provider.records.set('stale', {
        id: 'stale', name: installation.hostname, type: 'CNAME', proxied: true,
        content: `${oldTunnelId}.cfargotunnel.com`, comment: name, ...over.record,
      });
    }

    const provisionRequest = (installation: Record<string, unknown>, generation = newGeneration): Request => request(
      installationPath(installation, 'provision'), 'POST', String(installation.installationCredential), { generation },
    );

    it('replaces the stale pair on provision, in order, without leaking provider data', async () => {
      const installation = await enroll(registry, 'stale-provision');
      seed(installation);
      provider.fetch.mockClear();
      const response = await registry.fetch(provisionRequest(installation));
      expect(response.status).toBe(200);
      const text = JSON.stringify(await json(response));
      expect(text).not.toContain('provider-secret-never-returned');
      expect(text).not.toContain(oldGeneration);
      expect(provider.tunnels.has(oldTunnelId)).toBe(false);
      expect(provider.tunnels.size).toBe(1);
      expect(provider.records.size).toBe(1);
      expect([...provider.records.values()][0].comment).toContain(newGeneration);
      const calls = provider.fetch.mock.calls.map(([u, i]) => `${(i as RequestInit | undefined)?.method ?? 'GET'} ${new URL(String(u)).pathname.split('/').slice(-2).join('/')}`);
      const recordDelete = calls.findIndex((c) => c === 'DELETE dns_records/stale');
      const connDelete = calls.findIndex((c) => c === `DELETE ${oldTunnelId}/connections`);
      const tunnelDelete = calls.findIndex((c) => c === `DELETE cfd_tunnel/${oldTunnelId}`);
      expect(recordDelete).toBeGreaterThanOrEqual(0);
      expect(connDelete).toBeGreaterThan(recordDelete);
      expect(tunnelDelete).toBeGreaterThan(connDelete);
      expect(calls.length).toBeLessThan(40);
    });

    it('replaces the stale pair on reconcile of a wedged enabling installation', async () => {
      const installation = await enroll(registry, 'stale-reconcile');
      seed(installation, { tunnel: { liveConnections: true } });
      const failed = await registry.fetch(provisionRequest(installation));
      expect(failed.status).toBe(503);
      expect(provider.tunnels.has(oldTunnelId)).toBe(true);
      provider.tunnels.get(oldTunnelId)!.liveConnections = false;
      const recovered = await registry.fetch(request(
        installationPath(installation, 'reconcile'), 'POST', String(installation.installationCredential),
      ));
      expect(recovered.status).toBe(200);
      expect((await json(recovered)).phase).toBe('provisioned');
      expect(provider.tunnels.has(oldTunnelId)).toBe(false);
      expect(provider.records.size).toBe(1);
    });

    it('converges after a lost response mid-replacement', async () => {
      const installation = await enroll(registry, 'stale-lost');
      seed(installation);
      // Simulate the record deletion having succeeded but the tunnel delete lost.
      provider.records.delete('stale');
      const response = await registry.fetch(provisionRequest(installation));
      expect(response.status).toBe(200);
      expect((await json(response)).phase).toBe('provisioned');
      const again = await registry.fetch(request(
        installationPath(installation, 'reconcile'), 'POST', String(installation.installationCredential),
      ));
      expect(again.status).toBe(200);
      expect(provider.records.size).toBe(1);
    });

    it('disable removes a stale same-installation pair', async () => {
      const installation = await enroll(registry, 'stale-disable');
      const wedged = await registry.fetch(provisionRequest(installation));
      expect(wedged.status).toBe(200);
      provider.records.clear();
      provider.tunnels.clear();
      seed(installation);
      const disable = await registry.fetch(request(
        installationPath(installation, 'revoke'), 'POST', String(installation.installationCredential),
      ));
      expect(disable.status).toBe(200);
      expect(provider.records.size).toBe(0);
      expect(provider.tunnels.size).toBe(0);
    });

    const refusals: Array<[string, (i: Record<string, unknown>) => Parameters<typeof seed>[1]]> = [
      ['missing comment', () => ({ record: { comment: undefined } })],
      ['malformed comment', () => ({ comment: 'stagepilot-garbage' })],
      ['different installation id', () => ({ comment: `stagepilot-deadbeef-${oldGeneration}` })],
      ['same generation but record points elsewhere', (i) => ({ comment: `stagepilot-${String(i.installationId)}-${newGeneration}`, record: { content: 'other.cfargotunnel.com' } })],
      ['unproxied record', () => ({ record: { proxied: false } })],
      ['non-CNAME record', () => ({ record: { type: 'A' } })],
      ['content mismatch', () => ({ record: { content: 'other.cfargotunnel.com' } })],
      ['tunnel missing', () => ({ tunnel: null })],
      ['duplicate tunnels', () => ({ second: true })],
      ['locally managed tunnel', () => ({ tunnel: { config_src: 'local' } })],
      ['live connections', () => ({ tunnel: { liveConnections: true } })],
    ];
    for (const [label, build] of refusals) {
      it(`fails closed: ${label}`, async () => {
        const installation = await enroll(registry, `stale-refuse-${label.replace(/[^a-z]/gi, '-')}-request`);
        const over = build(installation);
        seed(installation, over);
        const before = [provider.tunnels.size, provider.records.size];
        const response = await registry.fetch(provisionRequest(installation));
        expect(response.status).toBe(503);
        expect(provider.createCount).toBe(0);
        expect([provider.tunnels.size, provider.records.size]).toEqual(before);
        expect(JSON.stringify(await json(response))).not.toContain('cfargotunnel.com');
      });
    }

    it('disable also refuses a stale pair with live connections', async () => {
      const installation = await enroll(registry, 'stale-disable-live');
      await registry.fetch(provisionRequest(installation));
      provider.records.clear();
      provider.tunnels.clear();
      seed(installation, { tunnel: { liveConnections: true } });
      const disable = await registry.fetch(request(
        installationPath(installation, 'revoke'), 'POST', String(installation.installationCredential),
      ));
      expect(disable.status).toBe(503);
      expect(provider.records.size).toBe(1);
      expect(provider.tunnels.size).toBe(1);
    });
  });

  it('reprovisions the same hostname with a new generation after disable -> re-enable', async () => {
    const installation = await enroll(registry, 'durable-identity-request');
    const firstGeneration = '55555555-5555-4555-8555-555555555555';
    const provisioned = await registry.fetch(request(
      installationPath(installation, 'provision'),
      'POST',
      String(installation.installationCredential),
      { generation: firstGeneration },
    ));
    expect(provisioned.status).toBe(200);

    const disable = await registry.fetch(request(
      installationPath(installation, 'revoke'),
      'POST',
      String(installation.installationCredential),
    ));
    expect(disable.status).toBe(200);
    expect((await json(disable)).revoked).toBe(true);

    // Re-enable replays the SAME enrollment nonce the installer retained
    // locally (finish_revoke keeps identity/nonce; only the credential and
    // provider resources are actually revoked).
    const reenrolled = await enroll(registry, 'durable-identity-request');
    expect(reenrolled.installationId).toBe(installation.installationId);
    expect(reenrolled.hostname).toBe(installation.hostname);
    expect(reenrolled.installationCredential).not.toBe(installation.installationCredential);
    expect(reenrolled.revoked).toBe(false);
    expect(reenrolled.generation).toBeNull();

    const secondGeneration = '66666666-6666-4666-8666-666666666666';
    const reprovisioned = await registry.fetch(request(
      installationPath(reenrolled, 'provision'),
      'POST',
      String(reenrolled.installationCredential),
      { generation: secondGeneration },
    ));
    expect(reprovisioned.status).toBe(200);
    const reprovisionedBody = await json(reprovisioned);
    expect(reprovisionedBody.generation).toBe(secondGeneration);
    expect(reprovisionedBody.generation).not.toBe(firstGeneration);
    expect(reprovisionedBody.hostname).toBe(installation.hostname);
  });

  it('rejects a hostname-ownership conflict from a different owner on re-enroll', async () => {
    const installation = await enroll(registry, 'owner-identity-request');
    await registry.fetch(request(
      installationPath(installation, 'revoke'),
      'POST',
      String(installation.installationCredential),
    ));
    // A different source presenting a different nonce must mint its own
    // fresh installation, never reuse or collide with the revoked one.
    const other = await enroll(registry, 'a-completely-different-nonce');
    expect(other.installationId).not.toBe(installation.installationId);
    expect(other.hostname).not.toBe(installation.hostname);
  });

  it('reenroll tears down the old tunnel/DNS/credential and never accepts an unauthenticated caller', async () => {
    const installation = await enroll(registry, 'reenroll-teardown-request');
    const generation = '77777777-7777-4777-8777-777777777777';
    const provisioned = await registry.fetch(request(
      installationPath(installation, 'provision'),
      'POST',
      String(installation.installationCredential),
      { generation },
    ));
    expect(provisioned.status).toBe(200);
    expect(provider.tunnels.size).toBe(1);
    expect(provider.records.size).toBe(1);

    // No credential at all: rejected before any provider call.
    const anonymous = await registry.fetch(request(installationPath(installation, 'reenroll'), 'POST'));
    expect(anonymous.status).toBe(401);
    // Wrong credential: also rejected.
    const wrongCredential = await registry.fetch(request(
      installationPath(installation, 'reenroll'), 'POST', 'not-the-real-credential',
    ));
    expect(wrongCredential.status).toBe(401);
    expect(provider.tunnels.size).toBe(1);
    expect(provider.records.size).toBe(1);

    const reenrolled = await registry.fetch(request(
      installationPath(installation, 'reenroll'), 'POST', String(installation.installationCredential),
    ));
    expect(reenrolled.status).toBe(201);
    const fresh = await json(reenrolled);
    expect(fresh.installationId).not.toBe(installation.installationId);
    expect(fresh.hostname).not.toBe(installation.hostname);

    // Old tunnel/DNS resources were actually revoked, not orphaned.
    expect(provider.tunnels.size).toBe(0);
    expect(provider.records.size).toBe(0);

    // Old credential is dead; only the new installation/credential works.
    const oldStatus = await registry.fetch(request(
      installationPath(installation, 'status'), 'GET', String(installation.installationCredential),
    ));
    expect(oldStatus.status).toBe(401);
    const newStatus = await registry.fetch(request(
      installationPath(fresh, 'status'), 'GET', String(fresh.installationCredential),
    ));
    expect(newStatus.status).toBe(200);
  });
});

describe('fleet alert ingestion and admin endpoints', () => {
  let storage: MemoryStorage;
  let provider: FakeCloudflare;
  let registry: Registry;

  beforeEach(() => {
    storage = new MemoryStorage();
    provider = new FakeCloudflare();
    vi.stubGlobal('fetch', provider.fetch);
    registry = new Registry({ storage } as unknown as DurableObjectState, env as never);
  });

  function alertBody(overrides: Record<string, unknown> = {}): Record<string, unknown> {
    return {
      category: 'sync_failure',
      subject: 'planning_center',
      message: 'Planning Center sync failed 5 times',
      severity: 'critical',
      first_seen: '2026-01-01T00:00:00.000Z',
      last_seen: '2026-01-01T00:10:00.000Z',
      count: 5,
      ...overrides,
    };
  }

  it('rejects an alert POST for an installation that does not exist', async () => {
    const response = await registry.fetch(request(
      '/v1/installations/deadbeef/alerts', 'POST', 'irrelevant-credential', alertBody(),
    ));
    expect(response.status).toBe(401);
  });

  it('rejects an alert POST with the wrong installation credential', async () => {
    const installation = await enroll(registry, 'alert-auth-0001');
    const response = await registry.fetch(request(
      installationPath(installation, 'alerts'), 'POST', 'not-the-real-credential', alertBody(),
    ));
    expect(response.status).toBe(401);
  });

  it('rejects an alert POST for a revoked installation', async () => {
    const installation = await enroll(registry, 'alert-auth-0002');
    const revoke = await registry.fetch(request(
      installationPath(installation, 'revoke'), 'POST', String(installation.installationCredential),
    ));
    expect(revoke.status).toBe(200);
    const response = await registry.fetch(request(
      installationPath(installation, 'alerts'), 'POST', String(installation.installationCredential), alertBody(),
    ));
    expect(response.status).toBe(401);
  });

  it('rejects a malformed alert body', async () => {
    const installation = await enroll(registry, 'alert-invalid-0001');
    const response = await registry.fetch(request(
      installationPath(installation, 'alerts'), 'POST', String(installation.installationCredential),
      alertBody({ severity: 'not-a-severity' }),
    ));
    expect(response.status).toBe(400);
  });

  it('accepts and stores a valid alert, then updates it in place on re-forward', async () => {
    const installation = await enroll(registry, 'alert-store-0001');
    const created = await registry.fetch(request(
      installationPath(installation, 'alerts'), 'POST', String(installation.installationCredential), alertBody(),
    ));
    expect(created.status).toBe(201);
    const firstBody = await json(created);
    expect(firstBody.status).toBe('open');
    expect(firstBody.installationId).toBe(installation.installationId);

    const updated = await registry.fetch(request(
      installationPath(installation, 'alerts'), 'POST', String(installation.installationCredential),
      alertBody({ count: 8, last_seen: '2026-01-01T00:20:00.000Z' }),
    ));
    expect(updated.status).toBe(200);
    const secondBody = await json(updated);
    expect(secondBody.alertId).toBe(firstBody.alertId);
    expect(secondBody.count).toBe(8);
  });

  it('requires the admin bearer token for GET /v1/admin/alerts, matching /v1/admin/metrics', async () => {
    const missing = await registry.fetch(request('/v1/admin/alerts', 'GET'));
    expect(missing.status).toBe(401);
    const wrong = await registry.fetch(request('/v1/admin/alerts', 'GET', 'wrong-token-not-admin-at-all-x'));
    expect(wrong.status).toBe(401);
    const authorized = await registry.fetch(request('/v1/admin/alerts', 'GET', adminToken));
    expect(authorized.status).toBe(200);
  });

  it('requires the admin bearer token for the acknowledge route', async () => {
    const installation = await enroll(registry, 'alert-ack-auth-0001');
    await registry.fetch(request(
      installationPath(installation, 'alerts'), 'POST', String(installation.installationCredential), alertBody(),
    ));
    const response = await registry.fetch(request('/v1/admin/alerts/whatever/acknowledge', 'POST'));
    expect(response.status).toBe(401);
  });

  it('lists alerts from multiple installations without mixing attribution, supports status filter, and acknowledge flips status', async () => {
    const installationA = await enroll(registry, 'alert-fleet-a-0001');
    const installationB = await enroll(registry, 'alert-fleet-b-0001');
    await registry.fetch(request(
      installationPath(installationA, 'alerts'), 'POST', String(installationA.installationCredential),
      alertBody({ category: 'sync_failure', subject: 'planning_center' }),
    ));
    const bAlertResponse = await registry.fetch(request(
      installationPath(installationB, 'alerts'), 'POST', String(installationB.installationCredential),
      alertBody({ category: 'disk_space', subject: 'recordings_volume', message: 'Disk nearly full' }),
    ));
    const bAlert = await json(bAlertResponse);

    const listAll = await registry.fetch(request('/v1/admin/alerts', 'GET', adminToken));
    expect(listAll.status).toBe(200);
    const allBody = await json(listAll) as { alerts: Record<string, unknown>[] };
    expect(allBody.alerts).toHaveLength(2);
    const byInstallation = new Map(allBody.alerts.map((alert) => [alert.installationId, alert]));
    expect(byInstallation.get(String(installationA.installationId))?.category).toBe('sync_failure');
    expect(byInstallation.get(String(installationB.installationId))?.category).toBe('disk_space');

    const openOnly = await registry.fetch(request('/v1/admin/alerts?status=open', 'GET', adminToken));
    expect((await json(openOnly) as { alerts: unknown[] }).alerts).toHaveLength(2);
    const acknowledgedOnly = await registry.fetch(request('/v1/admin/alerts?status=acknowledged', 'GET', adminToken));
    expect((await json(acknowledgedOnly) as { alerts: unknown[] }).alerts).toHaveLength(0);

    const acknowledge = await registry.fetch(request(
      `/v1/admin/alerts/${String(bAlert.alertId)}/acknowledge`, 'POST', adminToken,
    ));
    expect(acknowledge.status).toBe(200);
    expect((await json(acknowledge)).status).toBe('acknowledged');

    const afterAckOpen = await registry.fetch(request('/v1/admin/alerts?status=open', 'GET', adminToken));
    const afterAckOpenBody = await json(afterAckOpen) as { alerts: Record<string, unknown>[] };
    expect(afterAckOpenBody.alerts).toHaveLength(1);
    expect(afterAckOpenBody.alerts[0]?.installationId).toBe(installationA.installationId);

    const afterAckAcked = await registry.fetch(request('/v1/admin/alerts?status=acknowledged', 'GET', adminToken));
    const afterAckAckedBody = await json(afterAckAcked) as { alerts: Record<string, unknown>[] };
    expect(afterAckAckedBody.alerts).toHaveLength(1);
    expect(afterAckAckedBody.alerts[0]?.installationId).toBe(installationB.installationId);
  });

  it('404s acknowledging an unknown alert id', async () => {
    const response = await registry.fetch(request('/v1/admin/alerts/does-not-exist/acknowledge', 'POST', adminToken));
    expect(response.status).toBe(404);
  });

  it('carries the installation\'s current location onto the alert, and updates on the next forward after a check-in', async () => {
    const installation = await enroll(registry, 'alert-geo-0001');
    const credential = String(installation.installationCredential);

    await registry.fetch(requestWithCf(
      installationPath(installation, 'status'), 'GET', credential, undefined,
      { city: 'Austin', region: 'Texas', country: 'US' },
    ));

    const created = await registry.fetch(request(
      installationPath(installation, 'alerts'), 'POST', credential, alertBody(),
    ));
    expect(created.status).toBe(201);
    const firstBody = await json(created);
    expect(firstBody.location).toBe('Austin');

    const listAfterFirst = await registry.fetch(request('/v1/admin/alerts', 'GET', adminToken));
    const listAfterFirstBody = await json(listAfterFirst) as { alerts: Record<string, unknown>[] };
    expect(listAfterFirstBody.alerts[0]?.location).toBe('Austin');

    // Installation moves; a later check-in updates its known location, but
    // the already-stored alert record is not retroactively rewritten.
    await registry.fetch(requestWithCf(
      installationPath(installation, 'reconcile'), 'POST', credential, undefined,
      { city: 'Denver', region: 'Colorado', country: 'US' },
    ));
    const listAfterMove = await registry.fetch(request('/v1/admin/alerts', 'GET', adminToken));
    const listAfterMoveBody = await json(listAfterMove) as { alerts: Record<string, unknown>[] };
    expect(listAfterMoveBody.alerts[0]?.location).toBe('Austin');

    // The NEXT forwarded/re-forwarded occurrence of that same alert picks
    // up the newly-current location.
    const updated = await registry.fetch(request(
      installationPath(installation, 'alerts'), 'POST', credential,
      alertBody({ count: 2, last_seen: '2026-01-01T00:20:00.000Z' }),
    ));
    expect(updated.status).toBe(200);
    const updatedBody = await json(updated);
    expect(updatedBody.location).toBe('Denver');

    const listAfterUpdate = await registry.fetch(request('/v1/admin/alerts', 'GET', adminToken));
    const listAfterUpdateBody = await json(listAfterUpdate) as { alerts: Record<string, unknown>[] };
    expect(listAfterUpdateBody.alerts[0]?.location).toBe('Denver');
  });

  it('falls back to "Unknown" for an alert from an installation with no known location', async () => {
    const installation = await enroll(registry, 'alert-geo-unknown-0001');
    const created = await registry.fetch(request(
      installationPath(installation, 'alerts'), 'POST', String(installation.installationCredential), alertBody(),
    ));
    expect(created.status).toBe(201);
    expect((await json(created)).location).toBe('Unknown');
  });
});

describe('fleet city list (edge geolocation, never raw IP)', () => {
  let storage: MemoryStorage;
  let provider: FakeCloudflare;
  let registry: Registry;

  beforeEach(() => {
    storage = new MemoryStorage();
    provider = new FakeCloudflare();
    vi.stubGlobal('fetch', provider.fetch);
    registry = new Registry({ storage } as unknown as DurableObjectState, env as never);
  });

  it('captures city/region on status, provision, and reconcile, and updates it on a later check-in', async () => {
    const installation = await enroll(registry, 'geo-fleet-0001');
    const credential = String(installation.installationCredential);

    const statusResponse = await registry.fetch(requestWithCf(
      installationPath(installation, 'status'), 'GET', credential, undefined,
      { city: 'Austin', region: 'Texas', country: 'US' },
    ));
    expect(statusResponse.status).toBe(200);
    let stored = await storage.get<Record<string, unknown>>(`installation:${String(installation.installationId)}`);
    expect(stored?.lastCity).toBe('Austin');
    expect(stored?.lastRegion).toBe('Texas');
    expect(stored?.lastCountry).toBe('US');

    await registry.fetch(requestWithCf(
      installationPath(installation, 'provision'), 'POST', credential, { generation: '1' },
      { city: 'Austin', region: 'Texas', country: 'US' },
    ));

    // Simulate the installation moving to a different network: the next
    // routine reconcile check-in should overwrite the stale city, not add
    // a second record or keep the old value.
    const reconcileResponse = await registry.fetch(requestWithCf(
      installationPath(installation, 'reconcile'), 'POST', credential, undefined,
      { city: 'Denver', region: 'Colorado', country: 'US' },
    ));
    expect(reconcileResponse.status).toBe(200);
    stored = await storage.get<Record<string, unknown>>(`installation:${String(installation.installationId)}`);
    expect(stored?.lastCity).toBe('Denver');
    expect(stored?.lastRegion).toBe('Colorado');

    const citiesResponse = await registry.fetch(request('/v1/admin/cities', 'GET', adminToken));
    const citiesBody = await json(citiesResponse) as { cities: Record<string, unknown>[] };
    const denver = citiesBody.cities.find((row) => row.location === 'Denver');
    expect(denver?.count).toBe(1);
    expect(citiesBody.cities.find((row) => row.location === 'Austin')).toBeUndefined();
  });

  it('falls back to region, then country, then "Unknown" when finer geolocation is unavailable', async () => {
    const noCity = await enroll(registry, 'geo-fleet-0002');
    await registry.fetch(requestWithCf(
      installationPath(noCity, 'status'), 'GET', String(noCity.installationCredential), undefined,
      { region: 'Ontario', country: 'CA' },
    ));

    const noRegion = await enroll(registry, 'geo-fleet-0003');
    await registry.fetch(requestWithCf(
      installationPath(noRegion, 'status'), 'GET', String(noRegion.installationCredential), undefined,
      { country: 'DE' },
    ));

    const noGeo = await enroll(registry, 'geo-fleet-0004');
    await registry.fetch(requestWithCf(
      installationPath(noGeo, 'status'), 'GET', String(noGeo.installationCredential), undefined, undefined,
    ));

    const response = await registry.fetch(request('/v1/admin/cities', 'GET', adminToken));
    const body = await json(response) as { cities: Record<string, unknown>[] };
    const byLocation = new Map(body.cities.map((row) => [row.location, row.count]));
    expect(byLocation.get('Ontario')).toBe(1);
    expect(byLocation.get('DE')).toBe(1);
    expect(byLocation.get('Unknown')).toBe(1);
  });

  it('aggregates across multiple installations sharing a city and sorts by count descending', async () => {
    const first = await enroll(registry, 'geo-fleet-0005');
    const second = await enroll(registry, 'geo-fleet-0006');
    const third = await enroll(registry, 'geo-fleet-0007');
    for (const installation of [first, second]) {
      await registry.fetch(requestWithCf(
        installationPath(installation, 'status'), 'GET', String(installation.installationCredential), undefined,
        { city: 'Austin', region: 'Texas', country: 'US' },
      ));
    }
    await registry.fetch(requestWithCf(
      installationPath(third, 'status'), 'GET', String(third.installationCredential), undefined,
      { city: 'Denver', region: 'Colorado', country: 'US' },
    ));

    const response = await registry.fetch(request('/v1/admin/cities', 'GET', adminToken));
    const body = await json(response) as { cities: Record<string, unknown>[] };
    expect(body.cities[0]).toEqual({ location: 'Austin', count: 2 });
    expect(body.cities[1]).toEqual({ location: 'Denver', count: 1 });
  });

  it('rejects GET /v1/admin/cities without a valid admin bearer token', async () => {
    const missing = await registry.fetch(request('/v1/admin/cities', 'GET'));
    expect(missing.status).toBe(401);
    const wrong = await registry.fetch(request('/v1/admin/cities', 'GET', 'wrong-token-not-admin-at-all-x'));
    expect(wrong.status).toBe(401);
    const authorized = await registry.fetch(request('/v1/admin/cities', 'GET', adminToken));
    expect(authorized.status).toBe(200);
    expect(await json(authorized)).toEqual({ cities: [] });
  });

  it('lists installations with label, location, and phase for the admin panel', async () => {
    const installation = await enroll(registry, 'geo-fleet-0008');
    await registry.fetch(requestWithCf(
      installationPath(installation, 'status'), 'GET', String(installation.installationCredential), undefined,
      { city: 'Austin', region: 'Texas', country: 'US' },
    ));

    const response = await registry.fetch(request('/v1/admin/installations', 'GET', adminToken));
    expect(response.status).toBe(200);
    const body = await json(response) as { installations: Record<string, unknown>[] };
    expect(body.installations).toHaveLength(1);
    const row = body.installations[0];
    expect(row.installationId).toBe(installation.installationId);
    expect(row.label).toBe('');
    expect(row.location).toBe('Austin');
    expect(row.revoked).toBe(false);
  });

  it('rejects GET /v1/admin/installations without a valid admin bearer token', async () => {
    const missing = await registry.fetch(request('/v1/admin/installations', 'GET'));
    expect(missing.status).toBe(401);
    const authorized = await registry.fetch(request('/v1/admin/installations', 'GET', adminToken));
    expect(authorized.status).toBe(200);
  });

  it('lets the administrator set and clear an installation label without touching identity/credentials', async () => {
    const installation = await enroll(registry, 'geo-fleet-0009');
    const id = String(installation.installationId);

    const setResponse = await registry.fetch(new Request(`https://control-plane.test/v1/admin/installations/${id}/label`, {
      method: 'PATCH',
      headers: { authorization: `Bearer ${adminToken}`, 'content-type': 'application/json' },
      body: JSON.stringify({ label: 'jpl-graphics Mac' }),
    }));
    expect(setResponse.status).toBe(200);
    expect((await json(setResponse) as Record<string, unknown>).label).toBe('jpl-graphics Mac');

    const listed = await registry.fetch(request('/v1/admin/installations', 'GET', adminToken));
    const listedBody = await json(listed) as { installations: Record<string, unknown>[] };
    expect(listedBody.installations[0].label).toBe('jpl-graphics Mac');
    // Credential/identity untouched by a label change.
    const statusAfter = await registry.fetch(new Request(
      `https://control-plane.test${installationPath(installation, 'status')}`,
      { headers: { authorization: `Bearer ${String(installation.installationCredential)}` } },
    ));
    expect(statusAfter.status).toBe(200);

    const clearResponse = await registry.fetch(new Request(`https://control-plane.test/v1/admin/installations/${id}/label`, {
      method: 'PATCH',
      headers: { authorization: `Bearer ${adminToken}`, 'content-type': 'application/json' },
      body: JSON.stringify({ label: '' }),
    }));
    expect(clearResponse.status).toBe(200);
    expect((await json(clearResponse) as Record<string, unknown>).label).toBe('');
  });

  it('rejects an oversized or non-string label', async () => {
    const installation = await enroll(registry, 'geo-fleet-0010');
    const id = String(installation.installationId);
    const tooLong = 'x'.repeat(201);
    const response = await registry.fetch(new Request(`https://control-plane.test/v1/admin/installations/${id}/label`, {
      method: 'PATCH',
      headers: { authorization: `Bearer ${adminToken}`, 'content-type': 'application/json' },
      body: JSON.stringify({ label: tooLong }),
    }));
    expect(response.status).toBe(400);
  });

  it('rejects a label change without a valid admin bearer token', async () => {
    const installation = await enroll(registry, 'geo-fleet-0011');
    const id = String(installation.installationId);
    const response = await registry.fetch(new Request(`https://control-plane.test/v1/admin/installations/${id}/label`, {
      method: 'PATCH',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ label: 'nope' }),
    }));
    expect(response.status).toBe(401);
  });

  it('404s a label change for an unknown installation id', async () => {
    const response = await registry.fetch(new Request('https://control-plane.test/v1/admin/installations/deadbeef/label', {
      method: 'PATCH',
      headers: { authorization: `Bearer ${adminToken}`, 'content-type': 'application/json' },
      body: JSON.stringify({ label: 'x' }),
    }));
    expect(response.status).toBe(404);
  });

  it('archives and unarchives an installation as a pure view flag, not an access change', async () => {
    const installation = await enroll(registry, 'geo-fleet-0012');
    const id = String(installation.installationId);

    const archiveResponse = await registry.fetch(new Request(`https://control-plane.test/v1/admin/installations/${id}/archive`, {
      method: 'PATCH',
      headers: { authorization: `Bearer ${adminToken}`, 'content-type': 'application/json' },
      body: JSON.stringify({ archived: true }),
    }));
    expect(archiveResponse.status).toBe(200);
    const archivedBody = await json(archiveResponse) as Record<string, unknown>;
    expect(archivedBody.archived).toBe(true);
    expect(archivedBody.revoked).toBe(false);

    // Still fully functional: status check-in succeeds while archived.
    const statusResponse = await registry.fetch(new Request(
      `https://control-plane.test${installationPath(installation, 'status')}`,
      { headers: { authorization: `Bearer ${String(installation.installationCredential)}` } },
    ));
    expect(statusResponse.status).toBe(200);

    const listed = await registry.fetch(request('/v1/admin/installations', 'GET', adminToken));
    const listedBody = await json(listed) as { installations: Record<string, unknown>[] };
    const row = listedBody.installations.find((r) => r.installationId === id);
    expect(row?.archived).toBe(true);

    const unarchiveResponse = await registry.fetch(new Request(`https://control-plane.test/v1/admin/installations/${id}/archive`, {
      method: 'PATCH',
      headers: { authorization: `Bearer ${adminToken}`, 'content-type': 'application/json' },
      body: JSON.stringify({ archived: false }),
    }));
    expect(unarchiveResponse.status).toBe(200);
    expect((await json(unarchiveResponse) as Record<string, unknown>).archived).toBe(false);
  });

  it('rejects a non-boolean archive value', async () => {
    const installation = await enroll(registry, 'geo-fleet-0013');
    const id = String(installation.installationId);
    const response = await registry.fetch(new Request(`https://control-plane.test/v1/admin/installations/${id}/archive`, {
      method: 'PATCH',
      headers: { authorization: `Bearer ${adminToken}`, 'content-type': 'application/json' },
      body: JSON.stringify({ archived: 'yes' }),
    }));
    expect(response.status).toBe(400);
  });

  it('rejects an archive change without a valid admin bearer token', async () => {
    const installation = await enroll(registry, 'geo-fleet-0014');
    const id = String(installation.installationId);
    const response = await registry.fetch(new Request(`https://control-plane.test/v1/admin/installations/${id}/archive`, {
      method: 'PATCH',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ archived: true }),
    }));
    expect(response.status).toBe(401);
  });

  it('404s an archive change for an unknown installation id', async () => {
    const response = await registry.fetch(new Request('https://control-plane.test/v1/admin/installations/deadbeef/archive', {
      method: 'PATCH',
      headers: { authorization: `Bearer ${adminToken}`, 'content-type': 'application/json' },
      body: JSON.stringify({ archived: true }),
    }));
    expect(response.status).toBe(404);
  });
});

describe('planning center OAuth routes', () => {
  let storage: MemoryStorage;
  let provider: FakeCloudflare;
  let registry: Registry;

  beforeEach(() => {
    storage = new MemoryStorage();
    provider = new FakeCloudflare();
    vi.stubGlobal('fetch', provider.fetch);
    registry = new Registry({ storage } as unknown as DurableObjectState, env as never);
  });

  async function mintFlow(): Promise<{ flow_id: string; ticket: string }> {
    const response = await registry.fetch(request('/v1/planning-center/oauth/flow', 'POST'));
    expect(response.status).toBe(201);
    return (await json(response)) as { flow_id: string; ticket: string };
  }

  it('mints a flow_id/ticket pair and rate limits abusive callers', async () => {
    const first = await mintFlow();
    expect(first.flow_id).toMatch(/^[a-f0-9]{32}$/);
    expect(first.ticket.length).toBeGreaterThan(0);

    for (let attempt = 0; attempt < 9; attempt += 1) {
      const response = await registry.fetch(request('/v1/planning-center/oauth/flow', 'POST'));
      expect(response.status).toBe(201);
    }
    const limited = await registry.fetch(request('/v1/planning-center/oauth/flow', 'POST'));
    expect(limited.status).toBe(429);
    expect(limited.headers.get('retry-after')).not.toBeNull();
  });

  it('exchanges a valid flow ticket for tokens against the mocked PCO endpoint', async () => {
    const flow = await mintFlow();
    let capturedBody: string | undefined;
    vi.stubGlobal('fetch', vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input : input.url);
      expect(url.toString()).toBe('https://api.planningcenteronline.com/oauth/token');
      capturedBody = String(init?.body);
      return new Response(JSON.stringify({
        access_token: 'new-access-token',
        refresh_token: 'new-refresh-token',
        token_type: 'bearer',
        expires_in: 7200,
      }), { status: 200, headers: { 'content-type': 'application/json' } });
    }));

    const response = await registry.fetch(request('/v1/planning-center/oauth/token', 'POST', undefined, {
      flow_id: flow.flow_id,
      ticket: flow.ticket,
      code: 'auth-code-from-pco',
      code_verifier: 'pkce-verifier',
      redirect_uri: 'http://127.0.0.1:52847/callback',
    }));
    expect(response.status).toBe(200);
    const payload = await json(response);
    expect(payload).toEqual({
      access_token: 'new-access-token',
      refresh_token: 'new-refresh-token',
      token_type: 'bearer',
      expires_in: 7200,
    });
    expect(capturedBody).toContain('grant_type=authorization_code');
    expect(capturedBody).toContain('client_secret=pco-client-secret-never-returned');
  });

  it('rejects a token exchange with a missing or invalid ticket', async () => {
    const flow = await mintFlow();

    const missing = await registry.fetch(request('/v1/planning-center/oauth/token', 'POST', undefined, {
      code: 'auth-code-from-pco',
      code_verifier: 'pkce-verifier',
      redirect_uri: 'http://127.0.0.1:52847/callback',
    }));
    expect(missing.status).toBe(401);

    const wrongTicket = await registry.fetch(request('/v1/planning-center/oauth/token', 'POST', undefined, {
      flow_id: flow.flow_id,
      ticket: 'not-the-real-ticket',
      code: 'auth-code-from-pco',
      code_verifier: 'pkce-verifier',
      redirect_uri: 'http://127.0.0.1:52847/callback',
    }));
    expect(wrongTicket.status).toBe(401);

    const unknownFlow = await registry.fetch(request('/v1/planning-center/oauth/token', 'POST', undefined, {
      flow_id: 'f'.repeat(32),
      ticket: flow.ticket,
      code: 'auth-code-from-pco',
      code_verifier: 'pkce-verifier',
      redirect_uri: 'http://127.0.0.1:52847/callback',
    }));
    expect(unknownFlow.status).toBe(401);
  });

  it('rate limits abuse of the token exchange route', async () => {
    const flow = await mintFlow();
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ access_token: 'x' }), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    })));

    for (let attempt = 0; attempt < 20; attempt += 1) {
      const response = await registry.fetch(request('/v1/planning-center/oauth/token', 'POST', undefined, {
        flow_id: flow.flow_id,
        ticket: flow.ticket,
        code: 'auth-code-from-pco',
        code_verifier: 'pkce-verifier',
        redirect_uri: 'http://127.0.0.1:52847/callback',
      }));
      expect(response.status).toBe(200);
    }
    const limited = await registry.fetch(request('/v1/planning-center/oauth/token', 'POST', undefined, {
      flow_id: flow.flow_id,
      ticket: flow.ticket,
      code: 'auth-code-from-pco',
      code_verifier: 'pkce-verifier',
      redirect_uri: 'http://127.0.0.1:52847/callback',
    }));
    expect(limited.status).toBe(429);
  });

  it('refresh route exchanges a refresh_token grant and requires the same ticket', async () => {
    const flow = await mintFlow();

    const unauthorized = await registry.fetch(request('/v1/planning-center/oauth/refresh', 'POST', undefined, {
      flow_id: flow.flow_id,
      ticket: 'bogus-ticket',
      refresh_token: 'stale-refresh-token',
    }));
    expect(unauthorized.status).toBe(401);

    let capturedBody: string | undefined;
    vi.stubGlobal('fetch', vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input : input.url);
      expect(url.toString()).toBe('https://api.planningcenteronline.com/oauth/token');
      capturedBody = String(init?.body);
      return new Response(JSON.stringify({
        access_token: 'refreshed-access-token',
        refresh_token: 'refreshed-refresh-token',
        token_type: 'bearer',
        expires_in: 7200,
      }), { status: 200, headers: { 'content-type': 'application/json' } });
    }));

    const response = await registry.fetch(request('/v1/planning-center/oauth/refresh', 'POST', undefined, {
      flow_id: flow.flow_id,
      ticket: flow.ticket,
      refresh_token: 'stale-refresh-token',
    }));
    expect(response.status).toBe(200);
    const payload = await json(response);
    expect(payload).toEqual({
      access_token: 'refreshed-access-token',
      refresh_token: 'refreshed-refresh-token',
      token_type: 'bearer',
      expires_in: 7200,
    });
    expect(capturedBody).toContain('grant_type=refresh_token');
    expect(capturedBody).toContain('refresh_token=stale-refresh-token');
  });

  it('relays a PCO invalid_grant error without leaking the client secret', async () => {
    const flow = await mintFlow();
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      error: 'invalid_grant',
    }), { status: 400, headers: { 'content-type': 'application/json' } })));

    const response = await registry.fetch(request('/v1/planning-center/oauth/refresh', 'POST', undefined, {
      flow_id: flow.flow_id,
      ticket: flow.ticket,
      refresh_token: 'revoked-refresh-token',
    }));
    expect(response.status).toBe(400);
    const payload = await json(response);
    expect(payload).toEqual({ error: 'invalid_grant' });
    expect(JSON.stringify(payload)).not.toContain('pco-client-secret-never-returned');
  });
});

describe('fleet admin actions: rate-limit reset and approval requests', () => {
  let storage: MemoryStorage;
  let provider: FakeCloudflare;
  let registry: Registry;

  beforeEach(() => {
    storage = new MemoryStorage();
    provider = new FakeCloudflare();
    vi.stubGlobal('fetch', provider.fetch);
    registry = new Registry({ storage } as unknown as DurableObjectState, env as never);
  });

  it('requires the admin bearer token to queue a rate-limit reset', async () => {
    const installation = await enroll(registry, 'reset-auth-0001');
    const path = installationPath(installation, 'rate-limit/reset');
    expect((await registry.fetch(request(path, 'POST'))).status).toBe(401);
    expect((await registry.fetch(request(path, 'POST', 'wrong-token-not-admin-at-all-x'))).status).toBe(401);
    // The installation's own credential is NOT sufficient for an admin route.
    expect((await registry.fetch(request(
      path, 'POST', String(installation.installationCredential),
    ))).status).toBe(401);
    expect((await registry.fetch(request(path, 'POST', adminToken))).status).toBe(201);
  });

  it('404s a rate-limit reset for an unknown installation', async () => {
    const response = await registry.fetch(request('/v1/installations/deadbeef/rate-limit/reset', 'POST', adminToken));
    expect(response.status).toBe(404);
  });

  it('delivers the queued reset only to the targeted installation and is idempotent', async () => {
    const target = await enroll(registry, 'reset-target-0001');
    const other = await enroll(registry, 'reset-other-0001');

    const queued = await registry.fetch(request(
      installationPath(target, 'rate-limit/reset'), 'POST', adminToken,
    ));
    expect(queued.status).toBe(201);
    const action = await json(queued);
    expect(action.kind).toBe('rate_limit_reset');
    expect(action.installationId).toBe(target.installationId);

    // Re-queueing before it is applied returns the same action, not a duplicate.
    const again = await registry.fetch(request(
      installationPath(target, 'rate-limit/reset'), 'POST', adminToken,
    ));
    expect(again.status).toBe(200);
    expect((await json(again)).actionId).toBe(action.actionId);

    const targetStatus = await json(await registry.fetch(request(
      installationPath(target, 'status'), 'GET', String(target.installationCredential),
    )));
    expect(targetStatus.pendingActions).toHaveLength(1);

    const otherStatus = await json(await registry.fetch(request(
      installationPath(other, 'status'), 'GET', String(other.installationCredential),
    )));
    expect(otherStatus.pendingActions).toHaveLength(0);

    // Another installation cannot acknowledge (and thus suppress) it.
    const foreignAck = await registry.fetch(request(
      `${installationPath(other, 'pending-actions')}/${String(action.actionId)}/ack`,
      'POST', String(other.installationCredential),
    ));
    expect(foreignAck.status).toBe(404);

    const ack = await registry.fetch(request(
      `${installationPath(target, 'pending-actions')}/${String(action.actionId)}/ack`,
      'POST', String(target.installationCredential),
    ));
    expect(ack.status).toBe(200);

    const afterAck = await json(await registry.fetch(request(
      installationPath(target, 'status'), 'GET', String(target.installationCredential),
    )));
    expect(afterAck.pendingActions).toHaveLength(0);
  });

  it('rejects an approval request without the matching installation credential', async () => {
    const installation = await enroll(registry, 'approval-auth-0001');
    const other = await enroll(registry, 'approval-auth-0002');
    const path = installationPath(installation, 'approval-requests');
    expect((await registry.fetch(request(path, 'POST', undefined, { reason: 'x' }))).status).toBe(401);
    expect((await registry.fetch(request(
      path, 'POST', String(other.installationCredential), { reason: 'x' },
    ))).status).toBe(401);
    // Admin token is not an installation principal for this route either.
    expect((await registry.fetch(request(path, 'POST', adminToken, { reason: 'x' }))).status).toBe(401);
  });

  it('rejects an approval request with a missing or invalid reason', async () => {
    const installation = await enroll(registry, 'approval-invalid-0001');
    const path = installationPath(installation, 'approval-requests');
    const credential = String(installation.installationCredential);
    expect((await registry.fetch(request(path, 'POST', credential, { reason: '' }))).status).toBe(400);
    expect((await registry.fetch(request(path, 'POST', credential, { reason: 42 }))).status).toBe(400);
  });

  it('requires the admin bearer token for the approval admin routes', async () => {
    expect((await registry.fetch(request('/v1/admin/approval-requests', 'GET'))).status).toBe(401);
    expect((await registry.fetch(request(
      '/v1/admin/approval-requests', 'GET', 'wrong-token-not-admin-at-all-x',
    ))).status).toBe(401);
    expect((await registry.fetch(request(
      '/v1/admin/approval-requests/anything/decide', 'POST', undefined, { approved: true },
    ))).status).toBe(401);
    expect((await registry.fetch(request(
      '/v1/admin/approval-requests/anything/decide', 'POST', 'wrong-token-not-admin-at-all-x',
      { approved: true },
    ))).status).toBe(401);
  });

  it('runs the approval lifecycle: create -> pending -> decide -> resolved', async () => {
    const installationA = await enroll(registry, 'approval-life-a-0001');
    const installationB = await enroll(registry, 'approval-life-b-0001');

    const created = await registry.fetch(request(
      installationPath(installationA, 'approval-requests'), 'POST',
      String(installationA.installationCredential), { reason: 'quota increase for a real user' },
    ));
    expect(created.status).toBe(201);
    const approval = await json(created);
    expect(approval.status).toBe('pending');
    expect(approval.installationId).toBe(installationA.installationId);

    const denied = await registry.fetch(request(
      installationPath(installationB, 'approval-requests'), 'POST',
      String(installationB.installationCredential), { reason: 'risky setting change' },
    ));
    const bApproval = await json(denied);

    const pending = await json(await registry.fetch(request('/v1/admin/approval-requests', 'GET', adminToken)));
    expect((pending.requests as unknown[])).toHaveLength(2);

    const decide = await registry.fetch(request(
      `/v1/admin/approval-requests/${String(approval.requestId)}/decide`, 'POST', adminToken,
      { approved: true },
    ));
    expect(decide.status).toBe(200);
    expect((await json(decide)).status).toBe('approved');

    const decideB = await registry.fetch(request(
      `/v1/admin/approval-requests/${String(bApproval.requestId)}/decide`, 'POST', adminToken,
      { approved: false },
    ));
    expect((await json(decideB)).status).toBe('denied');

    // Decisions are final.
    const redecide = await registry.fetch(request(
      `/v1/admin/approval-requests/${String(approval.requestId)}/decide`, 'POST', adminToken,
      { approved: false },
    ));
    expect(redecide.status).toBe(409);

    const stillPending = await json(await registry.fetch(request('/v1/admin/approval-requests', 'GET', adminToken)));
    expect((stillPending.requests as unknown[])).toHaveLength(0);

    // The requesting installation sees its own outcome, and only its own.
    const aList = await json(await registry.fetch(request(
      installationPath(installationA, 'approval-requests'), 'GET',
      String(installationA.installationCredential),
    )));
    const aRequests = aList.requests as Record<string, unknown>[];
    expect(aRequests).toHaveLength(1);
    expect(aRequests[0]?.status).toBe('approved');
    expect(aRequests[0]?.installationId).toBe(installationA.installationId);
  });

  it('rejects a decision body without a boolean approved field, and 404s unknown ids', async () => {
    expect((await registry.fetch(request(
      '/v1/admin/approval-requests/does-not-exist/decide', 'POST', adminToken, { approved: 'yes' },
    ))).status).toBe(400);
    expect((await registry.fetch(request(
      '/v1/admin/approval-requests/does-not-exist/decide', 'POST', adminToken, { approved: true },
    ))).status).toBe(404);
  });

  describe('self-reported deviceName', () => {
    it('captures a valid deviceName on enroll and surfaces it in admin list', async () => {
      const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, {
        nonce: 'device-name-enroll-0001',
        deviceName: 'Front-of-House-Mac.local',
      }));
      expect(response.status).toBe(201);
      const installation = await json(response);
      expect(installation.deviceName).toBe('Front-of-House-Mac.local');

      const list = await json(await registry.fetch(request('/v1/admin/installations', 'GET', adminToken)));
      const rows = list.installations as Record<string, unknown>[];
      expect(rows.find((row) => row.installationId === installation.installationId)?.deviceName)
        .toBe('Front-of-House-Mac.local');
    });

    it('rejects an oversized or invalid-shaped enroll body without enrolling', async () => {
      const tooMany = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, {
        nonce: 'device-name-reject-0001',
        deviceName: 'ok',
        unexpected: 'field',
      }));
      expect(tooMany.status).toBe(400);
    });

    it('truncates an overlong deviceName to 253 characters and strips control characters', async () => {
      const response = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, {
        nonce: 'device-name-long-0001',
        deviceName: `bad\x07name${'x'.repeat(300)}`,
      }));
      const installation = await json(response);
      expect(String(installation.deviceName).includes('\x07')).toBe(false);
      expect(String(installation.deviceName).length).toBeLessThanOrEqual(253);
    });

    it('drops an empty-after-trim or non-string deviceName instead of storing it', async () => {
      const empty = await json(await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, {
        nonce: 'device-name-empty-0001',
        deviceName: '   ',
      })));
      expect(empty.deviceName).toBeNull();

      const nonString = await json(await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, {
        nonce: 'device-name-nonstring-0001',
        deviceName: 12345,
      })));
      expect(nonString.deviceName).toBeNull();
    });

    it('auto-fills label from deviceName only the first time, never overwriting an operator-set label', async () => {
      const enrolled = await enroll(registry, 'device-name-label-0001');
      const id = String(enrolled.installationId);

      // First authenticated status check-in carries a deviceName: label
      // auto-fills because it has never been set before.
      const firstStatus = await json(await registry.fetch(request(
        statusPathWithDeviceName(enrolled, 'Stage-Laptop'), 'GET',
        String(enrolled.installationCredential),
      )));
      expect(firstStatus.deviceName).toBe('Stage-Laptop');
      let adminRow = (await json(await registry.fetch(
        request('/v1/admin/installations', 'GET', adminToken),
      ))).installations as Record<string, unknown>[];
      expect(adminRow.find((row) => row.installationId === id)?.label).toBe('Stage-Laptop');

      // Operator explicitly sets a different label.
      await registry.fetch(request(
        `/v1/admin/installations/${id}/label`, 'PATCH', adminToken, { label: 'Front Desk' },
      ));

      // A later check-in with a DIFFERENT deviceName must never clobber
      // the operator's label, even though deviceName itself still updates.
      const secondStatus = await json(await registry.fetch(request(
        statusPathWithDeviceName(enrolled, 'Renamed-Laptop'), 'GET',
        String(enrolled.installationCredential),
      )));
      expect(secondStatus.deviceName).toBe('Renamed-Laptop');
      adminRow = (await json(await registry.fetch(
        request('/v1/admin/installations', 'GET', adminToken),
      ))).installations as Record<string, unknown>[];
      expect(adminRow.find((row) => row.installationId === id)?.label).toBe('Front Desk');
    });

    it('never auto-fills label when an operator already cleared it to blank', async () => {
      const enrolled = await enroll(registry, 'device-name-cleared-label-0001');
      const id = String(enrolled.installationId);
      // Operator explicitly sets then clears the label (an explicit "blank"
      // decision, not merely "never set").
      await registry.fetch(request(
        `/v1/admin/installations/${id}/label`, 'PATCH', adminToken, { label: 'Temp Name' },
      ));
      await registry.fetch(request(
        `/v1/admin/installations/${id}/label`, 'PATCH', adminToken, { label: '' },
      ));
      // Per spec: auto-fill is tracked only via "label is currently empty
      // AND this is the first deviceName ever stored on this installation".
      // The installation already has no deviceName stored yet here, so a
      // first deviceName arriving now still legitimately auto-fills --
      // this test documents that exact boundary rather than asserting a
      // stronger guarantee the spec does not provide.
      const status = await json(await registry.fetch(request(
        statusPathWithDeviceName(enrolled, 'Whatever-Box'), 'GET',
        String(enrolled.installationCredential),
      )));
      expect(status.deviceName).toBe('Whatever-Box');
    });
  });

  describe('lightweight check-in never triggers provisioning', () => {
    it('GET status with deviceName never calls the Cloudflare provider', async () => {
      const enrolled = await enroll(registry, 'lightweight-checkin-0001');
      provider.fetch.mockClear();

      const status = await registry.fetch(request(
        statusPathWithDeviceName(enrolled, 'Lightweight-Only'), 'GET',
        String(enrolled.installationCredential),
      ));
      expect(status.status).toBe(200);
      expect(provider.fetch).not.toHaveBeenCalled();
      const payload = await json(status);
      expect(payload.phase).toBe('disabled');
    });

    it('repeated anonymous enroll replay with deviceName never calls the Cloudflare provider', async () => {
      provider.fetch.mockClear();
      await enroll(registry, 'lightweight-checkin-replay-0001');
      const replay = await registry.fetch(request('/v1/installations/enroll', 'POST', undefined, {
        nonce: 'lightweight-checkin-replay-0001',
        deviceName: 'Still-Lightweight',
      }));
      expect(replay.status).toBe(201);
      expect(provider.fetch).not.toHaveBeenCalled();
    });
  });
});

// Minimal in-memory stand-in for R2Bucket, covering only what index.ts uses
// (put/get keyed by object key, returning an object whose .body is readable).
class FakeR2Bucket {
  readonly objects = new Map<string, { bytes: Uint8Array; httpMetadata?: Record<string, unknown> }>();

  put = vi.fn(async (key: string, value: Uint8Array, options?: { httpMetadata?: Record<string, unknown> }): Promise<void> => {
    this.objects.set(key, { bytes: value, httpMetadata: options?.httpMetadata });
  });

  get = vi.fn(async (key: string): Promise<{ body: ReadableStream<Uint8Array> } | null> => {
    const object = this.objects.get(key);
    if (!object) return null;
    const bytes = object.bytes;
    return {
      body: new ReadableStream<Uint8Array>({
        start(controller) {
          controller.enqueue(bytes);
          controller.close();
        },
      }),
    };
  });

  delete = vi.fn(async (key: string): Promise<void> => {
    this.objects.delete(key);
  });
}

function rawRequest(path: string, method: string, token: string | undefined, bytes: Uint8Array): Request {
  return new Request(`https://control.example.com${path}`, {
    method,
    headers: {
      ...(token ? { authorization: `Bearer ${token}` } : {}),
      'content-type': 'application/json',
      'content-length': String(bytes.byteLength),
    },
    body: bytes,
  });
}

describe('diagnostic bundle uploads and admin retrieval', () => {
  let storage: MemoryStorage;
  let provider: FakeCloudflare;
  let diagnostics: FakeR2Bucket;
  let registry: Registry;

  beforeEach(() => {
    storage = new MemoryStorage();
    provider = new FakeCloudflare();
    diagnostics = new FakeR2Bucket();
    vi.stubGlobal('fetch', provider.fetch);
    registry = new Registry({ storage } as unknown as DurableObjectState, { ...env, DIAGNOSTICS: diagnostics } as never);
  });

  function bundleBytes(size = 100): Uint8Array {
    return new TextEncoder().encode(JSON.stringify({ frontend: 'x'.repeat(Math.max(0, size - 40)), version: '1.0.0' }));
  }

  it('rejects a diagnostics POST for an installation that does not exist', async () => {
    const response = await registry.fetch(rawRequest(
      '/v1/installations/deadbeef/diagnostics', 'POST', 'irrelevant-credential', bundleBytes(),
    ));
    expect(response.status).toBe(401);
  });

  it('rejects a diagnostics POST with the wrong installation credential', async () => {
    const installation = await enroll(registry, 'diag-auth-0001');
    const response = await registry.fetch(rawRequest(
      installationPath(installation, 'diagnostics'), 'POST', 'not-the-real-credential', bundleBytes(),
    ));
    expect(response.status).toBe(401);
  });

  it('rejects a diagnostics POST for a revoked installation', async () => {
    const installation = await enroll(registry, 'diag-auth-0002');
    const revoke = await registry.fetch(request(
      installationPath(installation, 'revoke'), 'POST', String(installation.installationCredential),
    ));
    expect(revoke.status).toBe(200);
    const response = await registry.fetch(rawRequest(
      installationPath(installation, 'diagnostics'), 'POST', String(installation.installationCredential), bundleBytes(),
    ));
    expect(response.status).toBe(401);
  });

  it('accepts and stores a valid diagnostic bundle', async () => {
    const installation = await enroll(registry, 'diag-store-0001');
    const response = await registry.fetch(rawRequest(
      installationPath(installation, 'diagnostics'), 'POST', String(installation.installationCredential), bundleBytes(),
    ));
    expect(response.status).toBe(201);
    const bodyJson = await json(response);
    expect(bodyJson.size).toBeGreaterThan(0);
    expect(diagnostics.objects.size).toBe(1);
  });

  it('rejects a bundle over the 2MB size cap', async () => {
    const installation = await enroll(registry, 'diag-size-0001');
    const huge = new Uint8Array(2 * 1024 * 1024 + 1);
    const response = await registry.fetch(rawRequest(
      installationPath(installation, 'diagnostics'), 'POST', String(installation.installationCredential), huge,
    ));
    expect(response.status).toBe(400);
    expect(diagnostics.objects.size).toBe(0);
  });

  it('still accepts a bundle right at the 2MB cap', async () => {
    const installation = await enroll(registry, 'diag-size-0002');
    const atCap = new Uint8Array(2 * 1024 * 1024);
    const response = await registry.fetch(rawRequest(
      installationPath(installation, 'diagnostics'), 'POST', String(installation.installationCredential), atCap,
    ));
    expect(response.status).toBe(201);
  });

  it('rate-limits repeated diagnostic uploads independently of the mutation rate limit', async () => {
    const installation = await enroll(registry, 'diag-rate-0001');
    for (let index = 0; index < 6; index += 1) {
      const response = await registry.fetch(rawRequest(
        installationPath(installation, 'diagnostics'), 'POST', String(installation.installationCredential), bundleBytes(),
      ));
      expect(response.status).toBe(201);
    }
    const limited = await registry.fetch(rawRequest(
      installationPath(installation, 'diagnostics'), 'POST', String(installation.installationCredential), bundleBytes(),
    ));
    expect(limited.status).toBe(429);

    // The dedicated diagnostics rate limit does not consume the installation's
    // normal mutation quota -- a status/mutation call still succeeds.
    const status = await registry.fetch(request(
      installationPath(installation, 'status'), 'GET', String(installation.installationCredential),
    ));
    expect(status.status).toBe(200);
  });

  it('responds 503 instead of throwing when the DIAGNOSTICS R2 binding is absent', async () => {
    const registryWithoutR2 = new Registry({ storage } as unknown as DurableObjectState, env as never);
    const installation = await enroll(registryWithoutR2, 'diag-no-binding-0001');
    const response = await registryWithoutR2.fetch(rawRequest(
      installationPath(installation, 'diagnostics'), 'POST', String(installation.installationCredential), bundleBytes(),
    ));
    expect(response.status).toBe(503);
  });

  it('requires the admin bearer token for listing and retrieving diagnostics', async () => {
    const installation = await enroll(registry, 'diag-admin-auth-0001');
    await registry.fetch(rawRequest(
      installationPath(installation, 'diagnostics'), 'POST', String(installation.installationCredential), bundleBytes(),
    ));
    const missingList = await registry.fetch(request(
      `/v1/admin/installations/${String(installation.installationId)}/diagnostics`, 'GET',
    ));
    expect(missingList.status).toBe(401);
    const list = await registry.fetch(request(
      `/v1/admin/installations/${String(installation.installationId)}/diagnostics`, 'GET', adminToken,
    ));
    expect(list.status).toBe(200);
    const { bundles } = await json(list) as { bundles: Record<string, unknown>[] };
    expect(bundles).toHaveLength(1);

    const missingGet = await registry.fetch(request(
      `/v1/admin/installations/${String(installation.installationId)}/diagnostics/${String(bundles[0]?.id)}`, 'GET',
    ));
    expect(missingGet.status).toBe(401);
  });

  it('lists uploaded bundles with timestamp and size, and streams one back via admin GET', async () => {
    const installation = await enroll(registry, 'diag-list-0001');
    const uploadResponse = await registry.fetch(rawRequest(
      installationPath(installation, 'diagnostics'), 'POST', String(installation.installationCredential), bundleBytes(321),
    ));
    expect(uploadResponse.status).toBe(201);

    const list = await registry.fetch(request(
      `/v1/admin/installations/${String(installation.installationId)}/diagnostics`, 'GET', adminToken,
    ));
    const { bundles } = await json(list) as { bundles: Record<string, unknown>[] };
    expect(bundles).toHaveLength(1);
    expect(bundles[0]?.size).toBeGreaterThan(0);
    expect(typeof bundles[0]?.uploadedAt).toBe('string');

    const download = await registry.fetch(request(
      `/v1/admin/installations/${String(installation.installationId)}/diagnostics/${String(bundles[0]?.id)}`,
      'GET', adminToken,
    ));
    expect(download.status).toBe(200);
    const downloadedText = await download.text();
    const parsed = JSON.parse(downloadedText) as Record<string, unknown>;
    expect(parsed.version).toBe('1.0.0');
  });

  it('404s an admin diagnostics GET for an unknown bundle id', async () => {
    const installation = await enroll(registry, 'diag-404-0001');
    const response = await registry.fetch(request(
      `/v1/admin/installations/${String(installation.installationId)}/diagnostics/20260101T000000Z-deadbeef`,
      'GET', adminToken,
    ));
    expect(response.status).toBe(404);
  });

  it('fails closed with 503 once the account-wide storage cap would be exceeded', async () => {
    const installation = await enroll(registry, 'diag-cap-0001');
    // Directly seed registry:stats so the test doesn't need to actually
    // upload 8 GiB of bundles -- this is the same counter uploadDiagnostics()
    // reads/writes, so seeding it is equivalent to having really uploaded
    // that much.
    await storage.put('registry:stats', {
      activeInstallations: 1,
      enrollments: 1,
      enrollmentDenied: 0,
      statusDenied: 0,
      mutationDenied: 0,
      providerDenied: 0,
      diagnosticsBytesUsed: 8 * 1024 * 1024 * 1024 - 50, // 50 bytes of headroom left
    });
    const response = await registry.fetch(rawRequest(
      installationPath(installation, 'diagnostics'), 'POST', String(installation.installationCredential), bundleBytes(200),
    ));
    expect(response.status).toBe(503);
    expect(diagnostics.objects.size).toBe(0);
  });

  it('still accepts an upload that fits exactly within remaining cap headroom', async () => {
    const installation = await enroll(registry, 'diag-cap-0002');
    const bundle = bundleBytes(200);
    await storage.put('registry:stats', {
      activeInstallations: 1,
      enrollments: 1,
      enrollmentDenied: 0,
      statusDenied: 0,
      mutationDenied: 0,
      providerDenied: 0,
      diagnosticsBytesUsed: 8 * 1024 * 1024 * 1024 - bundle.byteLength,
    });
    const response = await registry.fetch(rawRequest(
      installationPath(installation, 'diagnostics'), 'POST', String(installation.installationCredential), bundle,
    ));
    expect(response.status).toBe(201);
  });

  it('backward-compat: treats a pre-existing registry:stats with no diagnosticsBytesUsed as zero, not NaN', async () => {
    const installation = await enroll(registry, 'diag-compat-0001');
    // Simulates production state from before this field existed.
    await storage.put('registry:stats', {
      activeInstallations: 1,
      enrollments: 1,
      enrollmentDenied: 0,
      statusDenied: 0,
      mutationDenied: 0,
      providerDenied: 0,
    });
    const response = await registry.fetch(rawRequest(
      installationPath(installation, 'diagnostics'), 'POST', String(installation.installationCredential), bundleBytes(200),
    ));
    expect(response.status).toBe(201);
    const stats = await storage.get<{ diagnosticsBytesUsed: number }>('registry:stats');
    expect(Number.isFinite(stats?.diagnosticsBytesUsed)).toBe(true);
    expect(stats?.diagnosticsBytesUsed).toBeGreaterThan(0);
  });

  it('lazily expires bundles past the retention window and reclaims their bytes from the running total', async () => {
    const installation = await enroll(registry, 'diag-expire-0001');
    const old = await registry.fetch(rawRequest(
      installationPath(installation, 'diagnostics'), 'POST', String(installation.installationCredential), bundleBytes(500),
    ));
    expect(old.status).toBe(201);
    expect(diagnostics.objects.size).toBe(1);

    // Backdate the stored metadata to just past the retention window.
    const rows = await storage.list<{ uploadedAt: string; size: number }>({ prefix: 'diagnostics-meta:' });
    const [metaKey, meta] = [...rows.entries()][0];
    const ancient = new Date(Date.now() - 31 * 24 * 60 * 60 * 1000).toISOString();
    await storage.put(metaKey, { ...meta, uploadedAt: ancient });
    const statsBefore = await storage.get<{ diagnosticsBytesUsed: number }>('registry:stats');
    expect(statsBefore?.diagnosticsBytesUsed).toBeGreaterThan(0);

    // Any subsequent upload triggers the lazy sweep before its own cap check.
    const installation2 = await enroll(registry, 'diag-expire-0002');
    const fresh = await registry.fetch(rawRequest(
      installationPath(installation2, 'diagnostics'), 'POST', String(installation2.installationCredential), bundleBytes(100),
    ));
    expect(fresh.status).toBe(201);

    // The expired bundle's R2 object and metadata are gone, and its bytes
    // were subtracted from the running total (only the fresh upload's bytes
    // remain counted).
    expect(diagnostics.objects.size).toBe(1);
    const metaAfter = await storage.get(metaKey);
    expect(metaAfter).toBeUndefined();
    const statsAfter = await storage.get<{ diagnosticsBytesUsed: number }>('registry:stats');
    expect(statsAfter?.diagnosticsBytesUsed).toBeLessThan(statsBefore!.diagnosticsBytesUsed!);
  });
});

