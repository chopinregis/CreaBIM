//! The activity log: what the app did, one line at a time, in a plain
//! text file the owner can open (Help › Open the activity log) and send
//! with a report.
//!
//! It records what helps find a bug, not what the user types: drags
//! frame by frame (pointer, the point it gave, the snap and step that
//! applied, the value that resulted, how long the frame took), edits and
//! undos, tool changes. Nothing leaves the computer.
//!
//! The file is `activity.log` in the app's local data folder; when it
//! passes 4 MB it is moved to `activity.old.log` and a new one starts.

use std::io::Write;
use std::sync::Mutex;
use std::time::Instant;

struct Log {
    file: Option<std::fs::File>,
    start: Instant,
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);

/// Largest the log grows before it starts again.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Where the log is kept.
pub fn path() -> Option<std::path::PathBuf> {
    let var = |k: &str| std::env::var_os(k).map(std::path::PathBuf::from);
    let base = if cfg!(windows) {
        var("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Logs"))
    } else {
        var("XDG_STATE_HOME").or_else(|| var("HOME").map(|h| h.join(".local").join("state")))
    }?;
    Some(base.join("CreaBIM").join("activity.log"))
}

fn open() -> Option<std::fs::File> {
    let p = path()?;
    std::fs::create_dir_all(p.parent()?).ok()?;
    if std::fs::metadata(&p).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&p, p.with_file_name("activity.old.log"));
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p)
        .ok()
}

/// Start the log for this run: a header with the version and the time.
pub fn start() {
    let mut log = LOG.lock().unwrap_or_else(|e| e.into_inner());
    let mut file = open();
    if let Some(f) = &mut file {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let _ = writeln!(
            f,
            "\n=== CreaBIM {} (build {}) started, unix time {secs} ===",
            env!("CARGO_PKG_VERSION"),
            env!("CREABIM_COMMIT"),
        );
    }
    *log = Some(Log {
        file,
        start: Instant::now(),
    });
}

/// Write one line, stamped with the seconds since the app started.
pub fn line(text: impl AsRef<str>) {
    let mut log = LOG.lock().unwrap_or_else(|e| e.into_inner());
    let Some(log) = log.as_mut() else {
        return;
    };
    let t = log.start.elapsed().as_secs_f64();
    if let Some(f) = &mut log.file {
        let _ = writeln!(f, "{t:10.3}  {}", text.as_ref());
    }
}

/// A length in millimetres, for the log.
pub fn mm(v: f64) -> String {
    format!("{:.1}", v * 1000.0)
}

/// A point in millimetres, for the log.
pub fn pt(p: glam::DVec3) -> String {
    format!("({}, {}, {})", mm(p.x), mm(p.y), mm(p.z))
}
