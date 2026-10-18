//! Static dashboard serving: parity with Starlette's `StaticFiles(directory=..., html=True)`
//! mounted at `/` (see `main.create_app`), which `ServeDir` does not match.
//!
//! * only GET and HEAD; anything else is `405 {"detail":"Method Not Allowed"}`
//! * the path is percent-decoded and normalised like `os.path.normpath`; anything that resolves
//!   (symlinks included) outside the web root, contains a NUL byte or is not found is
//!   `404 {"detail":"Not Found"}`. There is **no SPA fallback**: an unknown path is a 404, exactly
//!   as in Python (the dashboard has no client-side routes)
//! * a directory with an `index.html` redirects (307) to the same URL with a trailing slash, and
//!   serves the index when the slash is there; a directory without one is a 404; `404.html` at
//!   the root, when present, is served with status 404
//! * files carry `content-type` (text types get `; charset=utf-8`), `accept-ranges: bytes`,
//!   `content-length`, `last-modified`, `etag`; `If-None-Match` / `If-Modified-Since` give a 304
//!   with only the validators; a single `Range` gives a 206 (416 when unsatisfiable, 400 when
//!   malformed)

use crate::validation::http_error;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use md5::{Digest, Md5};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

/// `mimetypes.guess_type` for the file types a Vite build produces (Python 3.12 table).
fn media_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "txt" => "text/plain",
        "xml" => "text/xml",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "ico" => "image/vnd.microsoft.icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "webmanifest" => "application/manifest+json",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

fn content_type(path: &Path) -> String {
    let media = media_type(path);
    if media.starts_with("text/") {
        format!("{media}; charset=utf-8")
    } else {
        media.to_string()
    }
}

/// `os.path.normpath(os.path.join(*route_path.split("/")))`: `None` when it climbs above the
/// root (Python's `commonpath` check then yields a 404).
fn normalise(route_path: &str) -> Option<PathBuf> {
    let decoded = percent_encoding::percent_decode_str(route_path)
        .decode_utf8()
        .ok()?
        .into_owned();
    if decoded.contains('\0') {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in decoded.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => {
                // A Windows drive or separator would make the join absolute.
                if other.contains('\\') || other.contains(':') {
                    return None;
                }
                parts.push(other);
            }
        }
    }
    Some(parts.iter().collect())
}

/// Resolve inside the root, following symlinks like `os.path.realpath`; refuse escapes.
fn lookup(root: &Path, relative: &Path) -> Option<(PathBuf, std::fs::Metadata)> {
    if relative.is_absolute()
        || relative.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return None;
    }
    let real_root = root.canonicalize().ok()?;
    let full = real_root.join(relative).canonicalize().ok()?;
    if !full.starts_with(&real_root) {
        return None;
    }
    let meta = std::fs::metadata(&full).ok()?;
    Some((full, meta))
}

fn json_error(code: u16, detail: &str) -> Response {
    http_error(code, detail).into_response()
}

struct Validators {
    last_modified: String,
    etag: String,
    size: u64,
}

fn validators(meta: &std::fs::Metadata) -> Validators {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok());
    let seconds = mtime.map(|d| d.as_secs_f64()).unwrap_or(0.0);
    let last_modified =
        httpdate::fmt_http_date(UNIX_EPOCH + std::time::Duration::from_secs(seconds as u64));
    // Starlette: md5(f"{st_mtime}-{st_size}"); Rust formats the float differently from Python,
    // so the tag value differs while keeping its meaning (changes with mtime or size).
    let mut hasher = Md5::new();
    hasher.update(format!("{seconds}-{}", meta.len()).as_bytes());
    Validators {
        last_modified,
        etag: format!("\"{}\"", hex::encode(hasher.finalize())),
        size: meta.len(),
    }
}

fn not_modified(headers: &HeaderMap, v: &Validators) -> bool {
    if let Some(candidates) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|h| h.to_str().ok())
    {
        return candidates
            .split(',')
            .map(|t| t.trim().trim_start_matches("W/"))
            .any(|t| t == v.etag);
    }
    let since = headers
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| httpdate::parse_http_date(s).ok());
    let modified = httpdate::parse_http_date(&v.last_modified).ok();
    matches!((since, modified), (Some(since), Some(modified)) if since >= modified)
}

enum Ranges {
    Whole,
    One(u64, u64),
    Malformed(&'static str),
    Unsatisfiable,
}

/// Starlette `FileResponse._parse_range_header`. Several ranges are served as the whole file
/// (a documented difference: no `multipart/byteranges`).
fn parse_range(header: &str, size: u64) -> Ranges {
    let Some((units, spec)) = header.split_once('=') else {
        return Ranges::Malformed("Malformed range header.");
    };
    if units.trim().to_ascii_lowercase() != "bytes" {
        return Ranges::Malformed("Only support bytes range");
    }
    let mut ranges: Vec<(i128, i128)> = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() || part == "-" {
            continue;
        }
        let Some((start, end)) = part.split_once('-') else {
            continue;
        };
        let (start, end) = (start.trim(), end.trim());
        let size_i = i128::from(size);
        let parsed = (|| -> Option<(i128, i128)> {
            let first = if start.is_empty() {
                (size_i - end.parse::<i128>().ok()?).max(0)
            } else {
                start.parse::<i128>().ok()?
            };
            let last = if !start.is_empty() && !end.is_empty() && end.parse::<i128>().ok()? < size_i
            {
                end.parse::<i128>().ok()? + 1
            } else {
                size_i
            };
            Some((first, last))
        })();
        if let Some(range) = parsed {
            ranges.push(range);
        }
    }
    if ranges.is_empty() {
        return Ranges::Malformed("Range header: range must be requested");
    }
    if ranges
        .iter()
        .any(|(s, _)| !(0..i128::from(size)).contains(s))
    {
        return Ranges::Unsatisfiable;
    }
    if ranges.iter().any(|(s, e)| s > e) {
        return Ranges::Malformed("Range header: start must be less than end");
    }
    if ranges.len() == 1 {
        let (s, e) = ranges[0];
        return Ranges::One(s as u64, e as u64);
    }
    Ranges::Whole
}

fn file_response(
    path: &Path,
    meta: &std::fs::Metadata,
    request_headers: &HeaderMap,
    head: bool,
    status: StatusCode,
) -> Response {
    let v = validators(meta);
    let header_value = |s: &str| HeaderValue::from_str(s).unwrap_or(HeaderValue::from_static(""));
    if status == StatusCode::OK && not_modified(request_headers, &v) {
        let mut response = StatusCode::NOT_MODIFIED.into_response();
        let headers = response.headers_mut();
        headers.insert(header::ETAG, header_value(&v.etag));
        return response;
    }
    let ranged = request_headers
        .get(header::RANGE)
        .and_then(|h| h.to_str().ok())
        .filter(|_| {
            // `If-Range` must match the validators or the whole file is sent.
            request_headers
                .get(header::IF_RANGE)
                .and_then(|h| h.to_str().ok())
                .is_none_or(|v2| v2 == v.last_modified || v2 == v.etag)
        });
    let mut start = 0;
    let mut end = v.size;
    let mut status = status;
    if let Some(range) = ranged {
        match parse_range(range, v.size) {
            Ranges::Malformed(text) => {
                let mut response = (StatusCode::BAD_REQUEST, text).into_response();
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/plain; charset=utf-8"),
                );
                return response;
            }
            Ranges::Unsatisfiable => {
                let mut response = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/plain; charset=utf-8"),
                );
                response.headers_mut().insert(
                    header::CONTENT_RANGE,
                    header_value(&format!("bytes */{}", v.size)),
                );
                return response;
            }
            Ranges::One(s, e) => {
                start = s;
                end = e.min(v.size);
                status = StatusCode::PARTIAL_CONTENT;
            }
            Ranges::Whole => {}
        }
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return json_error(404, "Not Found"),
    };
    let slice = bytes
        .get(start as usize..(end as usize).min(bytes.len()))
        .unwrap_or_default()
        .to_vec();
    let mut response = Response::new(if head {
        Body::empty()
    } else {
        Body::from(slice.clone())
    });
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, header_value(&content_type(path)));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        header::CONTENT_LENGTH,
        header_value(&slice.len().to_string()),
    );
    headers.insert(header::LAST_MODIFIED, header_value(&v.last_modified));
    headers.insert(header::ETAG, header_value(&v.etag));
    if status == StatusCode::PARTIAL_CONTENT {
        headers.insert(
            header::CONTENT_RANGE,
            header_value(&format!("bytes {start}-{}/{}", end - 1, v.size)),
        );
    }
    response
}

/// `URL(scope).replace(path=path + "/")`: same scheme, host and query, trailing slash added.
fn slash_redirect(request: &Request) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost");
    let query = request
        .uri()
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let location = format!("http://{host}{}/{query}", request.uri().path());
    let mut response = StatusCode::TEMPORARY_REDIRECT.into_response();
    if let Ok(value) = HeaderValue::from_str(&location) {
        response.headers_mut().insert(header::LOCATION, value);
    }
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, HeaderValue::from_static("0"));
    response
}

/// The router fallback.
pub async fn serve(root: Arc<PathBuf>, request: Request) -> Response {
    let method = request.method().clone();
    if method != Method::GET && method != Method::HEAD {
        return json_error(405, "Method Not Allowed");
    }
    let head = method == Method::HEAD;
    let Some(relative) = normalise(request.uri().path()) else {
        return json_error(404, "Not Found");
    };
    let headers = request.headers().clone();
    let Some((full, meta)) = lookup(&root, &relative) else {
        return not_found(&root, &headers, head);
    };
    if meta.is_file() {
        return file_response(&full, &meta, &headers, head, StatusCode::OK);
    }
    if meta.is_dir() {
        if let Some((index, index_meta)) = lookup(&root, &relative.join("index.html")) {
            if index_meta.is_file() {
                if !request.uri().path().ends_with('/') {
                    return slash_redirect(&request);
                }
                return file_response(&index, &index_meta, &headers, head, StatusCode::OK);
            }
        }
    }
    not_found(&root, &headers, head)
}

/// `404.html` (html mode), else the JSON 404.
fn not_found(root: &Path, headers: &HeaderMap, head: bool) -> Response {
    if let Some((page, meta)) = lookup(root, Path::new("404.html")) {
        if meta.is_file() {
            return file_response(&page, &meta, headers, head, StatusCode::NOT_FOUND);
        }
    }
    json_error(404, "Not Found")
}
