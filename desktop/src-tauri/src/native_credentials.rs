use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

const SERVICE: &str = "org.stagepilot.desktop.remote";
const MAX_REQUEST_BYTES: usize = 16 * 1024;
/// Requests served at the same time. A keychain call can wait on a system dialog for as long
/// as nobody answers it, so each runs on its own thread, but never an unbounded number.
const MAX_IN_FLIGHT: usize = 8;

pub struct NativeCredentialBroker {
    pub origin: String,
    pub authorization: String,
    shutdown: Arc<AtomicBool>,
}

impl NativeCredentialBroker {
    pub fn start() -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|_| "Could not start the native credential broker.".to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "Could not secure the native credential broker.".to_string())?;
        let origin = format!(
            "http://127.0.0.1:{}",
            listener
                .local_addr()
                .map_err(|_| "Could not resolve the native credential broker.".to_string())?
                .port()
        );
        let mut random = [0_u8; 32];
        getrandom::getrandom(&mut random)
            .map_err(|_| "Could not authorize the native credential broker.".to_string())?;
        let authorization: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let thread_authorization = authorization.clone();
        let in_flight = Arc::new(AtomicUsize::new(0));
        thread::spawn(move || {
            while !thread_shutdown.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        // Each request on a thread of its own: one keychain call that sits on a
                        // system approval dialog must not stop the next request from being served.
                        // A few at once at most; beyond that the caller gets 503 and tries later.
                        if in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
                            in_flight.fetch_sub(1, Ordering::SeqCst);
                            let mut stream = stream;
                            respond(&mut stream, 503, "");
                            continue;
                        }
                        let authorization = thread_authorization.clone();
                        let counter = Arc::clone(&in_flight);
                        let spawned = thread::Builder::new()
                            .name("credential-broker-request".into())
                            .spawn(move || {
                                handle(stream, &authorization);
                                counter.fetch_sub(1, Ordering::SeqCst);
                            });
                        if spawned.is_err() {
                            in_flight.fetch_sub(1, Ordering::SeqCst);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(25));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(100)),
                }
            }
        });
        Ok(Self {
            origin,
            authorization,
            shutdown,
        })
    }
}

impl Drop for NativeCredentialBroker {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

fn valid_account(account: &str) -> bool {
    matches!(account.len(), 8 | 16 | 32) && account.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn handle(mut stream: TcpStream, authorization: &str) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let Some(request) = read_request(&mut stream) else {
        respond(&mut stream, 400, "");
        return;
    };
    if request.authorization.as_deref() != Some(authorization) {
        respond(&mut stream, 401, "");
        return;
    }
    let Some(account) = request.path.strip_prefix("/v1/credentials/") else {
        respond(&mut stream, 404, "");
        return;
    };
    if !valid_account(account) {
        respond(&mut stream, 400, "");
        return;
    }
    let Ok(entry) = keyring::Entry::new(SERVICE, account) else {
        respond(&mut stream, 503, "");
        return;
    };
    match request.method.as_str() {
        "GET" => match entry.get_password() {
            Ok(secret) => respond(&mut stream, 200, &secret),
            Err(keyring::Error::NoEntry) => respond(&mut stream, 404, ""),
            Err(_) => respond(&mut stream, 503, ""),
        },
        "PUT" => {
            let expected = format!("spi_{account}.");
            if request.body.len() > 512
                || !request.body.starts_with(&expected)
                || request.body.chars().any(char::is_whitespace)
            {
                respond(&mut stream, 400, "");
            } else if entry.set_password(&request.body).is_ok() {
                respond(&mut stream, 204, "");
            } else {
                respond(&mut stream, 503, "");
            }
        }
        "DELETE" => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => respond(&mut stream, 204, ""),
            Err(_) => respond(&mut stream, 503, ""),
        },
        _ => respond(&mut stream, 405, ""),
    }
}

struct Request {
    method: String,
    path: String,
    authorization: Option<String>,
    body: String,
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    let header_end = loop {
        let count = stream.read(&mut buffer).ok()?;
        if count == 0 || bytes.len() + count > MAX_REQUEST_BYTES {
            return None;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(position) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let header = std::str::from_utf8(&bytes[..header_end]).ok()?;
    let mut lines = header.split("\r\n");
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_string();
    let path = first.next()?.to_string();
    if first.next()? != "HTTP/1.1" || first.next().is_some() {
        return None;
    }
    let mut authorization = None;
    let mut content_length = 0_usize;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':')?;
        match name.trim().to_ascii_lowercase().as_str() {
            "authorization" => {
                authorization = value.trim().strip_prefix("Bearer ").map(str::to_string)
            }
            "content-length" => content_length = value.trim().parse().ok()?,
            _ => {}
        }
    }
    if header_end + content_length > MAX_REQUEST_BYTES {
        return None;
    }
    while bytes.len() < header_end + content_length {
        let count = stream.read(&mut buffer).ok()?;
        if count == 0 || bytes.len() + count > MAX_REQUEST_BYTES {
            return None;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let body = std::str::from_utf8(&bytes[header_end..header_end + content_length])
        .ok()?
        .to_string();
    Some(Request {
        method,
        path,
        authorization,
        body,
    })
}

fn respond(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Service Unavailable",
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use super::valid_account;

    #[test]
    fn accepts_current_and_legacy_installation_id_lengths() {
        // 8 hex chars: the current (beta.9+) shortened installation id.
        assert!(valid_account("43dcc938"));
        // 16 and 32 hex chars: legacy/back-compat installation id lengths
        // this broker must still accept for installations enrolled before
        // the id-shortening change.
        assert!(valid_account("43dcc9384f7eede0"));
        assert!(valid_account(&"a".repeat(32)));
    }

    #[test]
    fn rejects_wrong_length_or_non_hex_accounts() {
        assert!(!valid_account(""));
        assert!(!valid_account("abc"));
        assert!(!valid_account(&"a".repeat(7)));
        assert!(!valid_account(&"a".repeat(9)));
        assert!(!valid_account(&"a".repeat(33)));
        assert!(!valid_account("zzzzzzzz"));
        assert!(!valid_account("43dcc93!"));
    }
}
