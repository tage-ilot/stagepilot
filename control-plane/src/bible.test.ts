import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import worker, { Registry } from './index';
import {
  ESV_COPYRIGHT,
  MAX_VERSES_PER_REQUEST,
  VERSION_ROUTES,
  htmlToText,
  parseReference,
  positiveInteger,
} from './bible';

// Canary secrets: every test that reads a response or a log line checks that
// none of these values (and none of the verse text) ever shows up in it.
const API_BIBLE_KEY = 'canary-api-bible-key-3f9a1c7e5b2d';
const YOUVERSION_KEY = 'canary-youversion-key-8d41e0aa77c3';
const ESV_KEY = 'canary-esv-key-52b9c6d1f04e';
const CANARIES = [API_BIBLE_KEY, YOUVERSION_KEY, ESV_KEY];

class MemoryStorage {
  readonly values = new Map<string, unknown>();
  alarmAt: number | null = null;
  async getAlarm(): Promise<number | null> { return this.alarmAt; }
  async setAlarm(at: number): Promise<void> { this.alarmAt = at; }
  async get<T>(key: string): Promise<T | undefined> { return this.values.get(key) as T | undefined; }
  async put(keyOrEntries: string | Record<string, unknown>, value?: unknown): Promise<void> {
    if (typeof keyOrEntries === 'string') {
      this.values.set(keyOrEntries, structuredClone(value));
      return;
    }
    for (const [key, entry] of Object.entries(keyOrEntries)) this.values.set(key, structuredClone(entry));
  }
  async delete(keys: string | string[]): Promise<boolean | number> {
    if (typeof keys === 'string') return this.values.delete(keys);
    let deleted = 0;
    for (const key of keys) if (this.values.delete(key)) deleted += 1;
    return deleted;
  }
  async list<T>(options?: { prefix?: string }): Promise<Map<string, T>> {
    return new Map([...this.values.entries()]
      .filter(([key]) => !options?.prefix || key.startsWith(options.prefix))
      .map(([key, value]) => [key, structuredClone(value) as T]));
  }
}

// A recording stand-in for the Cloudflare Cache API.
class FakeCache {
  readonly entries = new Map<string, { body: string; cacheControl: string | null }>();
  readonly matched: string[] = [];
  readonly put = vi.fn(async (request: Request, response: Response): Promise<void> => {
    this.entries.set(request.url, { body: await response.text(), cacheControl: response.headers.get('cache-control') });
  });
  readonly match = vi.fn(async (request: Request): Promise<Response | undefined> => {
    this.matched.push(request.url);
    const entry = this.entries.get(request.url);
    return entry ? new Response(entry.body, { headers: { 'content-type': 'application/json' } }) : undefined;
  });
}

interface UpstreamCall {
  host: string;
  path: string;
  params: URLSearchParams;
  headers: Record<string, string>;
}

// Recorded-shape upstream answers, written from the Python providers
// (api_bible.py, youversion.py, esv.py) and python/tests/test_bible_providers.py.
// The verse text is synthetic, not Scripture.
class FakeUpstream {
  calls: UpstreamCall[] = [];
  // Per-test overrides: return a Response to replace the normal answer.
  override: ((call: UpstreamCall) => Response | Promise<Response> | undefined) | undefined;

  readonly fetch = vi.fn(async (input: string | URL | Request, init?: RequestInit): Promise<Response> => {
    const target = new URL(typeof input === 'string' ? input : input instanceof URL ? input.toString() : input.url);
    const headers: Record<string, string> = {};
    for (const [name, value] of Object.entries((init?.headers ?? {}) as Record<string, string>)) headers[name.toLowerCase()] = value;
    const call = { host: target.host, path: target.pathname, params: target.searchParams, headers };
    this.calls.push(call);
    const replaced = await this.override?.(call);
    if (replaced) return replaced;
    return this.answer(call);
  });

  private answer(call: UpstreamCall): Response {
    const json = (value: unknown): Response => new Response(JSON.stringify(value), { headers: { 'content-type': 'application/json' } });
    if (call.host === 'api.scripture.api.bible') {
      if (call.path === '/v1/bibles') {
        return json({ data: [
          { id: 'other-bible-01', abbreviation: 'OTH', name: 'Other', language: { id: 'eng' }, copyright: 'Other copyright' },
          {
            id: '63097d2a0a2f7db3-01', abbreviation: 'NKJV', abbreviationLocal: 'NKJV', name: 'New King James Version',
            language: { id: 'eng' }, copyright: 'Synthetic NKJV copyright', description: 'Synthetic NKJV description',
          },
          { id: 'de4e12af7f28f599-01', abbreviation: 'KJV', name: 'King James', language: { id: 'eng' }, copyright: 'Public Domain', description: 'KJV description' },
          { id: '9879dbb7cfe39e4d-01', abbreviation: 'WEB', name: 'World English Bible', language: { id: 'eng' }, copyright: 'Public Domain', description: 'WEB description' },
          { id: '6f11a7de016f942e-01', abbreviation: 'MSG', name: 'The Message', language: { id: 'eng' }, copyright: 'Synthetic MSG copyright' },
          { id: 'd6e14a625393b4da-01', abbreviation: 'NLT', name: 'New Living Translation', language: { id: 'eng' }, copyright: 'Synthetic NLT copyright' },
        ] });
      }
      const verse = call.path.match(/^\/v1\/bibles\/([^/]+)\/verses\/([A-Z0-9]+)\.(\d+)\.(\d+)$/);
      if (verse) {
        const [, , book, chapter, number] = verse;
        return json({ data: {
          id: `${book}.${chapter}.${number}`,
          content: `<p class="p"><span data-number="${number}" class="v">[${number}]</span> Synthetic <span class="nd">API</span> verse ${chapter}:${number}.</p>`,
          copyright: 'Synthetic per-verse copyright',
        } });
      }
    }
    if (call.host === 'api.youversion.com') {
      if (call.path === '/v1/bibles') {
        return json({ data: [
          { id: 111, abbreviation: 'NIV11', localized_title: 'New International Version', language_tag: 'en', copyright: 'Synthetic NIV copyright', promotional_content: 'Synthetic promo' },
          { id: 1849, abbreviation: 'TPT', localized_title: 'The Passion Translation', language_tag: 'en', copyright: 'Synthetic TPT copyright', publisher_url: 'https://example.test/tpt' },
        ] });
      }
      const passage = call.path.match(/^\/v1\/bibles\/(\d+)\/passages\/([A-Z0-9]+)\.(\d+)\.(\d+)$/);
      if (passage) return json({ content: `  Synthetic   YouVersion\n verse ${passage[3]}:${passage[4]}.  ` });
    }
    if (call.host === 'api.esv.org' && call.path === '/v3/passage/text/') {
      return json({ query: call.params.get('q'), passages: [`  Synthetic ESV\n verse for ${call.params.get('q')}.  `] });
    }
    return new Response('unexpected upstream request', { status: 599 });
  }
}

const adminToken = 'admin-token-with-at-least-thirty-two-characters';
const baseEnv = {
  CLOUDFLARE_API_TOKEN: 'provider-secret-never-returned',
  CLOUDFLARE_ACCOUNT_ID: 'a'.repeat(32),
  CLOUDFLARE_ZONE_ID: 'b'.repeat(32),
  REMOTE_HOST_SUFFIX: 'remote.example.com',
  ADMIN_API_TOKEN: adminToken,
  INSTALLATION_SIGNING_KEY: 'installation-signing-key-at-least-32-bytes',
  PLANNING_CENTER_CLIENT_ID: 'pco-client-id',
  PLANNING_CENTER_CLIENT_SECRET: 'pco-client-secret-never-returned',
  REMOTE_PORT: '18766',
  ENROLLMENT_ENABLED: 'true',
  BETA_INSTALLATION_LIMIT: '500',
  BETA_RELEASE_VERSIONS: '1.1.103-beta.1',
  BETA_LATEST_RELEASE_VERSION: '1.1.103-beta.1',
  API_BIBLE_API_KEY: API_BIBLE_KEY,
  YOUVERSION_API_KEY: YOUVERSION_KEY,
  ESV_API_KEY: ESV_KEY,
};

interface Enrolled {
  id: string;
  credential: string;
}

describe('Bible text proxy', () => {
  let storage: MemoryStorage;
  let upstream: FakeUpstream;
  let cache: FakeCache;
  let registry: Registry;
  let workerEnv: Record<string, unknown>;
  let logged: string[];

  const buildEnv = (extra: Record<string, unknown> = {}): Record<string, unknown> => {
    const merged: Record<string, unknown> = { ...baseEnv, ...extra };
    merged.REGISTRY = { idFromName: (name: string) => name, get: () => ({ fetch: (request: Request) => registry.fetch(request) }) };
    return merged;
  };

  const useEnv = (extra: Record<string, unknown> = {}): void => {
    workerEnv = buildEnv(extra);
    registry = new Registry({ storage } as unknown as DurableObjectState, workerEnv as never);
  };

  beforeEach(() => {
    storage = new MemoryStorage();
    upstream = new FakeUpstream();
    cache = new FakeCache();
    logged = [];
    vi.stubGlobal('fetch', upstream.fetch);
    vi.stubGlobal('caches', { default: cache });
    for (const method of ['log', 'info', 'warn', 'error', 'debug'] as const) {
      vi.spyOn(console, method).mockImplementation((...args: unknown[]) => { logged.push(args.map(String).join(' ')); });
    }
    useEnv();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  async function enroll(nonce: string): Promise<Enrolled> {
    const response = await registry.fetch(new Request('https://control.example.com/v1/installations/enroll', {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'cf-connecting-ip': '203.0.113.10' },
      body: JSON.stringify({ nonce }),
    }));
    expect(response.status).toBe(201);
    const body = await response.json() as { installationId: string; installationCredential: string };
    return { id: body.installationId, credential: body.installationCredential };
  }

  function passage(who: Enrolled | undefined, version: string, ref: string, id = who?.id ?? 'deadbeef'): Promise<Response> {
    const query = new URLSearchParams({ version, ref });
    return worker.fetch(new Request(`https://control.example.com/v1/installations/${id}/bible/passage?${query}`, {
      headers: who ? { authorization: `Bearer ${who.credential}` } : {},
    }), workerEnv as never);
  }

  function passagePost(who: Enrolled, body: unknown, raw?: string): Promise<Response> {
    return worker.fetch(new Request(`https://control.example.com/v1/installations/${who.id}/bible/passage`, {
      method: 'POST',
      headers: { authorization: `Bearer ${who.credential}`, 'content-type': 'application/json' },
      body: raw ?? JSON.stringify(body),
    }), workerEnv as never);
  }

  const verseCalls = (): UpstreamCall[] => upstream.calls.filter((call) => /\/(verses|passages)\/|passage\/text/.test(call.path));

  function expectClean(...texts: string[]): void {
    const haystack = [...texts, ...logged].join('\n');
    for (const canary of CANARIES) expect(haystack).not.toContain(canary);
  }

  describe('providers', () => {
    it('serves NKJV through API.Bible exactly as the Python provider calls it', async () => {
      const friend = await enroll('bible-nkjv-0001');
      const response = await passage(friend, 'nkjv', 'John 3:16-18');
      expect(response.status).toBe(200);
      expect(response.headers.get('cache-control')).toBe('no-store');
      const body = await response.json() as Record<string, unknown>;
      expect(body).toEqual({
        version: 'NKJV',
        reference: 'John 3:16-18',
        verses: [
          { book_id: 'JHN', chapter: 3, verse: 16, text: 'Synthetic API verse 3:16.' },
          { book_id: 'JHN', chapter: 3, verse: 17, text: 'Synthetic API verse 3:17.' },
          { book_id: 'JHN', chapter: 3, verse: 18, text: 'Synthetic API verse 3:18.' },
        ],
        copyright: 'Synthetic per-verse copyright',
        attribution: 'Synthetic NKJV description',
        provider: 'api_bible',
        cached: false,
      });
      const list = upstream.calls.find((call) => call.path === '/v1/bibles');
      expect(list?.params.get('language')).toBe('eng');
      expect(list?.headers['api-key']).toBe(API_BIBLE_KEY);
      const verses = verseCalls();
      expect(verses.map((call) => call.path)).toEqual([
        '/v1/bibles/63097d2a0a2f7db3-01/verses/JHN.3.16',
        '/v1/bibles/63097d2a0a2f7db3-01/verses/JHN.3.17',
        '/v1/bibles/63097d2a0a2f7db3-01/verses/JHN.3.18',
      ]);
      for (const call of verses) {
        expect(call.host).toBe('api.scripture.api.bible');
        expect(call.headers['api-key']).toBe(API_BIBLE_KEY);
        expect(Object.fromEntries(call.params)).toEqual({ 'content-type': 'text', 'include-notes': 'false', 'include-titles': 'false' });
      }
      expectClean(JSON.stringify(body));
    });

    it('falls back to the Bible copyright when a verse carries none, and keeps the verse order across spans', async () => {
      upstream.override = (call) => /\/verses\//.test(call.path)
        ? new Response(JSON.stringify({ data: { content: '<p>[1-2] Combined synthetic verse.</p>' } }))
        : undefined;
      const friend = await enroll('bible-nkjv-0002');
      const body = await (await passage(friend, 'NKJV', 'John 3:16 & 20')).json() as { verses: Array<{ verse: number; text: string }>; copyright: string };
      expect(body.verses.map((verse) => verse.verse)).toEqual([16, 20]);
      expect(body.verses[0].text).toBe('Combined synthetic verse.');
      expect(body.copyright).toBe('Synthetic NKJV copyright');
    });

    it('serves NIV through YouVersion with the configured Bible id and collapsed whitespace', async () => {
      const friend = await enroll('bible-niv-0001');
      const response = await passage(friend, 'NIV', 'John 3:16-17');
      expect(response.status).toBe(200);
      expect(await response.json()).toEqual({
        version: 'NIV',
        reference: 'John 3:16-17',
        verses: [
          { book_id: 'JHN', chapter: 3, verse: 16, text: 'Synthetic YouVersion verse 3:16.' },
          { book_id: 'JHN', chapter: 3, verse: 17, text: 'Synthetic YouVersion verse 3:17.' },
        ],
        copyright: 'Synthetic NIV copyright',
        attribution: 'Synthetic promo',
        provider: 'youversion',
        cached: false,
      });
      const list = upstream.calls.find((call) => call.path === '/v1/bibles');
      expect(list?.host).toBe('api.youversion.com');
      expect(list?.params.get('language_ranges[]')).toBe('en');
      expect(list?.params.get('page_size')).toBe('99');
      expect(verseCalls().map((call) => call.path)).toEqual([
        '/v1/bibles/111/passages/JHN.3.16', '/v1/bibles/111/passages/JHN.3.17',
      ]);
      for (const call of upstream.calls) expect(call.headers['x-yvp-app-key']).toBe(YOUVERSION_KEY);
      for (const call of verseCalls()) {
        expect(Object.fromEntries(call.params)).toEqual({ format: 'text', include_headings: 'false', include_notes: 'false' });
      }
    });

    it('uses the publisher URL as attribution when YouVersion has no promotional content', async () => {
      const friend = await enroll('bible-tpt-0001');
      const body = await (await passage(friend, 'TPT', 'Romans 8:1')).json() as Record<string, unknown>;
      expect(body.attribution).toBe('https://example.test/tpt');
      expect(body.provider).toBe('youversion');
    });

    it('serves ESV with the Python format options, the Crossway notice and never caches it', async () => {
      const friend = await enroll('bible-esv-0001');
      for (let round = 0; round < 2; round += 1) {
        const response = await passage(friend, 'ESV', 'John 3:16-17');
        expect(response.status).toBe(200);
        expect(await response.json()).toEqual({
          version: 'ESV',
          reference: 'John 3:16-17',
          verses: [
            { book_id: 'JHN', chapter: 3, verse: 16, text: 'Synthetic ESV verse for John 3:16.' },
            { book_id: 'JHN', chapter: 3, verse: 17, text: 'Synthetic ESV verse for John 3:17.' },
          ],
          copyright: ESV_COPYRIGHT,
          attribution: null,
          provider: 'esv',
          cached: false,
        });
      }
      // Fetched live both times; the cache was neither read nor written.
      expect(verseCalls()).toHaveLength(4);
      expect(cache.match).not.toHaveBeenCalled();
      expect(cache.put).not.toHaveBeenCalled();
      expect(cache.entries.size).toBe(0);
      const first = verseCalls()[0];
      expect(first.host).toBe('api.esv.org');
      expect(first.headers.authorization).toBe(`Token ${ESV_KEY}`);
      expect(Object.fromEntries(first.params)).toEqual({
        q: 'John 3:16',
        'include-passage-references': 'false', 'include-verse-numbers': 'false', 'include-first-verse-numbers': 'false',
        'include-footnotes': 'false', 'include-footnote-body': 'false', 'include-headings': 'false',
        'include-short-copyright': 'false', 'include-copyright': 'false', 'include-passage-horizontal-lines': 'false',
        'include-heading-horizontal-lines': 'false', 'indent-poetry': 'false', 'indent-paragraphs': '0',
        'indent-declares': '0', 'indent-psalm-doxology': '0', 'line-length': '0',
      });
    });

    it('never writes ESV text to the cache even when the cache is the only thing that works', async () => {
      const friend = await enroll('bible-esv-0002');
      await passage(friend, 'esv', 'Psalm 23:1');
      expect([...cache.entries.values()].join(' ')).not.toContain('Synthetic ESV');
      expect(cache.put).not.toHaveBeenCalled();
    });

    it('routes every configured version to the provider named in the operator configuration', () => {
      const providers = Object.fromEntries(Object.entries(VERSION_ROUTES).map(([version, route]) => [version, route.provider]));
      expect(providers).toEqual({
        ESV: 'esv',
        NIV: 'youversion', AMP: 'youversion', BSB: 'youversion', LSV: 'youversion', NIRV: 'youversion',
        NASB1995: 'youversion', NASB2020: 'youversion', TPT: 'youversion',
        NKJV: 'api_bible', NLT: 'api_bible', KJV: 'api_bible', MSG: 'api_bible', WEB: 'api_bible',
      });
      expect(VERSION_ROUTES.NIV.bibleId).toBe('111');
      expect(VERSION_ROUTES.NKJV.bibleId).toBe('63097d2a0a2f7db3-01');
    });

    it('accepts a POST body for long references', async () => {
      const friend = await enroll('bible-post-0001');
      const response = await passagePost(friend, { version: 'kjv', ref: 'Psalm 23:1-2 & 4 & 6' });
      expect(response.status).toBe(200);
      const body = await response.json() as { reference: string; verses: unknown[]; version: string };
      expect(body.reference).toBe('Psalms 23:1-2 & 4 & 6');
      expect(body.version).toBe('KJV');
      expect(body.verses).toHaveLength(4);
      expect((await passagePost(friend, undefined, '{not json')).status).toBe(400);
      expect((await passagePost(friend, { version: 'KJV' })).status).toBe(400);
      expect((await passagePost(friend, undefined, JSON.stringify({ version: 'KJV', ref: 'John 3:16', pad: 'x'.repeat(5000) }))).status).toBe(413);
    });
  });

  describe('caching', () => {
    it('caches non-ESV passages for 30 days and answers the repeat from the cache without upstream calls', async () => {
      const friend = await enroll('bible-cache-0001');
      const first = await (await passage(friend, 'KJV', 'John 3:16-17')).json() as Record<string, unknown>;
      expect(first.cached).toBe(false);
      const callsAfterFirst = upstream.calls.length;
      const stored = [...cache.entries.entries()].filter(([key]) => key.includes('/passage/'));
      expect(stored).toHaveLength(1);
      expect(stored[0][0].startsWith('https://bible-cache.internal/')).toBe(true);
      expect(stored[0][1].cacheControl).toBe(`public, max-age=${30 * 24 * 60 * 60}`);

      const second = await (await passage(friend, 'kjv', 'Jhn 3:16-17')).json() as Record<string, unknown>;
      expect(second.cached).toBe(true);
      expect({ ...second, cached: false }).toEqual(first);
      expect(upstream.calls).toHaveLength(callsAfterFirst);
    });

    it('does not mix up versions or references in the cache', async () => {
      const friend = await enroll('bible-cache-0002');
      await passage(friend, 'KJV', 'John 3:16');
      const other = await (await passage(friend, 'NKJV', 'John 3:16')).json() as Record<string, unknown>;
      expect(other.cached).toBe(false);
      const another = await (await passage(friend, 'KJV', 'John 3:17')).json() as Record<string, unknown>;
      expect(another.cached).toBe(false);
    });

    it('still answers when the cache is missing or throws', async () => {
      const friend = await enroll('bible-cache-0003');
      cache.match.mockRejectedValue(new Error('cache down'));
      cache.put.mockRejectedValue(new Error('cache down'));
      expect((await passage(friend, 'WEB', 'Genesis 1:1')).status).toBe(200);
      vi.stubGlobal('caches', undefined);
      const body = await (await passage(friend, 'WEB', 'Genesis 1:2')).json() as Record<string, unknown>;
      expect(body.cached).toBe(false);
    });
  });

  describe('secrets', () => {
    it.each([
      ['NKJV', 'API_BIBLE_API_KEY'],
      ['NIV', 'YOUVERSION_API_KEY'],
      ['ESV', 'ESV_API_KEY'],
    ])('answers 503 provider_not_configured for %s when %s is missing, empty or blank', async (version, secret) => {
      const friend = await enroll(`bible-secret-${version}`);
      for (const value of [undefined, '', '   ']) {
        useEnv({ [secret]: value });
        const response = await passage(friend, version, 'John 3:16');
        expect(response.status).toBe(503);
        expect(response.headers.get('retry-after')).toBe('300');
        const body = await response.json() as Record<string, unknown>;
        expect(body.error).toBe('provider_not_configured');
        expectClean(JSON.stringify(body));
      }
      expect(upstream.calls).toHaveLength(0);
    });

    it('serves the other providers when only one secret is missing', async () => {
      useEnv({ ESV_API_KEY: undefined });
      const friend = await enroll('bible-secret-partial');
      expect((await passage(friend, 'ESV', 'John 3:16')).status).toBe(503);
      expect((await passage(friend, 'KJV', 'John 3:16')).status).toBe(200);
      expect((await passage(friend, 'NIV', 'John 3:16')).status).toBe(200);
    });

    it('never puts a key or verse text in any response or log line, including failures', async () => {
      const friend = await enroll('bible-canary-0001');
      const bodies: string[] = [];
      const record = async (response: Response): Promise<void> => { bodies.push(await response.text(), [...response.headers.values()].join(' ')); };
      const served: string[] = [];

      for (const [version, ref] of [['NKJV', 'John 3:16'], ['NIV', 'John 3:16'], ['ESV', 'John 3:16']]) {
        const response = await passage(friend, version, ref);
        served.push(await response.text(), [...response.headers.values()].join(' '));
      }
      // Upstream failures whose own bodies carry the key and a verse.
      const hostile = `UPSTREAM-BODY ${API_BIBLE_KEY} ${YOUVERSION_KEY} ${ESV_KEY} Synthetic ESV verse`;
      for (const [index, status] of [401, 403, 404, 429, 500, 503].entries()) {
        upstream.override = () => new Response(hostile, { status });
        for (const version of ['NKJV', 'NIV', 'ESV']) await record(await passage(friend, version, `Acts 2:${index + 1}`));
      }
      upstream.override = () => { throw new TypeError(`network down ${API_BIBLE_KEY}`); };
      await record(await passage(friend, 'KJV', 'Mark 1:1'));
      upstream.override = () => new Response(`{"content": "${ESV_KEY}"`, { status: 200 });
      await record(await passage(friend, 'MSG', 'Mark 1:2'));
      await record(await passage(friend, 'NKJV', '../../etc/passwd 1:1'));
      await record(await passage(friend, 'https://evil.example/', 'John 3:16'));

      // Successful answers carry the text and nothing else; everything that
      // went wrong carries neither a key, an upstream body nor any verse text.
      const everything = bodies.join('\n');
      expect(everything).not.toContain('UPSTREAM-BODY');
      expect(everything).not.toContain(hostile);
      expect(everything).not.toMatch(/Synthetic (API|YouVersion|ESV)/);
      expectClean(everything, ...served);
      expect(logged.join('\n')).not.toMatch(/Synthetic (API|YouVersion|ESV)/);
      expect(logged.join('\n')).not.toContain('UPSTREAM-BODY');
    });

    it('never stores a key in the Registry storage', async () => {
      const friend = await enroll('bible-canary-0002');
      await passage(friend, 'ESV', 'John 3:16');
      await passage(friend, 'NKJV', 'John 3:16');
      const stored = JSON.stringify([...storage.values.entries()]);
      expectClean(stored);
      expect(stored).not.toContain('Synthetic');
    });
  });

  describe('validation', () => {
    it.each(['XYZ', '', 'N IV', '../v1/bibles', 'https://evil.example/x', 'toString', '__proto__', 'constructor'])(
      'refuses the version %j before any upstream call', async (version) => {
        const friend = await enroll('bible-version-0001');
        const response = await passage(friend, version, 'John 3:16');
        expect(response.status).toBe(400);
        expect((await response.json() as { error: string }).error).toBe('invalid_version');
        expect(upstream.calls).toHaveLength(0);
      },
    );

    it.each([
      ['Frodo 3:16', 'invalid_reference'],
      ['John', 'invalid_reference'],
      ['John 3', 'invalid_reference'],
      ['', 'invalid_reference'],
      ['John 0:1', 'invalid_reference'],
      ['John 22:1', 'invalid_reference'],
      ['John 3:0', 'invalid_reference'],
      ['John 3:999', 'invalid_reference'],
      ['John 3:18-16', 'invalid_reference'],
      ['John 3:16 & 3:16', 'invalid_reference'],
      ['John 3:16-18 & 17', 'invalid_reference'],
      ['John 3:16 NIV', 'invalid_reference'],
      ['https://evil.example/ 3:16', 'invalid_reference'],
      ['John 3:16/../../x', 'invalid_reference'],
      ['John 3:16-4:2', 'unsupported_reference'],
      ['Psalms 119:1-41', 'too_many_verses'],
      [`John 3:1-20 & 21-${20 + 21}`, 'too_many_verses'],
      ['x'.repeat(600), 'invalid_reference'],
    ])('refuses the reference %j with %s and makes no upstream call', async (ref, code) => {
      const friend = await enroll('bible-ref-0001');
      const response = await passage(friend, 'NKJV', ref);
      expect(response.status).toBe(400);
      expect((await response.json() as { error: string }).error).toBe(code);
      expect(upstream.calls).toHaveLength(0);
    });

    it('answers a missing version or ref with 400 and an unsupported method with 405', async () => {
      const friend = await enroll('bible-input-0001');
      const bare = await worker.fetch(new Request(`https://control.example.com/v1/installations/${friend.id}/bible/passage`, {
        headers: { authorization: `Bearer ${friend.credential}` },
      }), workerEnv as never);
      expect(bare.status).toBe(400);
      const put = await worker.fetch(new Request(`https://control.example.com/v1/installations/${friend.id}/bible/passage?version=KJV&ref=John%203:16`, {
        method: 'PUT', headers: { authorization: `Bearer ${friend.credential}` },
      }), workerEnv as never);
      expect(put.status).toBe(405);
    });

    it('serves exactly 40 verses and refuses 41', async () => {
      const friend = await enroll('bible-cap-0001');
      const ok = await passage(friend, 'WEB', 'Psalms 119:1-40');
      expect(ok.status).toBe(200);
      expect((await ok.json() as { verses: unknown[] }).verses).toHaveLength(MAX_VERSES_PER_REQUEST);
      const callsBefore = upstream.calls.length;
      const refused = await passage(friend, 'WEB', 'Psalms 119:1-41');
      expect(refused.status).toBe(400);
      expect((await refused.json() as { error: string }).error).toBe('too_many_verses');
      expect(upstream.calls).toHaveLength(callsBefore);
    });

    it('parses VerseVision normalized references', () => {
      const cases: Array<[string, string, number]> = [
        ['John 3:16-18 & 20', 'John 3:16-18 & 20', 4],
        ['1 John 4:7, 8', '1 John 4:7 & 8', 2],
        ['Ps 23:1-6', 'Psalms 23:1-6', 6],
        ['song of songs 2:1', 'Song of Solomon 2:1', 1],
        ['1 Sam 3:4 & 4:2', '1 Samuel 3:4 & 4:2', 2],
        ['JHN 3:16', 'John 3:16', 1],
        ['II Kings 2:11; 13', '2 Kings 2:11 & 13', 2],
        ['Romans 8:28\u201330', 'Romans 8:28-30', 3],
      ];
      for (const [input, normalized, count] of cases) {
        const parsed = parseReference(input);
        expect(parsed.normalized).toBe(normalized);
        expect(parsed.verses).toHaveLength(count);
      }
    });
  });

  describe('authentication', () => {
    it('answers 401 for no credential, a wrong credential and an unknown installation', async () => {
      const friend = await enroll('bible-auth-0001');
      expect((await passage(undefined, 'KJV', 'John 3:16', friend.id)).status).toBe(401);
      expect((await passage({ id: friend.id, credential: `${friend.credential}x` }, 'KJV', 'John 3:16')).status).toBe(401);
      expect((await passage(friend, 'KJV', 'John 3:16', 'deadbeef')).status).toBe(401);
      const other = await enroll('bible-auth-0002');
      expect((await passage({ id: friend.id, credential: other.credential }, 'KJV', 'John 3:16')).status).toBe(401);
      expect(upstream.calls).toHaveLength(0);
    });

    it('answers 401 for a revoked installation, before any upstream call', async () => {
      const friend = await enroll('bible-auth-0003');
      expect((await passage(friend, 'KJV', 'John 3:16')).status).toBe(200);
      const revoke = await registry.fetch(new Request(`https://control.example.com/v1/installations/${friend.id}/revoke`, {
        method: 'POST', headers: { authorization: `Bearer ${friend.credential}` },
      }));
      expect(revoke.status).toBe(200);
      const callsBefore = upstream.calls.length;
      const refused = await passage(friend, 'NIV', 'John 3:16');
      expect(refused.status).toBe(401);
      expect(await refused.json()).toEqual({ error: 'unauthorized' });
      expect(upstream.calls).toHaveLength(callsBefore);
    });

    it('does not let the network reach the Registry gate paths or other bible paths', async () => {
      const friend = await enroll('bible-auth-0004');
      for (const action of ['reserve', 'slot', 'passages', 'anything']) {
        const response = await worker.fetch(new Request(`https://control.example.com/v1/installations/${friend.id}/bible/${action}`, {
          method: 'POST', headers: { authorization: `Bearer ${friend.credential}` },
        }), workerEnv as never);
        expect(response.status).toBe(404);
      }
      expect(upstream.calls).toHaveLength(0);
    });
  });

  describe('limits', () => {
    it('limits each installation per hour, restarts the window, and isolates two installations', async () => {
      useEnv({ BIBLE_INSTALLATION_HOURLY_LIMIT: '3' });
      const one = await enroll('bible-limit-0001');
      const two = await enroll('bible-limit-0002');
      let now = 1_900_000_000_000;
      vi.spyOn(Date, 'now').mockImplementation(() => now);
      for (let n = 1; n <= 3; n += 1) expect((await passage(one, 'KJV', `John 3:${n}`)).status).toBe(200);
      const limited = await passage(one, 'KJV', 'John 3:4');
      expect(limited.status).toBe(429);
      expect(Number(limited.headers.get('retry-after'))).toBeGreaterThan(0);
      expect(Number(limited.headers.get('retry-after'))).toBeLessThanOrEqual(3600);
      const body = await limited.json() as Record<string, unknown>;
      expect(body.error).toBe('rate_limited');
      expect(body.scope).toBe('installation_hourly');
      // Cached passages count too: the limit is on requests, not upstream calls.
      expect((await passage(one, 'KJV', 'John 3:1')).status).toBe(429);
      // The other installation has its own allowance.
      for (let n = 1; n <= 3; n += 1) expect((await passage(two, 'KJV', `John 3:${n}`)).status).toBe(200);
      expect((await passage(two, 'KJV', 'John 3:9')).status).toBe(429);
      now += 3601 * 1000;
      expect((await passage(one, 'KJV', 'John 3:5')).status).toBe(200);
    });

    it('limits each installation per day', async () => {
      useEnv({ BIBLE_INSTALLATION_HOURLY_LIMIT: '100', BIBLE_INSTALLATION_DAILY_LIMIT: '2' });
      const friend = await enroll('bible-limit-0003');
      let now = 1_900_000_000_000;
      vi.spyOn(Date, 'now').mockImplementation(() => now);
      expect((await passage(friend, 'KJV', 'John 3:1')).status).toBe(200);
      now += 4000 * 1000;
      expect((await passage(friend, 'KJV', 'John 3:2')).status).toBe(200);
      now += 4000 * 1000;
      const limited = await passage(friend, 'KJV', 'John 3:3');
      expect(limited.status).toBe(429);
      expect((await limited.json() as { scope: string }).scope).toBe('installation_daily');
      expect(Number(limited.headers.get('retry-after'))).toBeGreaterThan(3600);
      now += 80_000 * 1000;
      expect((await passage(friend, 'KJV', 'John 3:3')).status).toBe(200);
    });

    it('stops all installations at the global daily cap with 429 and Retry-After, counting only upstream fetches', async () => {
      useEnv({ BIBLE_GLOBAL_DAILY_CAP: '2' });
      const one = await enroll('bible-global-0001');
      const two = await enroll('bible-global-0002');
      let now = 1_900_000_000_000;
      vi.spyOn(Date, 'now').mockImplementation(() => now);
      expect((await passage(one, 'KJV', 'John 3:1')).status).toBe(200);
      expect((await passage(two, 'NIV', 'John 3:2')).status).toBe(200);
      // A cached passage costs no upstream call, so the cap does not apply.
      const hit = await passage(two, 'KJV', 'John 3:1');
      expect(hit.status).toBe(200);
      expect((await hit.json() as { cached: boolean }).cached).toBe(true);
      const callsBefore = upstream.calls.length;
      const capped = await passage(one, 'KJV', 'John 3:3');
      expect(capped.status).toBe(429);
      expect(Number(capped.headers.get('retry-after'))).toBeGreaterThan(0);
      expect((await capped.json() as { scope: string }).scope).toBe('global_daily');
      expect((await passage(two, 'ESV', 'John 3:4')).status).toBe(429);
      expect(upstream.calls).toHaveLength(callsBefore);
      now += 86_401 * 1000;
      expect((await passage(one, 'KJV', 'John 3:3')).status).toBe(200);
    });

    it('uses 600 per hour, 5,000 per day and a 20,000 global cap by default, and ignores invalid overrides', async () => {
      expect(positiveInteger(undefined, 600)).toBe(600);
      expect(positiveInteger('', 600)).toBe(600);
      expect(positiveInteger('0', 600)).toBe(600);
      expect(positiveInteger('-5', 600)).toBe(600);
      expect(positiveInteger('1e3', 600)).toBe(600);
      expect(positiveInteger('abc', 600)).toBe(600);
      expect(positiveInteger(' 25 ', 600)).toBe(25);
      useEnv({ BIBLE_INSTALLATION_HOURLY_LIMIT: 'nonsense' });
      const friend = await enroll('bible-limit-0004');
      const record = storage.values.get(`installation:${friend.id}`) as Record<string, unknown>;
      storage.values.set(`installation:${friend.id}`, { ...record, bibleHourRate: { startedAt: Math.floor(Date.now() / 1000), count: 599 } });
      expect((await passage(friend, 'KJV', 'John 3:1')).status).toBe(200);
      expect((await passage(friend, 'KJV', 'John 3:2')).status).toBe(429);
    });
  });

  describe('upstream failures', () => {
    it('answers 504 on a timeout and uses the configured timeout signal', async () => {
      const friend = await enroll('bible-fail-0001');
      upstream.override = () => {
        throw new DOMException('The operation timed out', 'TimeoutError');
      };
      const timeout = vi.spyOn(AbortSignal, 'timeout');
      useEnv({ BIBLE_UPSTREAM_TIMEOUT_SECONDS: '7' });
      const response = await passage(friend, 'ESV', 'John 3:16');
      expect(response.status).toBe(504);
      expect((await response.json() as { error: string }).error).toBe('upstream_timeout');
      expect(timeout).toHaveBeenCalledWith(7000);
    });

    it('defaults the upstream timeout to 15 seconds', async () => {
      const friend = await enroll('bible-fail-0002');
      const timeout = vi.spyOn(AbortSignal, 'timeout');
      await passage(friend, 'ESV', 'John 3:16');
      expect(timeout).toHaveBeenCalledWith(15_000);
    });

    it('turns an upstream error, rate limit, bad JSON, empty text or an unknown Bible id into a generic answer', async () => {
      const friend = await enroll('bible-fail-0003');
      upstream.override = () => new Response('SECRET-UPSTREAM-BODY', { status: 500 });
      let response = await passage(friend, 'NKJV', 'John 3:16');
      expect(response.status).toBe(502);
      expect(await response.json()).toEqual({ error: 'upstream_error', message: 'the Bible provider could not supply this passage' });

      upstream.override = () => new Response('SECRET-UPSTREAM-BODY', { status: 429 });
      response = await passage(friend, 'NIV', 'John 3:16');
      expect(response.status).toBe(503);
      expect(response.headers.get('retry-after')).toBe('60');
      expect((await response.json() as { error: string }).error).toBe('upstream_busy');

      upstream.override = () => new Response('<html>not json</html>', { status: 200 });
      expect((await passage(friend, 'ESV', 'John 3:16')).status).toBe(502);

      upstream.override = (call) => call.path.includes('/passage/text')
        ? new Response(JSON.stringify({ passages: ['   '] }))
        : undefined;
      expect((await passage(friend, 'ESV', 'John 3:17')).status).toBe(502);

      upstream.override = (call) => call.path === '/v1/bibles'
        ? new Response(JSON.stringify({ data: [] }))
        : undefined;
      expect((await passage(friend, 'NKJV', 'John 3:18')).status).toBe(502);

      expect(logged.join('\n')).not.toContain('SECRET-UPSTREAM-BODY');
      expect(cache.entries.size).toBe(0);
    });

    it('does not cache a failed passage and recovers on the next request', async () => {
      const friend = await enroll('bible-fail-0004');
      upstream.override = () => new Response('', { status: 500 });
      expect((await passage(friend, 'KJV', 'John 3:16')).status).toBe(502);
      upstream.override = undefined;
      const body = await (await passage(friend, 'KJV', 'John 3:16')).json() as { cached: boolean };
      expect(body.cached).toBe(false);
    });

    it('does not refuse a Bible whose metadata call fails when it is already cached', async () => {
      const friend = await enroll('bible-fail-0005');
      expect((await passage(friend, 'KJV', 'John 3:16')).status).toBe(200);
      upstream.override = () => new Response('', { status: 500 });
      expect((await passage(friend, 'KJV', 'John 3:16')).status).toBe(200);
    });

    it('does not hold the Registry while it waits for an upstream answer', async () => {
      const friend = await enroll('bible-fail-0006');
      let release: (() => void) | undefined;
      upstream.override = (call) => call.path.includes('/passage/text')
        ? new Promise<Response>((resolve) => {
          release = () => resolve(new Response(JSON.stringify({ passages: ['Synthetic ESV slow verse.'] })));
        }) as unknown as Response
        : undefined;
      const pending = passage(friend, 'ESV', 'John 3:16');
      for (let spin = 0; spin < 50 && !release; spin += 1) await new Promise((resolve) => setTimeout(resolve, 1));
      expect(release).toBeDefined();
      // A check-in-style call through the Registry still completes meanwhile.
      const status = await registry.fetch(new Request(`https://control.example.com/v1/installations/${friend.id}/status`, {
        headers: { authorization: `Bearer ${friend.credential}` },
      }));
      expect(status.status).toBe(200);
      release?.();
      expect((await pending).status).toBe(200);
    });
  });

  describe('markup', () => {
    it('reads text like BeautifulSoup get_text(" ", strip=True)', () => {
      expect(htmlToText('<p class="p"><span class="v">[16]</span> For God <span>so</span> loved,</p>')).toBe('[16] For God so loved,');
      expect(htmlToText('plain &amp; simple &#8212; &lt;ok&gt;&nbsp;&#x41;')).toBe('plain & simple \u2014 <ok>\u00a0A'.trim());
      expect(htmlToText('<style>p{}</style><p>kept</p><!-- gone --><script>alert(1)</script>')).toBe('kept');
      expect(htmlToText('')).toBe('');
      expect(htmlToText('  <p>  </p> ')).toBe('');
    });
  });
});
