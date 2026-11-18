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
        let token =
            std::env::var("CREABIM_MCP_TOKEN").map_err(|_| "MCP bridge token is not configured")?;
        if !valid_token(&token) {
            return Err("MCP bridge token must contain 32 to 4096 printable ASCII characters without spaces".into());
        }
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

fn valid_token(token: &str) -> bool {
    (32..=4096).contains(&token.len()) && token.bytes().all(|b| (0x21..=0x7e).contains(&b))
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

fn read_message(stream: &mut TcpStream, timeout: Duration) -> Result<Vec<u8>, &'static str> {
    let deadline = Instant::now() + timeout;
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
    let line = match read_message(&mut stream, READ_TIMEOUT) {
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
    use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
    use std::thread;
    use std::time::{Duration, Instant};

    const TOKEN: &str = "random-test-token-1234567890123456789";

    fn start(timeout: Duration) -> (Bridge, SocketAddr) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let bridge =
            Bridge::serve(listener, TOKEN.into(), egui::Context::default(), timeout).unwrap();
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

    #[test]
    fn token_requires_printable_ascii_and_minimum_length() {
        assert!(valid_token(TOKEN));
        assert!(!valid_token("short"));
        assert!(!valid_token("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n"));
        assert!(!valid_token("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa "));
    }

    #[test]
    fn unauthenticated_input_is_never_dispatched() {
        let (bridge, addr) = start(Duration::from_secs(1));
        let response = request(
            addr,
            json!({"token":"bad", "command":"create_wall", "args":{}}),
        );
        assert_eq!(response["ok"], false);
        assert!(bridge.try_recv().is_none());
    }

    #[test]
    fn authenticated_command_is_dispatched_and_response_returned() {
        let (bridge, addr) = start(Duration::from_secs(1));
        let client = thread::spawn(move || {
            request(
                addr,
                json!({"token":TOKEN, "command":"create_wall", "args":{"x":1}, "document_id":"doc", "expected_revision":7}),
            )
        });
        let end = Instant::now() + Duration::from_secs(1);
        let req = loop {
            if let Some(req) = bridge.try_recv() {
                break req;
            }
            assert!(Instant::now() < end, "request never arrived");
            thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(req.command, "create_wall");
        assert_eq!(req.args["x"], 1);
        assert_eq!(req.document_id.as_deref(), Some("doc"));
        assert_eq!(req.expected_revision, Some(7));
        req.respond
            .send(json!({"ok":true,"result":{"id":"wall"}}))
            .unwrap();
        assert_eq!(client.join().unwrap()["result"]["id"], "wall");
    }

    #[test]
    fn expires_queued_requests_instead_of_late_execution() {
        let (bridge, addr) = start(Duration::from_millis(120));
        let response = request(
            addr,
            json!({"token":TOKEN, "command":"create_wall", "args":{}}),
        );
        assert!(response["error"].as_str().unwrap().contains("expired"));
        assert!(bridge.try_recv().is_none());
    }

    #[test]
    fn started_but_timed_out_reports_unknown_outcome() {
        let (bridge, addr) = start(Duration::from_millis(120));
        let client = thread::spawn(move || {
            request(
                addr,
                json!({"token":TOKEN, "command":"create_wall", "args":{}}),
            )
        });
        let end = Instant::now() + Duration::from_secs(1);
        let req = loop {
            if let Some(req) = bridge.try_recv() {
                break req;
            }
            assert!(Instant::now() < end);
            thread::sleep(Duration::from_millis(2));
        };
        assert!(client.join().unwrap()["error"]
            .as_str()
            .unwrap()
            .contains("outcome is unknown"));
        assert!(req.respond.send(json!({"ok":true})).is_err());
    }

    #[test]
    fn missing_newline_and_oversized_requests_are_rejected() {
        let (bridge, addr) = start(Duration::from_secs(1));
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream.write_all(b"{} ").unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["ok"], false);
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let _ = stream.write_all(&vec![b'x'; MAX_REQUEST + 1]);
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["ok"], false);
        assert!(bridge.try_recv().is_none());
    }

    #[test]
    fn read_deadline_is_absolute_even_when_bytes_keep_arriving() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let client = thread::spawn(move || {
            let mut s = TcpStream::connect(addr).unwrap();
            for _ in 0..10 {
                if s.write_all(b"x").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(35));
            }
        });
        let (mut s, _) = listener.accept().unwrap();
        let start = Instant::now();
        assert!(read_message(&mut s, Duration::from_millis(120)).is_err());
        assert!(start.elapsed() < Duration::from_millis(280));
        drop(s);
        client.join().unwrap();
    }
}
