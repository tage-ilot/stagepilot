use stagepilot_core::keychain_vault::AccountSecrets;
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

/// The secrets this process keeps for the remote-access page.
///
/// macOS: ONE keychain item for the whole process (a vault under `SERVICE`, account `vault`), so
/// an update asks for keychain access once instead of once per account. The old per-account items
/// are read only to migrate them and are never deleted (see `stagepilot_core::keychain_vault`).
/// Windows keeps one Credential Manager entry per account, as before.
#[cfg(target_os = "macos")]
fn os_secrets() -> Arc<dyn AccountSecrets> {
    stagepilot_core::keychain_vault::vault_for_service(SERVICE)
}

#[cfg(windows)]
fn os_secrets() -> Arc<dyn AccountSecrets> {
    Arc::new(PerAccountKeyring)
}

#[cfg(windows)]
struct PerAccountKeyring;

#[cfg(windows)]
impl AccountSecrets for PerAccountKeyring {
    fn get(
        &self,
        account: &str,
    ) -> Result<Option<String>, stagepilot_core::settings::KeychainUnavailable> {
        match keyring::Entry::new(SERVICE, account).and_then(|entry| entry.get_password()) {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(stagepilot_core::settings::KeychainUnavailable),
        }
    }
    fn set(
        &self,
        account: &str,
        secret: &str,
    ) -> Result<(), stagepilot_core::settings::KeychainUnavailable> {
        keyring::Entry::new(SERVICE, account)
            .and_then(|entry| entry.set_password(secret))
            .map_err(|_| stagepilot_core::settings::KeychainUnavailable)
    }
    fn delete(&self, account: &str) -> Result<(), stagepilot_core::settings::KeychainUnavailable> {
        match keyring::Entry::new(SERVICE, account).and_then(|entry| entry.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(stagepilot_core::settings::KeychainUnavailable),
        }
    }
}

impl NativeCredentialBroker {
    pub fn start() -> Result<Self, String> {
        Self::start_with(os_secrets())
    }

    fn start_with(secrets: Arc<dyn AccountSecrets>) -> Result<Self, String> {
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
                        // The listener is non-blocking (so shutdown is noticed), and on macOS an accepted socket
                        // inherits that. A read that finds nothing yet then fails at once, the request is answered
                        // 400 and the socket closed with the request still unread, which resets the connection.
                        // Every request runs blocking, with the timeouts `handle` sets.
                        let _ = stream.set_nonblocking(false);
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
                        let secrets = Arc::clone(&secrets);
                        let counter = Arc::clone(&in_flight);
                        let spawned = thread::Builder::new()
                            .name("credential-broker-request".into())
                            .spawn(move || {
                                handle(stream, &authorization, secrets.as_ref());
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

fn handle(mut stream: TcpStream, authorization: &str, secrets: &dyn AccountSecrets) {
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
    let (status, body) = route(secrets, &request.method, account, &request.body);
    respond(&mut stream, status, &body);
}

/// What the browser is told for one authorized request: the status codes are part of the
/// contract with the remote-access page (200 with the secret, 204, 404, 503 when the keychain
/// does not answer) and do not depend on how the secrets are stored.
fn route(secrets: &dyn AccountSecrets, method: &str, account: &str, body: &str) -> (u16, String) {
    if !valid_account(account) {
        return (400, String::new());
    }
    match method {
        "GET" => match secrets.get(account) {
            Ok(Some(secret)) => (200, secret),
            Ok(None) => (404, String::new()),
            Err(_) => (503, String::new()),
        },
        "PUT" => {
            let expected = format!("spi_{account}.");
            if body.len() > 512
                || !body.starts_with(&expected)
                || body.chars().any(char::is_whitespace)
            {
                (400, String::new())
            } else if secrets.set(account, body).is_ok() {
                (204, String::new())
            } else {
                (503, String::new())
            }
        }
        "DELETE" => match secrets.delete(account) {
            Ok(()) => (204, String::new()),
            Err(_) => (503, String::new()),
        },
        _ => (405, String::new()),
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
    use super::{route, valid_account, NativeCredentialBroker};
    use stagepilot_core::keychain_vault::Vault;
    use stagepilot_core::settings::{KeychainBackend, KeychainUnavailable};
    use std::{
        collections::BTreeMap,
        io::{Read, Write},
        net::TcpStream,
        sync::{Arc, Mutex},
    };

    const ACCOUNT: &str = "43dcc938";
    const OTHER: &str = "0123abcd";

    /// An in-memory keychain item. `fail` makes reads and writes fail (a locked keychain).
    #[derive(Clone, Default)]
    struct Item {
        text: Arc<Mutex<Option<String>>>,
        fail: Arc<Mutex<bool>>,
    }

    impl KeychainBackend for Item {
        fn read(&self) -> Result<Option<String>, KeychainUnavailable> {
            if *self.fail.lock().unwrap() {
                return Err(KeychainUnavailable);
            }
            Ok(self.text.lock().unwrap().clone())
        }
        fn write(&self, secret: &str) -> Result<(), KeychainUnavailable> {
            if *self.fail.lock().unwrap() {
                return Err(KeychainUnavailable);
            }
            *self.text.lock().unwrap() = Some(secret.to_string());
            Ok(())
        }
        fn delete(&self) -> Result<(), KeychainUnavailable> {
            Err(KeychainUnavailable)
        }
    }

    /// Old per-account items (read only).
    struct Legacy(Arc<Mutex<BTreeMap<String, String>>>, String);

    impl KeychainBackend for Legacy {
        fn read(&self) -> Result<Option<String>, KeychainUnavailable> {
            Ok(self.0.lock().unwrap().get(&self.1).cloned())
        }
        fn write(&self, _: &str) -> Result<(), KeychainUnavailable> {
            Err(KeychainUnavailable)
        }
        fn delete(&self) -> Result<(), KeychainUnavailable> {
            Err(KeychainUnavailable)
        }
    }

    fn vault(item: &Item, legacy: &[(&str, &str)]) -> Arc<Vault> {
        let map: BTreeMap<String, String> = legacy
            .iter()
            .map(|(a, s)| (a.to_string(), s.to_string()))
            .collect();
        let map = Arc::new(Mutex::new(map));
        Arc::new(Vault::new(
            Box::new(item.clone()),
            Box::new(move |account| Box::new(Legacy(Arc::clone(&map), account.to_string()))),
        ))
    }

    fn token(account: &str) -> String {
        format!("spi_{account}.test-value")
    }

    #[test]
    fn routes_answer_the_same_codes_with_the_vault_underneath() {
        let item = Item::default();
        let v = vault(&item, &[]);
        let secret = token(ACCOUNT);
        assert_eq!(route(v.as_ref(), "GET", ACCOUNT, ""), (404, String::new()));
        assert_eq!(
            route(v.as_ref(), "PUT", ACCOUNT, &secret),
            (204, String::new())
        );
        assert_eq!(route(v.as_ref(), "GET", ACCOUNT, ""), (200, secret.clone()));
        assert_eq!(route(v.as_ref(), "GET", OTHER, ""), (404, String::new()));
        assert_eq!(
            route(v.as_ref(), "DELETE", ACCOUNT, ""),
            (204, String::new())
        );
        assert_eq!(route(v.as_ref(), "GET", ACCOUNT, ""), (404, String::new()));
        // deleting what is not there is still a 204
        assert_eq!(route(v.as_ref(), "DELETE", OTHER, ""), (204, String::new()));
        assert_eq!(route(v.as_ref(), "POST", ACCOUNT, ""), (405, String::new()));
    }

    #[test]
    fn bad_requests_never_reach_the_keychain() {
        let item = Item::default();
        let v = vault(&item, &[]);
        assert_eq!(route(v.as_ref(), "GET", "abc", "").0, 400);
        assert_eq!(route(v.as_ref(), "PUT", ACCOUNT, "no-prefix").0, 400);
        assert_eq!(
            route(v.as_ref(), "PUT", ACCOUNT, &format!("{} x", token(ACCOUNT))).0,
            400
        );
        assert_eq!(
            route(
                v.as_ref(),
                "PUT",
                ACCOUNT,
                &format!("spi_{ACCOUNT}.{}", "a".repeat(600))
            )
            .0,
            400
        );
        assert!(item.text.lock().unwrap().is_none());
    }

    #[test]
    fn a_secret_saved_by_the_old_app_is_found_and_moved_into_the_vault() {
        let item = Item::default();
        let secret = token(ACCOUNT);
        let v = vault(&item, &[(ACCOUNT, &secret)]);
        assert_eq!(route(v.as_ref(), "GET", ACCOUNT, ""), (200, secret.clone()));
        assert!(item
            .text
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .contains(ACCOUNT));
    }

    #[test]
    fn a_keychain_that_does_not_answer_is_a_503_and_a_corrupt_vault_is_not_overwritten() {
        let item = Item::default();
        let v = vault(&item, &[]);
        *item.fail.lock().unwrap() = true;
        assert_eq!(route(v.as_ref(), "GET", ACCOUNT, "").0, 503);
        assert_eq!(route(v.as_ref(), "PUT", ACCOUNT, &token(ACCOUNT)).0, 503);
        assert_eq!(route(v.as_ref(), "DELETE", ACCOUNT, "").0, 503);

        let corrupt = Item::default();
        *corrupt.text.lock().unwrap() = Some("not json".into());
        let secret = token(ACCOUNT);
        let v = vault(&corrupt, &[(ACCOUNT, &secret)]);
        assert_eq!(route(v.as_ref(), "GET", ACCOUNT, ""), (200, secret));
        assert_eq!(route(v.as_ref(), "PUT", ACCOUNT, &token(ACCOUNT)).0, 503);
        assert_eq!(route(v.as_ref(), "DELETE", ACCOUNT, "").0, 503);
        assert_eq!(corrupt.text.lock().unwrap().as_deref(), Some("not json"));
    }

    fn request(
        origin: &str,
        authorization: &str,
        method: &str,
        path: &str,
        body: &str,
    ) -> (u16, String) {
        let address = origin.trim_start_matches("http://");
        let mut stream = TcpStream::connect(address).unwrap();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {authorization}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let status = response.split_whitespace().nth(1).unwrap().parse().unwrap();
        let body = response.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        (status, body)
    }

    #[test]
    fn the_loopback_broker_serves_get_put_delete_over_the_vault() {
        let item = Item::default();
        let broker = NativeCredentialBroker::start_with(vault(&item, &[])).unwrap();
        let path = format!("/v1/credentials/{ACCOUNT}");
        let secret = token(ACCOUNT);
        let auth = broker.authorization.clone();
        let origin = broker.origin.clone();
        assert_eq!(request(&origin, "wrong", "GET", &path, "").0, 401);
        assert_eq!(request(&origin, &auth, "GET", &path, "").0, 404);
        assert_eq!(request(&origin, &auth, "PUT", &path, &secret).0, 204);
        assert_eq!(request(&origin, &auth, "GET", &path, ""), (200, secret));
        assert_eq!(request(&origin, &auth, "DELETE", &path, "").0, 204);
        assert_eq!(request(&origin, &auth, "GET", &path, "").0, 404);
    }

    #[test]
    fn a_request_that_arrives_late_is_still_served() {
        // The request bytes reach the broker well after the connection was accepted (a slow client, a busy
        // machine). The accepted socket must wait for them instead of failing the read at once.
        let item = Item::default();
        let broker = NativeCredentialBroker::start_with(vault(&item, &[])).unwrap();
        let path = format!("/v1/credentials/{ACCOUNT}");
        let address = broker.origin.trim_start_matches("http://").to_string();
        let mut stream = TcpStream::connect(&address).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            broker.authorization
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 404"), "{response}");
    }

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
