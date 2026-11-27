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

/// The last lines, for the Log view (MU40): what the file holds lately,
/// without reading it back.
static RECENT: Mutex<std::collections::VecDeque<String>> =
    Mutex::new(std::collections::VecDeque::new());

/// How many lines the Log view keeps.
const KEEP: usize = 4000;

/// The lines written lately, oldest first, and how many were ever written
/// (so a view can tell whether there is anything new).
pub fn recent() -> (Vec<String>, usize) {
    let r = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    (
        r.iter().cloned().collect(),
        WRITTEN.load(std::sync::atomic::Ordering::Relaxed),
    )
}

static WRITTEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Is this line about something going wrong?
pub fn is_problem(line: &str) -> bool {
    let l = line.to_lowercase();
    l.contains("crash")
        || l.contains("couldn't")
        || l.contains("error")
        || l.contains("failed")
        || l.contains("not changed")
        || l.contains("panicked")
}

/// A timing note ("slow: between frames took 120 ms"): kept for a problem
/// report, grey in the Log, never counted as a problem (MU89: they made
/// up nearly all of its "problems", mostly the app waiting while idle).
pub fn is_timing(line: &str) -> bool {
    line.contains("slow:")
}

/// Largest the log grows before it starts again.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Where the log is kept.
pub fn path() -> Option<std::path::PathBuf> {
    let var = |k: &str| std::env::var_os(k).map(std::path::PathBuf::from);
    if let Some(dir) = var("CREABIM_LOG_DIR") {
        return Some(dir.join("activity.log"));
    }
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
    let row = format!("{t:10.3}  {}", text.as_ref());
    if let Some(f) = &mut log.file {
        let _ = writeln!(f, "{row}");
    }
    let mut r = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    // a crash's trace is many lines: each its own row in the view
    for l in row.lines() {
        if r.len() >= KEEP {
            r.pop_front();
        }
        r.push_back(l.to_string());
    }
    WRITTEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// A length in millimetres, for the log.
pub fn mm(v: f64) -> String {
    format!("{:.1}", v * 1000.0)
}

/// A point in millimetres, for the log.
pub fn pt(p: glam::DVec3) -> String {
    format!("({}, {}, {})", mm(p.x), mm(p.y), mm(p.z))
}

/// A step slower than this is lag worth knowing about, ms.
pub const SLOW_MS: f64 = 50.0;

/// Slow steps by kind: how many, the worst, the total (ms), and when the
/// last was written to the log.
type Slow = std::collections::BTreeMap<&'static str, (u32, f64, f64, Option<Instant>)>;
static SLOW: Mutex<Slow> = Mutex::new(std::collections::BTreeMap::new());

/// Note how long a step took: past [`SLOW_MS`] it counts as lag.
pub fn slow(what: &'static str, ms: f64) {
    if ms < SLOW_MS {
        return;
    }
    let write = {
        let mut t = SLOW.lock().unwrap_or_else(|e| e.into_inner());
        let e = t.entry(what).or_insert((0, 0.0, 0.0, None));
        e.0 += 1;
        e.1 = e.1.max(ms);
        e.2 += ms;
        let due = e.3.is_none_or(|w| w.elapsed().as_secs_f64() >= 1.0);
        if due {
            e.3 = Some(Instant::now());
        }
        due
    };
    if write {
        line(format!("slow: {what} took {ms:.0} ms"));
    }
}

/// The lag of this run as a table: each kind of step, how often it was
/// slow, its worst and its mean, the worst first.
pub fn slow_table() -> String {
    let t = SLOW.lock().unwrap_or_else(|e| e.into_inner());
    let mut rows: Vec<_> = t.iter().collect();
    rows.sort_by(|a, b| b.1 .1.total_cmp(&a.1 .1));
    if rows.is_empty() {
        return format!(
            "No step took over {SLOW_MS:.0} ms.
"
        );
    }
    let mut out = format!(
        "{:<28} {:>6} {:>9} {:>9}
",
        "step", "times", "worst ms", "mean ms"
    );
    for (what, (n, worst, total, _)) in rows {
        out.push_str(&format!(
            "{what:<28} {n:>6} {worst:>9.0} {:>9.0}
",
            total / *n as f64
        ));
    }
    out
}

/// The timed steps of the frame under way on this thread: those still
/// open, each with what its inner steps took so far, and those finished,
/// each with the time that was its own (its inner steps taken off).
#[derive(Default)]
struct Steps {
    open: Vec<(&'static str, f64, bool)>,
    done: Vec<(&'static str, f64, bool)>,
}

thread_local! {
    static STEPS: std::cell::RefCell<Steps> = std::cell::RefCell::new(Steps::default());
}

/// When the last frame ended, to tell a wait between frames from a frame.
static FRAME_END: Mutex<Option<Instant>> = Mutex::new(None);

/// A timed step starts: its place among the open ones, for [`step_close`].
pub fn step_open(what: &'static str) -> usize {
    STEPS.with(|s| {
        let mut s = s.borrow_mut();
        s.open.push((what, 0.0, false));
        s.open.len() - 1
    })
}

/// A timed step that took `ms` ends. When it is the outermost one, a slow
/// "frame" is written to the log as what its time went on (MU40: lag
/// measured, not told): each step's own time, the longest first, so a
/// frame never reads only "frame took 161 ms".
pub fn step_close(place: usize, what: &'static str, ms: f64) {
    let breakdown = STEPS.with(|s| close_in(&mut s.borrow_mut(), place, what, ms));
    if what == "frame" && place == 0 {
        *FRAME_END.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    }
    if let Some(b) = breakdown {
        line(b);
    }
}

/// How long ago the last frame ended, ms: a long wait while the pointer
/// is held is time lost outside the frame (drawing, the graphics card).
pub fn since_frame() -> Option<f64> {
    let end = *FRAME_END.lock().unwrap_or_else(|e| e.into_inner());
    end.map(|t| t.elapsed().as_secs_f64() * 1000.0)
}

fn close_in(s: &mut Steps, place: usize, what: &'static str, ms: f64) -> Option<String> {
    if place >= s.open.len() {
        // closed out of turn, after the step around it: nothing to add
        return None;
    }
    // steps inside it that were never closed end with it
    s.open.truncate(place + 1);
    let (_, inner, parent) = s.open.pop()?;
    if let Some(around) = s.open.last_mut() {
        around.1 += ms;
        around.2 = true;
    }
    s.done.push((what, (ms - inner).max(0.0), parent));
    if !s.open.is_empty() {
        return None;
    }
    let done = std::mem::take(&mut s.done);
    if what != "frame" || ms < SLOW_MS {
        return None;
    }
    Some(breakdown(ms, &done))
}

/// "slow: frame of 161 ms: snaps_3d 80 + rest of show_3d 41 + …": each
/// step's own time, steps of one name added up, the longest first; those
/// under 3 ms are left out.
fn breakdown(ms: f64, done: &[(&'static str, f64, bool)]) -> String {
    let mut by_name: Vec<(&'static str, f64, u32, bool)> = Vec::new();
    for &(what, own, parent) in done {
        match by_name.iter_mut().find(|r| r.0 == what) {
            Some(r) => {
                r.1 += own;
                r.2 += 1;
                r.3 |= parent;
            }
            None => by_name.push((what, own, 1, parent)),
        }
    }
    by_name.sort_by(|a, b| b.1.total_cmp(&a.1));
    let parts: Vec<String> = by_name
        .iter()
        .filter(|r| r.1 >= 3.0)
        .take(8)
        .map(|&(what, own, n, parent)| {
            let rest = if parent { "rest of " } else { "" };
            let times = if n > 1 {
                format!(" ×{n}")
            } else {
                String::new()
            };
            format!("{rest}{what}{times} {own:.0}")
        })
        .collect();
    format!("slow: frame of {ms:.0} ms: {}", parts.join(" + "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slow_frame_says_where_its_time_went() {
        let mut s = Steps::default();
        let open = |s: &mut Steps, what| {
            s.open.push((what, 0.0, false));
            s.open.len() - 1
        };
        let frame = open(&mut s, "frame");
        let view = open(&mut s, "show_3d");
        let snaps = open(&mut s, "snaps_3d");
        assert_eq!(close_in(&mut s, snaps, "snaps_3d", 80.0), None);
        assert_eq!(close_in(&mut s, view, "show_3d", 121.0), None);
        for _ in 0..2 {
            let plan = open(&mut s, "show_plan");
            assert_eq!(close_in(&mut s, plan, "show_plan", 10.0), None);
        }
        let quick = open(&mut s, "properties");
        assert_eq!(close_in(&mut s, quick, "properties", 1.0), None);
        assert_eq!(
            close_in(&mut s, frame, "frame", 161.0).as_deref(),
            Some(
                "slow: frame of 161 ms: snaps_3d 80 + rest of show_3d 41 + \
                 show_plan ×2 20 + rest of frame 19"
            )
        );
        assert!(s.open.is_empty() && s.done.is_empty());
    }

    #[test]
    fn a_quick_frame_writes_nothing_and_leaves_nothing() {
        let mut s = Steps::default();
        s.open.push(("frame", 0.0, false));
        s.open.push(("show_plan", 0.0, false));
        assert_eq!(close_in(&mut s, 1, "show_plan", 4.0), None);
        assert_eq!(close_in(&mut s, 0, "frame", 9.0), None);
        assert!(s.done.is_empty());
    }

    #[test]
    fn a_step_left_open_ends_with_the_one_around_it() {
        let mut s = Steps::default();
        s.open.push(("frame", 0.0, false));
        s.open.push(("show_3d", 0.0, false));
        s.open.push(("hover_3d", 0.0, false));
        // show_3d closes while hover_3d is still open
        assert_eq!(close_in(&mut s, 1, "show_3d", 70.0), None);
        // hover_3d's own close, late, changes nothing
        assert_eq!(close_in(&mut s, 2, "hover_3d", 60.0), None);
        assert_eq!(
            close_in(&mut s, 0, "frame", 75.0).as_deref(),
            Some("slow: frame of 75 ms: show_3d 70 + rest of frame 5")
        );
    }
}
