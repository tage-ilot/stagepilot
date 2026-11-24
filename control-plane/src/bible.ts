// Bible text proxy: VerseVision fetches verse text through this Worker with the
// operator's provider keys, which exist ONLY as Worker secrets (API_BIBLE_API_KEY,
// YOUVERSION_API_KEY, ESV_API_KEY). An installation never sees a key, and no key,
// upstream body or passage text is ever written to a log or an error. The provider
// calls mirror VerseVision's Python providers (bibles/api_bible.py, youversion.py,
// esv.py): one upstream request per verse, the same query options, the same
// markup stripping. The ESV is never cached or stored (Crossway terms).
//
// The Registry Durable Object owns authentication and the counters (it answers
// POST .../bible/reserve and .../bible/slot, see index.ts). Everything slow, the
// upstream calls, runs HERE, outside the Registry's serialised section, so a
// slow Bible provider can never stall enrollment or check-ins.

export interface BibleEnv {
  API_BIBLE_API_KEY?: string;
  YOUVERSION_API_KEY?: string;
  ESV_API_KEY?: string;
  // Optional: seconds to wait for one upstream request (1-30, default 15).
  BIBLE_UPSTREAM_TIMEOUT_SECONDS?: string;
}

export interface RegistryStub {
  fetch(request: Request): Promise<Response>;
}

// The slice of the Cache API used here, so tests can pass a recording fake.
export interface BibleCache {
  match(request: Request): Promise<Response | undefined>;
  put(request: Request, response: Response): Promise<void>;
}

export type ProviderName = 'api_bible' | 'youversion' | 'esv';

export const MAX_VERSES_PER_REQUEST = 40;
export const CACHE_TTL_SECONDS = 30 * 24 * 60 * 60;
const META_TTL_SECONDS = 7 * 24 * 60 * 60;
const MAX_REFERENCE_CHARS = 500;
const MAX_BODY_BYTES = 4096;
const MAX_VERSE_NUMBER = 200;
const UPSTREAM_CONCURRENCY = 6;
const DEFAULT_UPSTREAM_TIMEOUT_SECONDS = 15;
const CACHE_ORIGIN = 'https://bible-cache.internal';
const CACHE_SCHEMA = 'v1';

const PROVIDER_BASE_URL: Readonly<Record<ProviderName, string>> = {
  api_bible: 'https://api.scripture.api.bible/v1',
  youversion: 'https://api.youversion.com/v1',
  esv: 'https://api.esv.org/v3',
};

const PROVIDER_SECRET: Readonly<Record<ProviderName, 'API_BIBLE_API_KEY' | 'YOUVERSION_API_KEY' | 'ESV_API_KEY'>> = {
  api_bible: 'API_BIBLE_API_KEY',
  youversion: 'YOUVERSION_API_KEY',
  esv: 'ESV_API_KEY',
};

interface VersionRoute {
  provider: ProviderName;
  // The provider's own Bible id (not a secret). The ESV needs none.
  bibleId?: string;
}

// Which provider serves which version, with the provider Bible ids, exactly as
// in the operator's VerseVision configuration ([bible.version_routes] and
// [bible.providers.*.bible_ids]). Anything not listed here is refused, so a
// request can never name a free-form upstream resource.
export const VERSION_ROUTES: Readonly<Record<string, VersionRoute>> = Object.freeze({
  ESV: { provider: 'esv' },
  NIV: { provider: 'youversion', bibleId: '111' },
  AMP: { provider: 'youversion', bibleId: '1588' },
  BSB: { provider: 'youversion', bibleId: '3034' },
  LSV: { provider: 'youversion', bibleId: '2660' },
  NIRV: { provider: 'youversion', bibleId: '110' },
  NASB1995: { provider: 'youversion', bibleId: '100' },
  NASB2020: { provider: 'youversion', bibleId: '2692' },
  TPT: { provider: 'youversion', bibleId: '1849' },
  NKJV: { provider: 'api_bible', bibleId: '63097d2a0a2f7db3-01' },
  NLT: { provider: 'api_bible', bibleId: 'd6e14a625393b4da-01' },
  KJV: { provider: 'api_bible', bibleId: 'de4e12af7f28f599-01' },
  MSG: { provider: 'api_bible', bibleId: '6f11a7de016f942e-01' },
  WEB: { provider: 'api_bible', bibleId: '9879dbb7cfe39e4d-01' },
});

export const ESV_COPYRIGHT =
  'Scripture quotations are from the ESV\u00ae Bible (The Holy Bible, English Standard Version\u00ae), '
  + '\u00a9 2001 by Crossway, a publishing ministry of Good News Publishers. Used by permission. All rights reserved. '
  + 'www.esv.org';

// ---------------------------------------------------------------- books

type BookRow = readonly [string, string, number, readonly string[]];

// id, name, chapter count, aliases: VerseVision's references/books.py.
const BOOK_ROWS: readonly BookRow[] = [
  ['GEN', 'Genesis', 50, ['Gen']],
  ['EXO', 'Exodus', 40, ['Exod', 'Ex']],
  ['LEV', 'Leviticus', 27, ['Lev']],
  ['NUM', 'Numbers', 36, ['Num']],
  ['DEU', 'Deuteronomy', 34, ['Deut', 'Dt']],
  ['JOS', 'Joshua', 24, ['Josh']],
  ['JDG', 'Judges', 21, ['Judg', 'Jdgs']],
  ['RUT', 'Ruth', 4, []],
  ['1SA', '1 Samuel', 31, ['1 Sam', 'I Samuel', 'First Samuel']],
  ['2SA', '2 Samuel', 24, ['2 Sam', 'II Samuel', 'Second Samuel']],
  ['1KI', '1 Kings', 22, ['1 Kgs', 'I Kings', 'First Kings']],
  ['2KI', '2 Kings', 25, ['2 Kgs', 'II Kings', 'Second Kings']],
  ['1CH', '1 Chronicles', 29, ['1 Chron', 'I Chronicles', 'First Chronicles']],
  ['2CH', '2 Chronicles', 36, ['2 Chron', 'II Chronicles', 'Second Chronicles']],
  ['EZR', 'Ezra', 10, []],
  ['NEH', 'Nehemiah', 13, ['Neh']],
  ['EST', 'Esther', 10, ['Esth']],
  ['JOB', 'Job', 42, []],
  ['PSA', 'Psalms', 150, ['Psalm', 'Ps', 'Psa']],
  ['PRO', 'Proverbs', 31, ['Prov', 'Pr']],
  ['ECC', 'Ecclesiastes', 12, ['Eccl', 'Ecc']],
  ['SNG', 'Song of Solomon', 8, ['Song of Songs', 'Song', 'SOS', 'Canticles']],
  ['ISA', 'Isaiah', 66, ['Isa']],
  ['JER', 'Jeremiah', 52, ['Jer']],
  ['LAM', 'Lamentations', 5, ['Lam']],
  ['EZK', 'Ezekiel', 48, ['Ezek']],
  ['DAN', 'Daniel', 12, ['Dan']],
  ['HOS', 'Hosea', 14, ['Hos']],
  ['JOL', 'Joel', 3, []],
  ['AMO', 'Amos', 9, []],
  ['OBA', 'Obadiah', 1, ['Obad']],
  ['JON', 'Jonah', 4, ['Jon']],
  ['MIC', 'Micah', 7, ['Mic']],
  ['NAM', 'Nahum', 3, ['Nah']],
  ['HAB', 'Habakkuk', 3, ['Hab']],
  ['ZEP', 'Zephaniah', 3, ['Zeph']],
  ['HAG', 'Haggai', 2, ['Hag']],
  ['ZEC', 'Zechariah', 14, ['Zech']],
  ['MAL', 'Malachi', 4, ['Mal']],
  ['MAT', 'Matthew', 28, ['Matt', 'Mt']],
  ['MRK', 'Mark', 16, ['Mk', 'Mrk']],
  ['LUK', 'Luke', 24, ['Lk']],
  ['JHN', 'John', 21, ['Jn', 'Jhn']],
  ['ACT', 'Acts', 28, ['Ac']],
  ['ROM', 'Romans', 16, ['Rom']],
  ['1CO', '1 Corinthians', 16, ['1 Cor', 'I Corinthians', 'First Corinthians']],
  ['2CO', '2 Corinthians', 13, ['2 Cor', 'II Corinthians', 'Second Corinthians']],
  ['GAL', 'Galatians', 6, ['Gal']],
  ['EPH', 'Ephesians', 6, ['Eph']],
  ['PHP', 'Philippians', 4, ['Phil', 'Php']],
  ['COL', 'Colossians', 4, ['Col']],
  ['1TH', '1 Thessalonians', 5, ['1 Thess', 'I Thessalonians', 'First Thessalonians']],
  ['2TH', '2 Thessalonians', 3, ['2 Thess', 'II Thessalonians', 'Second Thessalonians']],
  ['1TI', '1 Timothy', 6, ['1 Tim', 'I Timothy', 'First Timothy']],
  ['2TI', '2 Timothy', 4, ['2 Tim', 'II Timothy', 'Second Timothy']],
  ['TIT', 'Titus', 3, ['Tit']],
  ['PHM', 'Philemon', 1, ['Phlm']],
  ['HEB', 'Hebrews', 13, ['Heb']],
  ['JAS', 'James', 5, ['Jas', 'Jm']],
  ['1PE', '1 Peter', 5, ['1 Pet', 'I Peter', 'First Peter']],
  ['2PE', '2 Peter', 3, ['2 Pet', 'II Peter', 'Second Peter']],
  ['1JN', '1 John', 5, ['1 Jn', 'I John', 'First John']],
  ['2JN', '2 John', 1, ['2 Jn', 'II John', 'Second John']],
  ['3JN', '3 John', 1, ['3 Jn', 'III John', 'Third John']],
  ['JUD', 'Jude', 1, ['Jud']],
  ['REV', 'Revelation', 22, ['Rev', 'The Revelation']],
];

interface Book {
  id: string;
  name: string;
  chapters: number;
}

function bookKey(value: string): string {
  return value.toLowerCase().replace(/\./g, '').replace(/\s+/g, ' ').trim();
}

const BOOK_LOOKUP = new Map<string, Book>();
for (const [id, name, chapters, aliases] of BOOK_ROWS) {
  const book = { id, name, chapters };
  BOOK_LOOKUP.set(bookKey(id), book);
  for (const label of [name, ...aliases]) BOOK_LOOKUP.set(bookKey(label), book);
}

// ---------------------------------------------------------------- references

export interface VersePoint {
  chapter: number;
  verse: number;
}

export interface VerseSpan {
  start: VersePoint;
  end?: VersePoint;
}

export interface ParsedReference {
  book: Book;
  spans: VerseSpan[];
  verses: VersePoint[];
  // VerseVision's normalized form without the version: "John 3:16-18 & 20".
  normalized: string;
}

export class BibleReferenceError extends Error {
  constructor(
    readonly code: 'invalid_reference' | 'unsupported_reference' | 'too_many_verses',
    message: string,
  ) {
    super(message);
  }
}

const SPAN_PATTERN = /^(?:(\d{1,3}):)?(\d{1,3})(?:\s*[-\u2013\u2014]\s*(?:(\d{1,3}):)?(\d{1,3}))?$/;

// Parses VerseVision's normalized reference, "John 3:16-18" or "John 3:16-18 & 20 & 4:2".
// Later spans may omit the chapter (it is the previous span's chapter), and "," or
// ";" are accepted in place of "&". A whole chapter, a version suffix or a range
// across chapters is refused: the same limits as the Python providers.
export function parseReference(text: string): ParsedReference {
  if (typeof text !== 'string' || text.trim().length === 0 || text.length > MAX_REFERENCE_CHARS) {
    throw new BibleReferenceError('invalid_reference', 'reference is missing or too long');
  }
  const split = text.trim().match(/^(.+?)\s+(\d{1,3}\s*:.*)$/s);
  if (!split) throw new BibleReferenceError('invalid_reference', 'reference needs a book, a chapter and verses');
  const book = BOOK_LOOKUP.get(bookKey(split[1]));
  if (!book) throw new BibleReferenceError('invalid_reference', 'unknown book');
  const pieces = split[2].split(/\s*[&,;]\s*/).map((piece) => piece.trim());
  const spans: VerseSpan[] = [];
  let chapter: number | undefined;
  for (const piece of pieces) {
    const match = piece.match(SPAN_PATTERN);
    if (!match) throw new BibleReferenceError('invalid_reference', 'verses are not written as chapter:verse');
    const startChapter = match[1] !== undefined ? Number(match[1]) : chapter;
    if (startChapter === undefined) throw new BibleReferenceError('invalid_reference', 'the first verse needs a chapter');
    const start: VersePoint = { chapter: startChapter, verse: Number(match[2]) };
    let end: VersePoint | undefined;
    if (match[4] !== undefined) {
      end = { chapter: match[3] !== undefined ? Number(match[3]) : startChapter, verse: Number(match[4]) };
    }
    spans.push(end ? { start, end } : { start });
    chapter = startChapter;
  }
  const verses: VersePoint[] = [];
  let previousEnd: VersePoint | undefined;
  for (const span of spans) {
    const end = span.end ?? span.start;
    for (const point of [span.start, end]) {
      if (point.chapter < 1 || point.chapter > book.chapters) {
        throw new BibleReferenceError('invalid_reference', `${book.name} has no chapter ${point.chapter}`);
      }
      if (point.verse < 1 || point.verse > MAX_VERSE_NUMBER) {
        throw new BibleReferenceError('invalid_reference', `${book.name} ${point.chapter} has no verse ${point.verse}`);
      }
    }
    if (end.chapter !== span.start.chapter) {
      throw new BibleReferenceError('unsupported_reference', 'a range across chapters is not supported; ask for each chapter separately');
    }
    if (end.verse < span.start.verse) throw new BibleReferenceError('invalid_reference', 'range end precedes range start');
    if (previousEnd && (span.start.chapter < previousEnd.chapter
      || (span.start.chapter === previousEnd.chapter && span.start.verse <= previousEnd.verse))) {
      throw new BibleReferenceError('invalid_reference', 'duplicate or overlapping spans');
    }
    for (let verse = span.start.verse; verse <= end.verse; verse += 1) {
      verses.push({ chapter: span.start.chapter, verse });
      if (verses.length > MAX_VERSES_PER_REQUEST) {
        throw new BibleReferenceError('too_many_verses', `at most ${MAX_VERSES_PER_REQUEST} verses per request`);
      }
    }
    previousEnd = end;
  }
  return { book, spans, verses, normalized: formatReference(book, spans) };
}

function formatReference(book: Book, spans: VerseSpan[]): string {
  const pieces: string[] = [];
  let chapter: number | undefined;
  for (const span of spans) {
    const start = chapter === span.start.chapter ? String(span.start.verse) : `${span.start.chapter}:${span.start.verse}`;
    pieces.push(span.end ? `${start}-${span.end.verse}` : start);
    chapter = span.start.chapter;
  }
  return `${book.name} ${pieces.join(' & ')}`;
}

// ---------------------------------------------------------------- upstream

class UpstreamError extends Error {
  constructor(
    readonly kind: 'timeout' | 'network' | 'status' | 'rate_limited' | 'invalid' | 'empty' | 'unavailable',
    readonly status?: number,
  ) {
    // The message is a fixed word: never an upstream body, URL or header.
    super(kind);
  }
}

interface VersionMeta {
  copyright: string | null;
  attribution: string | null;
}

interface NormalizedVerse {
  book_id: string;
  chapter: number;
  verse: number;
  text: string;
}

interface PassageResult {
  verses: NormalizedVerse[];
  copyright: string | null;
  attribution: string | null;
}

async function upstreamJson(url: URL, headers: Record<string, string>, timeoutMs: number): Promise<unknown> {
  try {
    const response = await fetch(url.toString(), {
      method: 'GET',
      headers,
      redirect: 'error',
      signal: AbortSignal.timeout(timeoutMs),
    });
    if (!response.ok) {
      // The body is never read or echoed.
      await response.body?.cancel().catch(() => undefined);
      throw new UpstreamError(response.status === 429 ? 'rate_limited' : 'status', response.status);
    }
    return await response.json();
  } catch (error) {
    if (error instanceof UpstreamError) throw error;
    const name = error instanceof Error ? error.name : '';
    if (name === 'TimeoutError' || name === 'AbortError') throw new UpstreamError('timeout');
    if (error instanceof SyntaxError) throw new UpstreamError('invalid');
    throw new UpstreamError('network');
  }
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

function asText(value: unknown): string | null {
  return typeof value === 'string' && value.length > 0 ? value : null;
}

const NAMED_ENTITIES: Readonly<Record<string, string>> = {
  amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: '\u00a0', mdash: '\u2014', ndash: '\u2013',
  ldquo: '\u201c', rdquo: '\u201d', lsquo: '\u2018', rsquo: '\u2019', hellip: '\u2026',
};

function decodeEntities(text: string): string {
  return text.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (whole, name: string) => {
    if (name[0] === '#') {
      const code = name[1].toLowerCase() === 'x' ? parseInt(name.slice(2), 16) : parseInt(name.slice(1), 10);
      return Number.isFinite(code) && code > 0 && code <= 0x10ffff ? String.fromCodePoint(code) : whole;
    }
    return NAMED_ENTITIES[name.toLowerCase()] ?? whole;
  });
}

// BeautifulSoup(html, "html.parser").get_text(" ", strip=True): every text node
// trimmed, empty ones dropped, the rest joined with one space. Comments, script
// and style content are not text.
export function htmlToText(html: string): string {
  const withoutHidden = html
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/<(script|style)\b[\s\S]*?<\/\1\s*>/gi, '');
  return withoutHidden
    .split(/<[^>]*>/)
    .map((node) => decodeEntities(node).trim())
    .filter((node) => node.length > 0)
    .join(' ');
}

const LEADING_VERSE_MARKER = /^\[\d+(?:-\d+)?\]\s*/;

function collapse(text: string): string {
  return text.split(/\s+/).filter(Boolean).join(' ');
}

function verseId(book: Book, point: VersePoint): string {
  return `${book.id}.${point.chapter}.${point.verse}`;
}

function url(base: string, path: string, params: Record<string, string>): URL {
  const target = new URL(base + path);
  for (const [key, value] of Object.entries(params)) target.searchParams.set(key, value);
  return target;
}

interface ProviderContext {
  key: string;
  timeoutMs: number;
  // Cached lookup of the Bible's copyright/attribution (never ESV).
  meta(provider: ProviderName, bibleId: string, load: () => Promise<VersionMeta>): Promise<VersionMeta>;
}

async function fetchApiBible(
  context: ProviderContext, route: VersionRoute, book: Book, points: VersePoint[],
): Promise<PassageResult> {
  const headers = { 'api-key': context.key };
  const bibleId = route.bibleId as string;
  const meta = await context.meta('api_bible', bibleId, async () => {
    const payload = asRecord(await upstreamJson(
      url(PROVIDER_BASE_URL.api_bible, '/bibles', { language: 'eng' }), headers, context.timeoutMs));
    const list = Array.isArray(payload.data) ? payload.data : [];
    const item = list.map(asRecord).find((entry) => entry.id === bibleId);
    if (!item) throw new UpstreamError('unavailable');
    return { copyright: asText(item.copyright), attribution: asText(item.description) };
  });
  let firstCopyright: string | null = null;
  const verses = await mapLimited(points, async (point) => {
    const payload = asRecord(await upstreamJson(
      url(PROVIDER_BASE_URL.api_bible, `/bibles/${bibleId}/verses/${verseId(book, point)}`, {
        'content-type': 'text', 'include-notes': 'false', 'include-titles': 'false',
      }), headers, context.timeoutMs));
    const data = asRecord(payload.data);
    const text = htmlToText(typeof data.content === 'string' ? data.content : '').replace(LEADING_VERSE_MARKER, '');
    if (!text) throw new UpstreamError('empty');
    return { verse: { book_id: book.id, chapter: point.chapter, verse: point.verse, text }, copyright: asText(data.copyright) };
  });
  for (const entry of verses) {
    if (entry.copyright) { firstCopyright = entry.copyright; break; }
  }
  return {
    verses: verses.map((entry) => entry.verse),
    copyright: firstCopyright ?? meta.copyright,
    attribution: meta.attribution,
  };
}

async function fetchYouVersion(
  context: ProviderContext, route: VersionRoute, book: Book, points: VersePoint[],
): Promise<PassageResult> {
  const headers = { 'X-YVP-App-Key': context.key };
  const bibleId = route.bibleId as string;
  const meta = await context.meta('youversion', bibleId, async () => {
    const payload = asRecord(await upstreamJson(
      url(PROVIDER_BASE_URL.youversion, '/bibles', { 'language_ranges[]': 'en', page_size: '99' }),
      headers, context.timeoutMs));
    const list = Array.isArray(payload.data) ? payload.data : [];
    const item = list.map(asRecord).find((entry) => String(entry.id) === bibleId);
    if (!item) throw new UpstreamError('unavailable');
    return {
      copyright: asText(item.copyright),
      attribution: asText(item.promotional_content) ?? asText(item.publisher_url),
    };
  });
  const verses = await mapLimited(points, async (point) => {
    const payload = asRecord(await upstreamJson(
      url(PROVIDER_BASE_URL.youversion, `/bibles/${bibleId}/passages/${verseId(book, point)}`, {
        format: 'text', include_headings: 'false', include_notes: 'false',
      }), headers, context.timeoutMs));
    const text = collapse(typeof payload.content === 'string' ? payload.content : '');
    if (!text) throw new UpstreamError('empty');
    return { book_id: book.id, chapter: point.chapter, verse: point.verse, text };
  });
  return { verses, copyright: meta.copyright, attribution: meta.attribution };
}

const ESV_OPTIONS: Readonly<Record<string, string>> = {
  'include-passage-references': 'false', 'include-verse-numbers': 'false',
  'include-first-verse-numbers': 'false', 'include-footnotes': 'false',
  'include-footnote-body': 'false', 'include-headings': 'false',
  'include-short-copyright': 'false', 'include-copyright': 'false',
  'include-passage-horizontal-lines': 'false', 'include-heading-horizontal-lines': 'false',
  'indent-poetry': 'false', 'indent-paragraphs': '0', 'indent-declares': '0',
  'indent-psalm-doxology': '0', 'line-length': '0',
};

async function fetchEsv(context: ProviderContext, book: Book, points: VersePoint[]): Promise<PassageResult> {
  const headers = { Authorization: `Token ${context.key}` };
  const verses = await mapLimited(points, async (point) => {
    const payload = asRecord(await upstreamJson(
      url(PROVIDER_BASE_URL.esv, '/passage/text/', { q: `${book.name} ${point.chapter}:${point.verse}`, ...ESV_OPTIONS }),
      headers, context.timeoutMs));
    const passages = Array.isArray(payload.passages) ? payload.passages.filter((p): p is string => typeof p === 'string') : [];
    const text = collapse(passages.join(' '));
    if (!text) throw new UpstreamError('empty');
    return { book_id: book.id, chapter: point.chapter, verse: point.verse, text };
  });
  return { verses, copyright: ESV_COPYRIGHT, attribution: null };
}

// Runs the tasks a few at a time, in order of the input, and stops starting new
// ones after the first failure.
async function mapLimited<T, R>(items: readonly T[], task: (item: T) => Promise<R>): Promise<R[]> {
  const results = new Array<R>(items.length);
  let next = 0;
  let failure: unknown;
  let failed = false;
  async function worker(): Promise<void> {
    while (!failed) {
      const index = next;
      next += 1;
      if (index >= items.length) return;
      try {
        results[index] = await task(items[index]);
      } catch (error) {
        if (!failed) { failed = true; failure = error; }
        return;
      }
    }
  }
  await Promise.all(Array.from({ length: Math.min(UPSTREAM_CONCURRENCY, items.length) }, worker));
  if (failed) throw failure;
  return results;
}

// ---------------------------------------------------------------- the route

const RESPONSE_HEADERS = {
  'content-type': 'application/json; charset=utf-8',
  'cache-control': 'no-store',
};

function respond(body: unknown, status = 200, retryAfter?: number): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { ...RESPONSE_HEADERS, ...(retryAfter ? { 'retry-after': String(retryAfter) } : {}) },
  });
}

function fail(status: number, code: string, message: string, retryAfter?: number): Response {
  return respond({ error: code, message }, status, retryAfter);
}

export function positiveInteger(value: string | undefined, fallback: number, maximum = 1_000_000): number {
  if (typeof value !== 'string' || !/^\d{1,9}$/.test(value.trim())) return fallback;
  const parsed = Number(value.trim());
  return parsed >= 1 && parsed <= maximum ? parsed : fallback;
}

function configuredKey(env: BibleEnv, provider: ProviderName): string | undefined {
  const value = env[PROVIDER_SECRET[provider]];
  return typeof value === 'string' && value.trim().length > 0 ? value : undefined;
}

function passageCacheRequest(version: string, route: VersionRoute, parsed: ParsedReference): Request {
  const spans = parsed.spans.map((span) => {
    const start = `${span.start.chapter}:${span.start.verse}`;
    return span.end ? `${start}-${span.end.verse}` : start;
  }).join(',');
  return new Request(`${CACHE_ORIGIN}/${CACHE_SCHEMA}/passage/${route.provider}/${route.bibleId}/${version}/${parsed.book.id}/${encodeURIComponent(spans)}`);
}

function metaCacheRequest(provider: ProviderName, bibleId: string): Request {
  return new Request(`${CACHE_ORIGIN}/${CACHE_SCHEMA}/meta/${provider}/${bibleId}`);
}

function cacheable(body: unknown, ttl: number): Response {
  return new Response(JSON.stringify(body), {
    headers: { 'content-type': 'application/json', 'cache-control': `public, max-age=${ttl}` },
  });
}

async function readCached<T>(cache: BibleCache | undefined, key: Request): Promise<T | undefined> {
  if (!cache) return undefined;
  try {
    const hit = await cache.match(key);
    return hit ? await hit.json() as T : undefined;
  } catch {
    return undefined;
  }
}

async function writeCached(cache: BibleCache | undefined, key: Request, body: unknown, ttl: number): Promise<void> {
  if (!cache) return;
  try {
    await cache.put(key, cacheable(body, ttl));
  } catch {
    // A cache that cannot be written only costs an upstream call next time.
  }
}

interface PassageInput {
  version: string;
  ref: string;
}

async function readInput(request: Request): Promise<PassageInput | Response> {
  let version: unknown;
  let ref: unknown;
  if (request.method === 'GET') {
    const params = new URL(request.url).searchParams;
    version = params.get('version');
    ref = params.get('ref');
  } else {
    let value: unknown;
    try {
      const text = await request.text();
      if (text.length > MAX_BODY_BYTES) return fail(413, 'request_too_large', 'request body is too large');
      value = JSON.parse(text);
    } catch {
      return fail(400, 'invalid_request', 'the body must be JSON');
    }
    const record = asRecord(value);
    version = record.version;
    ref = record.ref;
  }
  if (typeof version !== 'string' || typeof ref !== 'string') {
    return fail(400, 'invalid_request', 'version and ref are required');
  }
  return { version, ref };
}

// GET|POST /v1/installations/:id/bible/passage
// `registry` is the Registry Durable Object: it authenticates the installation
// and owns the counters. `cache` is the Cloudflare Cache API (undefined where it
// is unavailable, which only means nothing is cached).
export async function handleBiblePassage(
  request: Request,
  env: BibleEnv,
  installationId: string,
  registry: RegistryStub,
  cache: BibleCache | undefined,
): Promise<Response> {
  if (request.method !== 'GET' && request.method !== 'POST') return fail(405, 'method_not_allowed', 'use GET or POST');

  const gate = async (action: 'reserve' | 'slot'): Promise<Response> => registry.fetch(new Request(
    new URL(`/v1/installations/${installationId}/bible/${action}`, request.url).toString(),
    { method: 'POST', headers: { authorization: request.headers.get('authorization') ?? '' } },
  ));

  // 1. Who is asking, and are they within their own limit.
  const reserved = await gate('reserve');
  if (!reserved.ok) return reserved;

  // 2. What they asked for.
  const input = await readInput(request);
  if (input instanceof Response) return input;
  const version = input.version.trim().toUpperCase();
  const route = Object.hasOwn(VERSION_ROUTES, version) ? VERSION_ROUTES[version] : undefined;
  if (!route) return fail(400, 'invalid_version', 'this version is not available through the service');
  let parsed: ParsedReference;
  try {
    parsed = parseReference(input.ref);
  } catch (error) {
    if (error instanceof BibleReferenceError) return fail(400, error.code, error.message);
    return fail(400, 'invalid_reference', 'the reference could not be read');
  }

  // 3. The provider must have its secret.
  const key = configuredKey(env, route.provider);
  if (!key) return fail(503, 'provider_not_configured', 'this version is not enabled on the service yet', 300);

  const respondWith = (result: PassageResult, cached: boolean): Response => respond({
    version,
    reference: parsed.normalized,
    verses: result.verses,
    copyright: result.copyright,
    attribution: result.attribution,
    provider: route.provider,
    cached,
  });

  // 4. Cached text (never the ESV: its terms forbid storing it).
  const storable = route.provider !== 'esv';
  const passageKey = storable ? passageCacheRequest(version, route, parsed) : undefined;
  if (passageKey) {
    const hit = await readCached<PassageResult>(cache, passageKey);
    if (hit && Array.isArray(hit.verses) && hit.verses.length === parsed.verses.length) return respondWith(hit, true);
  }

  // 5. A real upstream call: the service-wide daily cap first.
  const slot = await gate('slot');
  if (!slot.ok) return slot;

  const timeoutMs = positiveInteger(env.BIBLE_UPSTREAM_TIMEOUT_SECONDS, DEFAULT_UPSTREAM_TIMEOUT_SECONDS, 30) * 1000;
  const context: ProviderContext = {
    key,
    timeoutMs,
    meta: async (provider, bibleId, load) => {
      const metaKey = metaCacheRequest(provider, bibleId);
      const known = await readCached<VersionMeta>(cache, metaKey);
      if (known) return known;
      const loaded = await load();
      await writeCached(cache, metaKey, loaded, META_TTL_SECONDS);
      return loaded;
    },
  };
  let result: PassageResult;
  try {
    if (route.provider === 'api_bible') result = await fetchApiBible(context, route, parsed.book, parsed.verses);
    else if (route.provider === 'youversion') result = await fetchYouVersion(context, route, parsed.book, parsed.verses);
    else result = await fetchEsv(context, parsed.book, parsed.verses);
  } catch (error) {
    const upstream = error instanceof UpstreamError ? error : new UpstreamError('network');
    // Provider, failure kind and status only: no key, URL, body or text.
    console.error('bible upstream failed', route.provider, upstream.kind, upstream.status ?? '');
    if (upstream.kind === 'timeout') return fail(504, 'upstream_timeout', 'the Bible provider did not answer in time');
    if (upstream.kind === 'rate_limited') return fail(503, 'upstream_busy', 'the Bible provider is busy; try again shortly', 60);
    return fail(502, 'upstream_error', 'the Bible provider could not supply this passage');
  }
  if (passageKey) await writeCached(cache, passageKey, result, CACHE_TTL_SECONDS);
  return respondWith(result, false);
}
