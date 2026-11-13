//! Optional, authenticated loopback transport for the in-app agent.

use eframe::egui;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

const MAX_REQUEST: usize = 1024 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(3);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);
const PENDING: u8 = 0;
const STARTED: u8 = 1;
const CANCELLED: u8 = 2;

#[derive(Deserialize)]
struct Incoming {
    token: String,
    command: String,
    args: Value,
    document_id: Option<String>,
    expected_revision: Option<u64>,
}

pub(crate) struct Request {
    pub(crate) command: String,
    pub(crate) args: Value,
    pub(crate) document_id: Option<String>,
    pub(crate) expected_revision: Option<u64>,
    pub(crate) respond: mpsc::Sender<Value>,
    deadline: Instant,
    state: Arc<AtomicU8>,
}

pub(crate) struct Bridge {
    receiver: mpsc::Receiver<Request>,
    shutdown: Arc<AtomicBool>,
}

impl Bridge {
    /// Disabled unless both opt-in environment variables are set.
    pub(crate) fn start(ctx: egui::Context) -> Result<Option<Self>, String> {
        let port = match std::env::var("CREABIM_MCP_PORT") {
            Ok(port) => port,
            Err(std::env::VarError::NotPresent) => return Ok(None),
            Err(_) => return Err("CREABIM_MCP_PORT is not valid Unicode".into()),
        };
        let port = port
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or("CREABIM_MCP_PORT must be a port from 1 to 65535")?;
        let explicit = match std::env::var("CREABIM_MCP_TOKEN") {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => return Err("Invalid MCP bridge token".into()),
        };
        let token_file = std::env::var_os("CREABIM_MCP_TOKEN_FILE");
        let token = resolve_token(
            explicit.as_deref(),
            token_file.as_deref().map(std::path::Path::new),
        )?;
        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|e| format!("Could not bind the local MCP bridge: {e}"))?;
        Self::serve(listener, token, ctx, RESPONSE_TIMEOUT).map(Some)
    }

    fn serve(
        listener: TcpListener,
        token: String,
        ctx: egui::Context,
        timeout: Duration,
    ) -> Result<Self, String> {
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("Could not configure local MCP bridge: {e}"))?;
        let (sender, receiver) = mpsc::sync_channel(8);
        let shutdown = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&shutdown);
        thread::Builder::new()
            .name("creabim-agent".into())
            .spawn(move || {
                while !stopping.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => handle(stream, &token, &sender, &ctx, timeout),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20))
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(|e| format!("Could not start local MCP bridge: {e}"))?;
        Ok(Self { receiver, shutdown })
    }

    /// Requests returned here have started; dispatch and respond synchronously.
    pub(crate) fn try_recv(&self) -> Option<Request> {
        while let Ok(request) = self.receiver.try_recv() {
            if Instant::now() >= request.deadline {
                let _ = request.state.compare_exchange(
                    PENDING,
                    CANCELLED,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                continue;
            }
            if request
                .state
                .compare_exchange(PENDING, STARTED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                if Instant::now() < request.deadline {
                    return Some(request);
                }
                request.state.store(CANCELLED, Ordering::Release);
            }
        }
        None
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
    }
}

fn resolve_token(explicit: Option<&str>, file: Option<&std::path::Path>) -> Result<String, String> {
    let token = match explicit {
        Some(token) => token.to_string(),
        None => read_private_token(file.ok_or("MCP bridge token is not configured")?)?,
    };
    if token.chars().count() < 32 {
        return Err("MCP bridge token must contain at least 32 characters".into());
    }
    Ok(token)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn read_private_token(path: &std::path::Path) -> Result<String, String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    const ERROR: &str = "MCP bridge token file is invalid or not private";
    // geteuid has no arguments and reads only the current process identity.
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    let uid = unsafe { geteuid() };
    let parent = path
        .parent()
        .ok_or(ERROR)?
        .canonicalize()
        .map_err(|_| ERROR)?;
    let directory = std::fs::metadata(&parent).map_err(|_| ERROR)?;
    if !directory.is_dir() || directory.uid() != uid || directory.mode() & 0o7777 != 0o700 {
        return Err(ERROR.into());
    }
    let path = parent.join(path.file_name().ok_or(ERROR)?);
    let before = std::fs::symlink_metadata(&path).map_err(|_| ERROR)?;
    if !before.file_type().is_file() {
        return Err(ERROR.into());
    }
    #[cfg(target_os = "macos")]
    let flags = 0x100 | 0x4; // O_NOFOLLOW | O_NONBLOCK
    #[cfg(target_os = "linux")]
    let flags = 0o400000 | 0o4000; // O_NOFOLLOW | O_NONBLOCK
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(&path)
        .map_err(|_| ERROR)?;
    let opened = file.metadata().map_err(|_| ERROR)?;
    let after = std::fs::symlink_metadata(&path).map_err(|_| ERROR)?;
    if !opened.file_type().is_file()
        || !after.file_type().is_file()
        || opened.uid() != uid
        || opened.mode() & 0o7777 != 0o600
        || (before.dev(), before.ino()) != (opened.dev(), opened.ino())
        || (after.dev(), after.ino()) != (opened.dev(), opened.ino())
    {
        return Err(ERROR.into());
    }
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes).map_err(|_| ERROR)?;
    if bytes.len() > 4096 {
        return Err(ERROR.into());
    }
    let text = String::from_utf8(bytes).map_err(|_| ERROR)?;
    Ok(text.trim_end_matches(['\r', '\n']).to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn read_private_token(_path: &std::path::Path) -> Result<String, String> {
    Err("MCP bridge token files are not supported on this platform".into())
}

fn same_token(expected: &str, actual: &str) -> bool {
    if expected.len() != actual.len() {
        return false;
    }
    expected
        .as_bytes()
        .iter()
        .zip(actual.as_bytes())
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

fn read_message(stream: &mut TcpStream) -> Result<Vec<u8>, &'static str> {
    let deadline = Instant::now() + READ_TIMEOUT;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("request timed out")?;
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|_| "connection error")?;
        let count = stream
            .read(&mut chunk)
            .map_err(|_| "request could not be read")?;
        if count == 0 {
            return Err("request must end with a newline");
        }
        let end = chunk[..count].iter().position(|b| *b == b'\n');
        let used = end.map_or(count, |i| i + 1);
        if bytes.len() + used > MAX_REQUEST {
            return Err("request is too large");
        }
        bytes.extend_from_slice(&chunk[..used]);
        if end.is_some() {
            return Ok(bytes);
        }
    }
}

fn respond(stream: &mut TcpStream, value: Value) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    if let Ok(mut bytes) = serde_json::to_vec(&value) {
        bytes.push(b'\n');
        let _ = stream.write_all(&bytes);
    }
}

fn fail(stream: &mut TcpStream, message: &str) {
    respond(stream, json!({"ok": false, "error": message}));
}

fn handle(
    mut stream: TcpStream,
    token: &str,
    sender: &mpsc::SyncSender<Request>,
    ctx: &egui::Context,
    timeout: Duration,
) {
    if stream.set_nonblocking(false).is_err() {
        return;
    }
    let line = match read_message(&mut stream) {
        Ok(line) => line,
        Err(message) => {
            fail(&mut stream, message);
            return;
        }
    };
    let message: Incoming = match serde_json::from_slice(&line) {
        Ok(message) => message,
        Err(_) => {
            fail(&mut stream, "invalid request");
            return;
        }
    };
    if !same_token(token, &message.token) {
        fail(&mut stream, "authentication failed");
        return;
    }
    if message.command.is_empty() || message.command.len() > 128 || !message.args.is_object() {
        fail(&mut stream, "invalid command or arguments");
        return;
    }
    let (reply, receiver) = mpsc::channel();
    let state = Arc::new(AtomicU8::new(PENDING));
    let deadline = Instant::now() + timeout;
    let request = Request {
        command: message.command,
        args: message.args,
        document_id: message.document_id,
        expected_revision: message.expected_revision,
        respond: reply,
        deadline,
        state: Arc::clone(&state),
    };
    if sender.try_send(request).is_err() {
        fail(&mut stream, "bridge is busy");
        return;
    }
    ctx.request_repaint();
    match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(value) => respond(&mut stream, value),
        Err(_)
            if state
                .compare_exchange(PENDING, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok() =>
        {
            fail(&mut stream, "request expired before it started")
        }
        Err(_) if state.load(Ordering::Acquire) == CANCELLED => {
            fail(&mut stream, "request expired before it started")
        }
        Err(_) => fail(
            &mut stream,
            "request started but outcome is unknown; inspect the model before retrying",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::thread;
    use std::time::{Duration, Instant};

    const TOKEN: &str = "12345678901234567890123456789012";

    fn start(timeout: Duration) -> (Bridge, SocketAddr) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let bridge = Bridge::serve(
            listener,
            TOKEN.to_string(),
            eframe::egui::Context::default(),
            timeout,
        )
        .unwrap();
        (bridge, addr)
    }

    fn request(addr: SocketAddr, payload: serde_json::Value) -> serde_json::Value {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        writeln!(stream, "{payload}").unwrap();
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn token_file_is_private_bounded_and_env_takes_precedence() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = std::env::temp_dir().join(format!(
            "creabim-token-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.join("token");
        std::fs::write(&path, format!("{TOKEN}\r\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(resolve_token(None, Some(&path)).unwrap(), TOKEN);
        let explicit = "another-explicit-token-1234567890";
        assert_eq!(
            resolve_token(Some(explicit), Some(&dir.join("missing"))).unwrap(),
            explicit
        );
        assert!(resolve_token(Some("short"), Some(&path)).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(resolve_token(None, Some(&path)).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = dir.join("link");
        symlink(&path, &link).unwrap();
        assert!(resolve_token(None, Some(&link)).is_err());
        std::fs::write(&path, vec![b'x'; 4097]).unwrap();
        assert!(resolve_token(None, Some(&path)).is_err());
        std::fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(resolve_token(None, Some(&path)).is_err());
        std::fs::write(&path, TOKEN).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(resolve_token(None, Some(&path)).is_err());
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_wrong_token_without_dispatch() {
        let (bridge, addr) = start(Duration::from_secs(1));
        let response = request(
            addr,
            serde_json::json!({"token":"wrong", "command":"inspect", "args":{}}),
        );
        assert_eq!(response["ok"], false);
        assert!(bridge.try_recv().is_none());
    }

    #[test]
    fn dispatches_authenticated_request_and_returns_response() {
        let (bridge, addr) = start(Duration::from_secs(1));
        let client = thread::spawn(move || {
            request(
                addr,
                serde_json::json!({
                    "token":TOKEN, "command":"create_wall", "args":{"name":"wall"},
                    "document_id":"doc-1", "expected_revision":7
                }),
            )
        });
        let end = Instant::now() + Duration::from_secs(1);
        let item = loop {
            if let Some(item) = bridge.try_recv() {
                break item;
            }
            assert!(Instant::now() < end, "request never dispatched");
            thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(item.command, "create_wall");
        assert_eq!(item.args["name"], "wall");
        assert_eq!(item.document_id.as_deref(), Some("doc-1"));
        assert_eq!(item.expected_revision, Some(7));
        item.respond
            .send(serde_json::json!({"ok":true,"result":{"id":"wall-1"}}))
            .unwrap();
        assert_eq!(client.join().unwrap()["result"]["id"], "wall-1");
    }

    #[test]
    fn rejects_unterminated_and_oversized_requests() {
        use std::net::Shutdown;
        let (bridge, addr) = start(Duration::from_secs(1));
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream.write_all(b"{} ").unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&line).unwrap()["ok"],
            false
        );
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let _ = stream.write_all(&vec![b'x'; MAX_REQUEST + 1]);
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&line).unwrap()["ok"],
            false
        );
        assert!(bridge.try_recv().is_none());
    }

    #[test]
    fn started_request_that_outlives_deadline_reports_unknown_outcome() {
        let (bridge, addr) = start(Duration::from_millis(150));
        let client = thread::spawn(move || {
            request(
                addr,
                serde_json::json!({
                    "token":TOKEN, "command":"create_wall", "args":{}
                }),
            )
        });
        let end = Instant::now() + Duration::from_secs(1);
        let item = loop {
            if let Some(item) = bridge.try_recv() {
                break item;
            }
            assert!(Instant::now() < end, "request never dispatched");
            thread::sleep(Duration::from_millis(2));
        };
        let response = client.join().unwrap();
        assert!(response["error"]
            .as_str()
            .unwrap()
            .contains("outcome is unknown"));
        assert!(item.respond.send(serde_json::json!({"ok":true})).is_err());
    }

    #[test]
    fn expired_queued_request_is_never_delivered() {
        let (bridge, addr) = start(Duration::from_millis(150));
        let client = thread::spawn(move || {
            request(
                addr,
                serde_json::json!({
                    "token":TOKEN, "command":"create_wall", "args":{}
                }),
            )
        });
        let response = client.join().unwrap();
        assert_eq!(response["ok"], false);
        assert!(bridge.try_recv().is_none());
    }
}
