//! The demo player: plays a script of pointer moves, clicks and keys into
//! the app as if a person were using it, and records the window frame by
//! frame. For tutorials and product films; off unless `CREABIM_DEMO` names
//! a script.
//!
//! Script time runs at a fixed 30 frames a second, whatever the machine's
//! speed, so a recording comes out the same every time. The real mouse and
//! keyboard are ignored while a script plays.
//!
//! Each frame the window is captured (with its web panes) and piped to
//! ffmpeg as `capture.mkv`; `frames.jsonl` beside it gives the cursor, the
//! clicks and the captions per frame, for the film to draw over it.

use super::CreaBimApp;
use crate::webui::Pane;
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2};
use glam::DVec2;
use serde::Deserialize;
use serde_json::Value;
use std::io::Write;
use std::process::{Child, Command, Stdio};

pub const FPS: f64 = 30.0;

/// Where the cursor goes.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Target {
    /// A point of the model in the 2D view, in metres.
    pub plan: Option<[f64; 2]>,
    /// A point of the 3D view, as fractions of its width and height.
    pub view3d: Option<[f32; 2]>,
    /// A point of the model (metres, z up) where the 3D view shows it.
    pub world: Option<[f32; 3]>,
    /// A point of the window, in points.
    pub at: Option<[f32; 2]>,
    /// A button or row of the web panes, by its action id.
    pub act: Option<String>,
    /// A web pane element by CSS selector.
    pub sel: Option<String>,
    /// A web pane button, row or menu item by its text (within `sel`).
    pub text: Option<String>,
    /// A native widget that told the player its place with [`seen`].
    pub ui: Option<String>,
}

thread_local! {
    /// Native widgets drawn this frame, and last frame, by label.
    static WIDGETS: std::cell::RefCell<[std::collections::HashMap<String, egui::Rect>; 2]> =
        Default::default();
}

/// A native widget a script can click by its label: the panes' egui
/// widgets have no page to ask, so the ones a script needs say where they
/// are drawn.
pub fn seen(label: &str, rect: egui::Rect) {
    WIDGETS.with(|w| {
        w.borrow_mut()[0].insert(label.to_string(), rect);
    });
}

/// Every native widget's label and place, from the accessibility tree egui
/// builds at the end of each frame: what lets a script click a button, a
/// checkbox or a menu item of the panes by its text. Widgets that called
/// [`seen`] keep the place they gave.
struct Kit;

impl egui::plugin::Plugin for Kit {
    fn debug_name(&self) -> &'static str {
        "demo player"
    }

    fn output_hook(&mut self, _ctx: &egui::Context, out: &mut egui::FullOutput) {
        let Some(update) = &out.platform_output.accesskit_update else {
            return;
        };
        WIDGETS.with(|w| {
            let mut w = w.borrow_mut();
            for (_, node) in &update.nodes {
                let (Some(label), Some(b)) = (node.label(), node.bounds()) else {
                    continue;
                };
                let r = egui::Rect::from_min_max(
                    Pos2::new(b.x0 as f32, b.y0 as f32),
                    Pos2::new(b.x1 as f32, b.y1 as f32),
                );
                if r.is_positive() {
                    w[0].entry(label.to_string()).or_insert(r);
                }
            }
        });
    }
}

fn widget(label: &str) -> Option<egui::Rect> {
    WIDGETS.with(|w| w.borrow()[1].get(label).copied())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Step {
    /// Size the window's inside, in pixels.
    Size {
        w: f32,
        h: f32,
    },
    /// Start writing frames.
    Record,
    Wait {
        s: f64,
    },
    Move {
        #[serde(flatten)]
        to: Target,
        #[serde(default = "glide")]
        s: f64,
    },
    Click {
        #[serde(flatten)]
        to: Target,
        #[serde(default = "glide")]
        s: f64,
        #[serde(default)]
        double: bool,
        /// With the right button, for context menus.
        #[serde(default)]
        right: bool,
    },
    Drag {
        from: Target,
        to: Target,
        #[serde(default = "glide")]
        s: f64,
    },
    Key {
        key: String,
        #[serde(default)]
        ctrl: bool,
        #[serde(default)]
        shift: bool,
    },
    /// Typed a character at a time.
    Type {
        text: String,
    },
    /// Select all the text of the field with the keyboard focus, so what is
    /// typed next replaces it (Ctrl+A would select every element).
    SelectAll,
    /// Typed into a web pane field a character at a time, then entered.
    WebType {
        #[serde(flatten)]
        to: Target,
        value: String,
    },
    /// Press a web button straight away, without the cursor.
    Act {
        id: String,
    },
    /// Have the film close in on a region round a target (w × h points),
    /// from here on; `off` goes back to the whole window.
    Focus {
        #[serde(flatten)]
        to: Target,
        #[serde(default)]
        w: f32,
        #[serde(default)]
        h: f32,
        #[serde(default)]
        off: bool,
    },
    /// The caption from here on.
    Caption {
        text: String,
    },
    /// Scroll the wheel over the cursor.
    Wheel {
        y: f32,
        #[serde(default = "glide")]
        s: f64,
    },
    /// Write every clickable thing on screen to a file, to learn the UI:
    /// native widgets by label, and the web panes' actions and texts.
    Survey {
        path: String,
    },
    /// Save the project (.creabim), for the next script to open.
    Save {
        path: String,
    },
    /// Open a project, as File › Open would.
    Open {
        path: String,
    },
    /// Write the model as JSON, to check what a script built.
    Dump {
        path: String,
    },
    /// Frame the 2D view on a point of the model, at points per metre.
    Camera {
        center: [f64; 2],
        scale: f32,
    },
    End,
}

fn glide() -> f64 {
    0.6
}

/// What the current step is doing.
enum Phase {
    Start,
    /// Waiting for a web pane to say where an element is (frames asked).
    Asking {
        since: u64,
    },
    Glide {
        from: Pos2,
        to: Pos2,
        n: u64,
        i: u64,
    },
    Press {
        n: u64,
    },
    Hold {
        n: u64,
    },
    Typing {
        chars: Vec<char>,
        i: usize,
    },
}

pub struct Demo {
    steps: Vec<Step>,
    i: usize,
    phase: Phase,
    /// Frames played (script time).
    frame: u64,
    cursor: Pos2,
    down: bool,
    events: Vec<Event>,
    /// A web element's place, once a pane has answered.
    found: Option<egui::Rect>,
    recording: bool,
    written: u64,
    out: std::path::PathBuf,
    ffmpeg: Option<Child>,
    size: Option<(u32, u32)>,
    log: Option<std::fs::File>,
    caption: String,
    clicked: bool,
    /// The click under way is with the right button.
    right: bool,
    /// The accessibility tree is on, for finding native widgets.
    kit: bool,
    /// The region the film closes in on, in points.
    focus: Option<[f32; 4]>,
    /// Frames held while the window wasn't drawing.
    held: u64,
    pub done: bool,
}

impl Demo {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("CREABIM_DEMO")?;
        let text = std::fs::read_to_string(&path).ok()?;
        let steps: Vec<Step> = match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("demo script: {e}");
                return None;
            }
        };
        let out = std::env::var_os("CREABIM_DEMO_OUT")
            .map(Into::into)
            .unwrap_or_else(|| std::path::PathBuf::from("demo-out"));
        let _ = std::fs::create_dir_all(&out);
        Some(Self {
            steps,
            i: 0,
            phase: Phase::Start,
            frame: 0,
            cursor: Pos2::new(700.0, 400.0),
            down: false,
            events: vec![],
            found: None,
            recording: false,
            written: 0,
            out,
            ffmpeg: None,
            size: None,
            log: None,
            caption: String::new(),
            clicked: false,
            right: false,
            kit: false,
            focus: None,
            held: 0,
            done: false,
        })
    }

    /// Replace the real pointer and keys with the script's, on script time.
    pub fn input(&mut self, raw: &mut egui::RawInput) {
        raw.events.retain(|e| {
            !matches!(
                e,
                Event::PointerMoved(_)
                    | Event::PointerButton { .. }
                    | Event::PointerGone
                    | Event::MouseWheel { .. }
                    | Event::Key { .. }
                    | Event::Text(_)
                    | Event::Zoom(_)
                    | Event::MouseMoved(_)
                    | Event::WindowFocused(_)
            )
        });
        // the window keeps the keyboard while a script types, even when
        // another app is in front: egui drops focus from an unfocused window
        raw.focused = true;
        raw.events.append(&mut self.events);
        raw.time = Some(self.frame as f64 / FPS);
        raw.predicted_dt = (1.0 / FPS) as f32;
    }

    /// A web pane said where the element asked for is.
    pub fn found(&mut self, pane_rect: egui::Rect, b: &Value) {
        // a pane parked off the window keeps stale items
        if pane_rect.min.x < -1000.0 || !pane_rect.is_positive() {
            return;
        }
        let f = |k: &str| b[k].as_f64().unwrap_or(0.0) as f32;
        let r = egui::Rect::from_min_size(
            pane_rect.min + egui::vec2(f("x"), f("y")),
            egui::vec2(f("w"), f("h")),
        );
        self.found = Some(r);
    }

    fn moved(&mut self, p: Pos2) {
        self.cursor = p;
        self.events.push(Event::PointerMoved(p));
    }

    fn button(&mut self, pressed: bool) {
        self.down = pressed;
        if pressed {
            self.clicked = true;
        }
        self.events.push(Event::PointerButton {
            pos: self.cursor,
            button: if self.right {
                PointerButton::Secondary
            } else {
                PointerButton::Primary
            },
            pressed,
            modifiers: Modifiers::NONE,
        });
    }

    fn next(&mut self) {
        self.i += 1;
        self.phase = Phase::Start;
        self.found = None;
    }

    /// Write the frame just shown, with what the cursor was doing in it.
    /// Write the frame just shown; false when the window wasn't drawn
    /// (minimised, or its surface not yet the window's size).
    fn write(&mut self, hwnd: isize, force: bool) -> bool {
        let Some((w, h, px)) = grab::window(hwnd) else {
            return false;
        };
        // the right edge is the properties panel: black there means the
        // views weren't drawn into this capture
        let mut black = 0;
        let mut n = 0;
        for y in (h / 4..h * 3 / 4).step_by(8) {
            let i = ((y * w + w - 12) * 4) as usize;
            n += 1;
            if px[i] < 10 && px[i + 1] < 10 && px[i + 2] < 10 {
                black += 1;
            }
        }
        // a dark theme's panel is not black: past a few seconds, take it
        if black * 10 > n * 8 && !force {
            return false;
        }
        let (w0, h0) = *self.size.get_or_insert((w, h));
        if self.ffmpeg.is_none() {
            let child = Command::new("ffmpeg")
                .args([
                    "-y",
                    "-loglevel",
                    "error",
                    "-f",
                    "rawvideo",
                    "-pix_fmt",
                    "bgra",
                ])
                .args(["-s", &format!("{w0}x{h0}"), "-r", "30", "-i", "-"])
                .args(["-c:v", "libx264rgb", "-preset", "ultrafast", "-qp", "0"])
                .arg(self.out.join("capture.mkv"))
                .stdin(Stdio::piped())
                .spawn();
            match child {
                Ok(c) => self.ffmpeg = Some(c),
                Err(e) => {
                    eprintln!("demo: ffmpeg: {e}");
                    self.done = true;
                    return false;
                }
            }
            self.log = std::fs::File::create(self.out.join("frames.jsonl")).ok();
        }
        // a window that changed size is cropped or padded to the first size
        let mut frame = vec![0u8; (w0 * h0 * 4) as usize];
        for y in 0..h0.min(h) {
            let n = (w0.min(w) * 4) as usize;
            let src = (y * w * 4) as usize;
            let dst = (y * w0 * 4) as usize;
            frame[dst..dst + n].copy_from_slice(&px[src..src + n]);
        }
        if let Some(stdin) = self.ffmpeg.as_mut().and_then(|c| c.stdin.as_mut()) {
            let _ = stdin.write_all(&frame);
        }
        if let Some(log) = &mut self.log {
            let line = serde_json::json!({
                "f": self.written,
                "x": self.cursor.x, "y": self.cursor.y,
                "down": self.down, "click": self.clicked,
                "caption": self.caption,
                "focus": self.focus,
            });
            let _ = writeln!(log, "{line}");
        }
        self.clicked = false;
        self.written += 1;
        true
    }

    fn finish(&mut self) {
        if let Some(mut c) = self.ffmpeg.take() {
            drop(c.stdin.take());
            let _ = c.wait();
        }
        self.done = true;
    }
}

/// A JavaScript expression for the element a target names, or null.
fn find(t: &Target) -> Option<String> {
    let js = |v: &str| serde_json::to_string(v).unwrap();
    if let Some(text) = &t.text {
        let sel = t
            .sel
            .as_deref()
            .unwrap_or("button,.row,.mi,.tool,.group,.menubtn,label,.field");
        return Some(format!(
            "([...document.querySelectorAll({})].find(e=>{{const t=e.textContent.trim();return (t==={t}||(e.matches('.field')&&t.startsWith({t})))&&e.getClientRects().length}})||null)",
            js(sel),
            t = js(text)
        ));
    }
    let sel = t
        .sel
        .clone()
        .or_else(|| t.act.as_ref().map(|a| format!("[data-act=\"{a}\"]")))?;
    Some(format!("document.querySelector({})", js(&sel)))
}

/// The JavaScript that finds an element in a pane and reports its place.
fn ask_script(e: &str) -> String {
    format!(
        "(()=>{{const e={e};if(!e)return;\
         e.scrollIntoView({{block:'nearest'}});const r=e.getBoundingClientRect();\
         window.ipc.postMessage(JSON.stringify({{t:'demo_rect',x:r.left,y:r.top,w:r.width,h:r.height}}))}})()"
    )
}

/// Hover and press looks for the element the cursor is on, since the real
/// pointer never reaches the pages.
fn mark_script(e: &str, class: &str) -> String {
    format!(
        "(()=>{{if(!document.getElementById('demo-css')){{const s=document.createElement('style');\
         s.id='demo-css';s.textContent='.demo-hover{{background:var(--raise)!important;color:var(--text)!important}}';\
         document.head.appendChild(s)}}document.querySelectorAll('.demo-hover').forEach(x=>x.classList.remove('demo-hover'));\
         const e={e};if(e){{{class}}}}})()"
    )
}

const PANES: [Pane; 5] = [Pane::Top, Pane::Left, Pane::Status, Pane::Nav, Pane::Popup];

fn selector(t: &Target) -> Option<String> {
    find(t)
}

impl CreaBimApp {
    /// Where a target is on the window, or None while a pane is asked.
    fn demo_point(&mut self, t: &Target) -> Option<Pos2> {
        if let Some([x, y]) = t.at {
            return Some(Pos2::new(x, y));
        }
        if let Some([x, y]) = t.plan {
            let rect = self.rect_2d?;
            let view = self.active_view?;
            let (center, scale) = *self.plan.cameras.get(&view)?;
            let xf = crate::planview::Xf {
                rect,
                center,
                scale,
            };
            return Some(xf.to_screen(DVec2::new(x, y)));
        }
        if let Some([x, y, z]) = t.world {
            let r = self.rect_3d?;
            let ndc = self
                .camera
                .view_proj(r.width() / r.height())
                .project_point3(glam::Vec3::new(x, y, z));
            return Some(
                r.center() + egui::vec2(ndc.x * r.width() / 2.0, -ndc.y * r.height() / 2.0),
            );
        }
        if let Some(label) = &t.ui {
            if let Some(r) = widget(label) {
                return Some(r.center());
            }
            // "Width field": the value a Properties row draws right of its
            // label "Width"
            let base = label.strip_suffix(" field")?;
            return widget(base).map(|r| Pos2::new(r.max.x + 30.0, r.center().y));
        }
        if let Some([fx, fy]) = t.view3d {
            let r = self.rect_3d?;
            return Some(r.min + egui::vec2(r.width() * fx, r.height() * fy));
        }
        let demo = self.demo.as_mut()?;
        demo.found.map(|r| r.center())
    }

    /// Ask the web panes where a target is.
    fn demo_ask(&self, t: &Target) {
        let (Some(sel), Some(web)) = (selector(t), &self.shell.web) else {
            return;
        };
        for pane in PANES {
            web.run(pane, &ask_script(&sel));
        }
    }

    fn demo_mark(&self, t: &Target, class: &str) {
        let (Some(sel), Some(web)) = (selector(t), &self.shell.web) else {
            return;
        };
        for pane in PANES {
            web.run(pane, &mark_script(&sel, class));
        }
    }

    /// Record the last frame and move the script on by one frame.
    pub(super) fn demo_frame(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        ctx.request_repaint();
        let Some(mut demo) = self.demo.take() else {
            return;
        };
        if !demo.kit {
            demo.kit = true;
            ctx.enable_accesskit();
            ctx.add_plugin(Kit);
        }
        if demo.done {
            self.demo = Some(demo);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        WIDGETS.with(|w| {
            let mut w = w.borrow_mut();
            w[1] = std::mem::take(&mut w[0]);
        });
        // let the web panes catch up before the window is taken
        std::thread::sleep(std::time::Duration::from_millis(25));
        let asking = matches!(demo.phase, Phase::Asking { .. });
        if demo.recording && !asking {
            if let Some(hwnd) = hwnd(frame) {
                let force = demo.held > 150;
                if demo.write(hwnd, force) {
                    demo.held = 0;
                } else {
                    // hold the script until the window draws again
                    demo.held += 1;
                    if demo.held % 150 == 1 {
                        eprintln!("demo: the window isn't drawing; holding the script");
                    }
                    self.demo = Some(demo);
                    std::thread::sleep(std::time::Duration::from_millis(40));
                    return;
                }
            }
        }
        let lock = demo.frame % 15 == 0;
        self.demo = Some(demo);
        // the pages would still follow the real mouse (hover, a menu bar
        // switching menus): while a script plays they take no pointer at
        // all, and the script's clicks reach them from inside the page
        if lock {
            if let Some(web) = &self.shell.web {
                for pane in PANES {
                    web.run(
                        pane,
                        "if(!document.getElementById('demo-lock')){const s=document.createElement('style');                         s.id='demo-lock';s.textContent='*{pointer-events:none!important}';document.head.appendChild(s)}",
                    );
                }
            }
        }
        self.demo_step(ctx);
        if let Some(d) = &mut self.demo {
            if !matches!(d.phase, Phase::Asking { .. }) {
                d.frame += 1;
            }
        }
    }

    fn demo_step(&mut self, ctx: &egui::Context) {
        let Some(demo) = &self.demo else { return };
        let Some(step) = demo.steps.get(demo.i).cloned() else {
            self.demo.as_mut().unwrap().finish();
            return;
        };
        let frames = |s: f64| ((s * FPS).round() as u64).max(1);
        match step {
            Step::Size { w, h } => {
                let ppp = ctx.pixels_per_point();
                // no title bar, so the whole height fits on a 1080p screen
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                    w / ppp,
                    h / ppp,
                )));
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(0.0, 0.0)));
                self.demo.as_mut().unwrap().next();
            }
            Step::Record => {
                let d = self.demo.as_mut().unwrap();
                d.recording = true;
                d.next();
            }
            Step::Focus { to, w, h, off } => {
                let at = if off { None } else { self.demo_point(&to) };
                let d = self.demo.as_mut().unwrap();
                d.focus = at.map(|c| [c.x - w / 2.0, c.y - h / 2.0, c.x + w / 2.0, c.y + h / 2.0]);
                d.next();
            }
            Step::Caption { text } => {
                let d = self.demo.as_mut().unwrap();
                d.caption = text;
                d.next();
            }
            Step::Wait { s } => {
                let d = self.demo.as_mut().unwrap();
                match &mut d.phase {
                    Phase::Start => d.phase = Phase::Hold { n: frames(s) },
                    Phase::Hold { n } if *n > 1 => *n -= 1,
                    _ => d.next(),
                }
            }
            Step::Act { id } => {
                if let Some(web) = &self.shell.web {
                    let sel = serde_json::to_string(&format!("[data-act=\"{id}\"]")).unwrap();
                    for pane in PANES {
                        web.run(pane, &format!("document.querySelector({sel})?.click()"));
                    }
                }
                self.demo.as_mut().unwrap().next();
            }
            Step::Key { key, ctrl, shift } => {
                let d = self.demo.as_mut().unwrap();
                if let Some(k) = Key::from_name(&key) {
                    let modifiers = Modifiers {
                        ctrl,
                        command: ctrl,
                        shift,
                        ..Default::default()
                    };
                    for pressed in [true, false] {
                        d.events.push(Event::Key {
                            key: k,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers,
                        });
                    }
                } else {
                    eprintln!("demo: no key {key}");
                }
                d.next();
            }
            Step::Type { text } => {
                let d = self.demo.as_mut().unwrap();
                match &mut d.phase {
                    Phase::Start => {
                        d.phase = Phase::Typing {
                            chars: text.chars().collect(),
                            i: 0,
                        }
                    }
                    Phase::Typing { chars, i } => {
                        // about eight characters a second
                        if d.frame % 4 == 0 {
                            if let Some(c) = chars.get(*i).copied() {
                                *i += 1;
                                d.events.push(Event::Text(c.to_string()));
                            } else {
                                d.next();
                            }
                        }
                    }
                    _ => d.next(),
                }
            }
            Step::WebType { to, value } => {
                let chars: Vec<char> = value.chars().collect();
                let d = self.demo.as_mut().unwrap();
                let i = match &mut d.phase {
                    Phase::Start => {
                        d.phase = Phase::Typing {
                            chars: chars.clone(),
                            i: 0,
                        };
                        0
                    }
                    Phase::Typing { i, .. } => {
                        if d.frame % 4 == 0 {
                            *i += 1;
                        }
                        *i
                    }
                    _ => chars.len() + 1,
                };
                let typed: String = chars.iter().take(i).collect();
                let done = i > chars.len();
                if let (Some(e), Some(web)) = (find(&to), &self.shell.web) {
                    let v = serde_json::to_string(&typed).unwrap();
                    let end = if done {
                        "e.dispatchEvent(new Event('change',{bubbles:true}));e.blur();"
                    } else {
                        ""
                    };
                    for pane in PANES {
                        web.run(
                            pane,
                            &format!("(()=>{{let e={e};if(e&&!e.matches('input,textarea'))e=e.querySelector('input,textarea');if(!e)return;e.focus();e.value={v};e.dispatchEvent(new Event('input',{{bubbles:true}}));{end}}})()"),
                        );
                    }
                }
                if done {
                    self.demo.as_mut().unwrap().next();
                }
            }
            Step::Wheel { y, s } => {
                let d = self.demo.as_mut().unwrap();
                match &mut d.phase {
                    Phase::Start => d.phase = Phase::Hold { n: frames(s) },
                    Phase::Hold { n } if *n > 1 => {
                        let total = frames(s);
                        *n -= 1;
                        d.events.push(Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Line,
                            delta: egui::vec2(0.0, y / total as f32),
                            modifiers: Modifiers::NONE,
                            phase: egui::TouchPhase::Move,
                        });
                    }
                    _ => d.next(),
                }
            }
            Step::Move { to, s } => self.demo_glide(&to, s, None),
            Step::Click {
                to,
                s,
                double,
                right,
            } => {
                self.demo.as_mut().unwrap().right = right;
                self.demo_glide(&to, s, Some(double))
            }
            Step::Drag { from, to, s } => {
                self.demo.as_mut().unwrap().right = false;
                self.demo_drag(&from, &to, s)
            }
            Step::Camera { center, scale } => {
                if let Some(view) = self.active_view {
                    let c = DVec2::new(center[0], center[1]);
                    self.plan.cameras.insert(view, (c, scale));
                }
                self.demo.as_mut().unwrap().next();
            }
            Step::SelectAll => {
                if let Some(id) = ctx.memory(|m| m.focused()) {
                    if let Some(mut st) = egui::text_edit::TextEditState::load(ctx, id) {
                        st.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(100_000),
                        )));
                        st.store(ctx, id);
                    }
                }
                self.demo.as_mut().unwrap().next();
            }
            Step::Survey { path } => {
                let native: Vec<Value> = WIDGETS.with(|w| {
                    w.borrow()[1]
                        .iter()
                        .map(|(l, r)| serde_json::json!({"label": l, "x": r.center().x, "y": r.center().y}))
                        .collect()
                });
                let _ = std::fs::write(
                    &path,
                    serde_json::to_string_pretty(&serde_json::json!({"native": native}))
                        .unwrap_or_default(),
                );
                if let Some(web) = &self.shell.web {
                    let file = serde_json::to_string(&format!("{path}.web")).unwrap();
                    for pane in PANES {
                        web.run(
                            pane,
                            &format!("(()=>{{const out=[...document.querySelectorAll('[data-act],[data-popup],.row,.mi,label.field,button')]                             .filter(e=>e.getClientRects().length).map(e=>({{act:e.dataset.act||e.dataset.popup||'',text:e.textContent.trim().slice(0,60),title:(e.title||'').slice(0,80)}}));                             window.ipc.postMessage(JSON.stringify({{t:'demo_survey',file:{file},items:out}}))}})()"),
                        );
                    }
                }
                self.demo.as_mut().unwrap().next();
            }
            Step::Save { path } => {
                if let Err(e) = creabim_store::save(&path, &self.doc) {
                    eprintln!("demo: save {path}: {e}");
                }
                self.demo.as_mut().unwrap().next();
            }
            Step::Open { path } => {
                self.open_path(&path);
                self.demo.as_mut().unwrap().next();
            }
            Step::Dump { path } => {
                if let Ok(text) = serde_json::to_string_pretty(self.doc.model()) {
                    let _ = std::fs::write(path, text);
                }
                self.demo.as_mut().unwrap().next();
            }
            Step::End => self.demo.as_mut().unwrap().finish(),
        }
    }

    /// Glide to a target, then click it if asked.
    fn demo_glide(&mut self, to: &Target, s: f64, click: Option<bool>) {
        let web = selector(to).is_some();
        let phase = std::mem::replace(&mut self.demo.as_mut().unwrap().phase, Phase::Start);
        match phase {
            Phase::Start | Phase::Asking { .. } => {
                if web && matches!(phase, Phase::Start) {
                    self.demo_ask(to);
                    let d = self.demo.as_mut().unwrap();
                    d.phase = Phase::Asking { since: 0 };
                    return;
                }
                let Some(p) = self.demo_point(to) else {
                    let d = self.demo.as_mut().unwrap();
                    match phase {
                        // a pane that never answers: the element is not there
                        Phase::Asking { since } if since < 90 => {
                            d.phase = Phase::Asking { since: since + 1 };
                            // a popup's page may not have its items yet
                            if since % 6 == 5 {
                                self.demo_ask(to);
                            }
                        }
                        _ => {
                            eprintln!("demo: step {} has no target", d.i);
                            d.next();
                        }
                    }
                    return;
                };
                if web {
                    self.demo_mark(to, "e.classList.add('demo-hover')");
                }
                let d = self.demo.as_mut().unwrap();
                d.phase = Phase::Glide {
                    from: d.cursor,
                    to: p,
                    n: ((s * FPS).round() as u64).max(1),
                    i: 0,
                };
            }
            Phase::Glide { from, to: p, n, i } => {
                let d = self.demo.as_mut().unwrap();
                let t = (i + 1) as f32 / n as f32;
                // ease in and out
                let e = t * t * (3.0 - 2.0 * t);
                d.moved(from + (p - from) * e);
                if i + 1 < n {
                    d.phase = Phase::Glide {
                        from,
                        to: p,
                        n,
                        i: i + 1,
                    };
                } else if click.is_some() {
                    d.phase = Phase::Press { n: 0 };
                } else {
                    d.next();
                }
            }
            Phase::Press { n } => {
                let double = click == Some(true);
                let d = self.demo.as_mut().unwrap();
                // press, release a frame later; twice for a double-click
                // a page takes its own click: the views must not see one,
                // or they close the menu it opens
                match n {
                    _ if web => {
                        d.down = n % 2 == 0;
                        d.clicked |= d.down;
                    }
                    0 | 2 => d.button(true),
                    1 | 3 => d.button(false),
                    _ => {}
                }
                let last = if double { 3 } else { 1 };
                if n == 0 && web {
                    if let (Some(e), Some(w)) = (selector(to), &self.shell.web) {
                        let act = if double {
                            "e.click();e.dispatchEvent(new MouseEvent('dblclick',{bubbles:true}))"
                        } else {
                            "e.click()"
                        };
                        for pane in PANES {
                            w.run(pane, &format!("(()=>{{const e={e};if(e){{{act}}}}})()"));
                        }
                    }
                }
                let d = self.demo.as_mut().unwrap();
                if n < last {
                    d.phase = Phase::Press { n: n + 1 };
                } else {
                    // a short settle after the click
                    d.phase = Phase::Hold { n: 4 };
                }
            }
            Phase::Hold { n } => {
                let d = self.demo.as_mut().unwrap();
                if n > 1 {
                    d.phase = Phase::Hold { n: n - 1 };
                } else {
                    if web {
                        self.demo_mark(to, "");
                    }
                    self.demo.as_mut().unwrap().next();
                }
            }
            Phase::Typing { .. } => self.demo.as_mut().unwrap().next(),
        }
    }

    fn demo_drag(&mut self, from: &Target, to: &Target, s: f64) {
        let phase = std::mem::replace(&mut self.demo.as_mut().unwrap().phase, Phase::Start);
        match phase {
            Phase::Start => {
                let (Some(a), Some(b)) = (self.demo_point(from), self.demo_point(to)) else {
                    eprintln!("demo: drag without a target");
                    self.demo.as_mut().unwrap().next();
                    return;
                };
                let d = self.demo.as_mut().unwrap();
                d.moved(a);
                d.button(true);
                d.phase = Phase::Glide {
                    from: a,
                    to: b,
                    n: ((s * FPS).round() as u64).max(1),
                    i: 0,
                };
            }
            Phase::Glide { from, to: p, n, i } => {
                let d = self.demo.as_mut().unwrap();
                let t = (i + 1) as f32 / n as f32;
                let e = t * t * (3.0 - 2.0 * t);
                d.moved(from + (p - from) * e);
                if i + 1 < n {
                    d.phase = Phase::Glide {
                        from,
                        to: p,
                        n,
                        i: i + 1,
                    };
                } else {
                    d.phase = Phase::Press { n: 0 };
                }
            }
            _ => {
                let d = self.demo.as_mut().unwrap();
                d.button(false);
                d.next();
            }
        }
    }
}

fn hwnd(frame: &eframe::Frame) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match frame.window_handle().ok()?.as_raw() {
        #[cfg(windows)]
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

#[cfg(windows)]
mod grab {
    //! The window as the screen shows it, web panes and all, even when
    //! other windows cover it.
    use std::ffi::c_void;
    type H = *mut c_void;

    #[repr(C)]
    struct Rect {
        l: i32,
        t: i32,
        r: i32,
        b: i32,
    }

    #[repr(C)]
    struct Header {
        size: u32,
        width: i32,
        height: i32,
        planes: u16,
        bits: u16,
        compression: u32,
        size_image: u32,
        x_ppm: i32,
        y_ppm: i32,
        used: u32,
        important: u32,
        colours: [u32; 4],
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetClientRect(h: H, r: *mut Rect) -> i32;
        fn GetDC(h: H) -> H;
        fn ReleaseDC(h: H, dc: H) -> i32;
        fn PrintWindow(h: H, dc: H, flags: u32) -> i32;
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn CreateCompatibleDC(dc: H) -> H;
        fn CreateCompatibleBitmap(dc: H, w: i32, h: i32) -> H;
        fn SelectObject(dc: H, o: H) -> H;
        fn DeleteObject(o: H) -> i32;
        fn DeleteDC(dc: H) -> i32;
        fn GetDIBits(
            dc: H,
            bm: H,
            start: u32,
            lines: u32,
            bits: *mut c_void,
            bi: *mut Header,
            usage: u32,
        ) -> i32;
    }

    /// Width, height and BGRA pixels, top row first.
    pub fn window(hwnd: isize) -> Option<(u32, u32, Vec<u8>)> {
        let h = hwnd as H;
        unsafe {
            let mut r = Rect {
                l: 0,
                t: 0,
                r: 0,
                b: 0,
            };
            GetClientRect(h, &mut r);
            let (w, ht) = (r.r - r.l, r.b - r.t);
            if w <= 0 || ht <= 0 {
                return None;
            }
            let screen = GetDC(h);
            let dc = CreateCompatibleDC(screen);
            let bm = CreateCompatibleBitmap(screen, w, ht);
            let old = SelectObject(dc, bm);
            // client area only, with what DirectComposition draws
            let ok = PrintWindow(h, dc, 1 | 2) != 0;
            let mut hd = Header {
                size: 40,
                width: w,
                height: -ht,
                planes: 1,
                bits: 32,
                compression: 0,
                size_image: 0,
                x_ppm: 0,
                y_ppm: 0,
                used: 0,
                important: 0,
                colours: [0; 4],
            };
            let mut px = vec![0u8; (w * ht * 4) as usize];
            SelectObject(dc, old);
            let lines = GetDIBits(
                dc,
                bm,
                0,
                ht as u32,
                px.as_mut_ptr() as *mut c_void,
                &mut hd,
                0,
            );
            DeleteObject(bm);
            DeleteDC(dc);
            ReleaseDC(h, screen);
            (ok && lines > 0).then_some((w as u32, ht as u32, px))
        }
    }
}

#[cfg(not(windows))]
mod grab {
    pub fn window(_hwnd: isize) -> Option<(u32, u32, Vec<u8>)> {
        None
    }
}
