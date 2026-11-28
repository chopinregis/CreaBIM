//! The interface around the views: menus, tools, the project browser and
//! the status bar are web pages (the webview Tauri uses, through wry),
//! placed as child windows over the egui window. The 3D and 2D views stay
//! native underneath.
//!
//! Pages talk to the app with JSON: `window.ipc.postMessage` one way and
//! `CB.state(...)` scripts the other.

use eframe::egui;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use wry::dpi::{PhysicalPosition, PhysicalSize};
use wry::{Rect, WebContext, WebView, WebViewBuilder};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Top,
    Left,
    Status,
    Popup,
    /// The 3D view's navigation buttons, over its top-right corner.
    Nav,
    /// The guide, at the right of the window while it is open.
    Help,
    /// A control's tip, over everything, right by its control (MU102).
    Tip,
}

const PANES: usize = 7;

impl Pane {
    /// In the order they are made, which stacks the popup on top.
    const ALL: [Pane; PANES] = [
        Pane::Top,
        Pane::Left,
        Pane::Status,
        Pane::Nav,
        Pane::Help,
        Pane::Popup,
        Pane::Tip,
    ];

    fn name(self) -> &'static str {
        match self {
            Pane::Top => "top",
            Pane::Left => "left",
            Pane::Status => "status",
            Pane::Popup => "popup",
            Pane::Nav => "nav",
            Pane::Help => "help",
            Pane::Tip => "tip",
        }
    }
}

/// A message from a page, with the pane it came from.
pub struct Msg {
    pub pane: Pane,
    pub body: Value,
}

pub struct WebUi {
    views: Vec<(Pane, WebView)>,
    /// The pages' shared browser profile, kept alive as long as they are.
    _context: WebContext,
    inbox: Arc<Mutex<Vec<Msg>>>,
    /// The last state sent to each pane, so unchanged state isn't re-sent.
    sent: [String; PANES],
    /// Where each pane is, in points.
    pub rects: [egui::Rect; PANES],
    /// The pages' zoom: the interface scale (#96).
    zoom: f32,
    popup_shown: bool,
    /// Frames until the shade asked to go is lifted.
    unshade: Option<u8>,
    /// The popup's page has been told it is on the window.
    popup_here: bool,
    /// Frames still to wait, off the window, after the card was told to
    /// show itself, so it has painted before it comes on (MU102).
    here_wait: u8,
    /// The panes are shaded behind a window that waits for an answer.
    shaded: bool,
    /// The popup and tip panels' solid ground, as last set.
    card_ground: Option<(u8, u8, u8, u8)>,
    /// The size each pane's page was last told it shows, in its pixels.
    told: [Option<(u32, u32)>; PANES],
    /// Which side the buttons sit in CreaBIM's own windows, sent with
    /// every state: "right" or "left".
    pub dir: &'static str,
    #[cfg(target_os = "linux")]
    x11: Option<X11Focus>,
}

const CSS: &str = include_str!("../ui/app.css");
const JS: &str = include_str!("../ui/app.js");
const ICONS: &str = include_str!("../ui/icons.js");

/// icons.js as name -> markup: one `"name": "markup",` per line.
pub fn icon_markup() -> Vec<(String, String)> {
    ICONS
        .lines()
        .filter(|l| l.starts_with('"'))
        .filter_map(|l| {
            let l = l.trim_end().trim_end_matches(',');
            let v: serde_json::Value = serde_json::from_str(&format!("{{{l}}}")).ok()?;
            let (k, v) = v.as_object()?.iter().next()?;
            Some((k.clone(), v.as_str()?.to_string()))
        })
        .collect()
}

fn page(pane: Pane) -> String {
    // the guide's pages are in its pane from the start, for searching
    let pages = match pane {
        Pane::Help => format!(
            "<script>window.HELP = {};</script>",
            crate::help::pages_json().to_string().replace("</", "<\\/")
        ),
        _ => String::new(),
    };
    format!(
        "<!doctype html><html data-pane=\"{}\"><head><meta charset=\"utf-8\"><style>{CSS}</style></head>\
         <body><div id=\"root\"></div><script>{ICONS}</script>{pages}<script>{JS}</script></body></html>",
        pane.name()
    )
}

/// Where the pages keep their browser data (cache, local storage): the
/// user's own app data, never beside the program, which an installed copy
/// can't write to (C:\Program Files). `None` leaves the platform's default.
fn data_dir() -> Option<std::path::PathBuf> {
    let var = |k: &str| std::env::var_os(k).map(std::path::PathBuf::from);
    let base = if cfg!(windows) {
        var("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Application Support"))
    } else {
        var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local").join("share")))
    }?;
    Some(base.join("CreaBIM").join("WebView"))
}

impl WebUi {
    /// Make the pages as children of the app window. Fails where child
    /// webviews aren't possible (Wayland without XWayland), and the app then
    /// keeps its egui panels.
    ///
    /// A message from a page wakes the app (`ctx`), so no frame has to run
    /// while nothing happens.
    pub fn new(frame: &eframe::Frame, ctx: &egui::Context) -> Result<Self, String> {
        let inbox: Arc<Mutex<Vec<Msg>>> = Default::default();
        let mut context = WebContext::new(data_dir());
        let mut views = vec![];
        for pane in Pane::ALL {
            let inbox = inbox.clone();
            let ctx = ctx.clone();
            let view = WebViewBuilder::new_with_web_context(&mut context)
                .with_html(page(pane))
                .with_bounds(Rect {
                    position: PhysicalPosition::new(0, 0).into(),
                    size: PhysicalSize::new(10, 10).into(),
                })
                // every pane is solid. A see-through popup or tip panel
                // (MU102) painted black where the system doesn't blend a
                // see-through child window (chopinregis/CreaBIM#112 on
                // Windows, #121 on Linux). The popup and tip panels take
                // the theme's card colour once known (`set_card_ground`),
                // so a page not yet painted shows as the card, not dark
                .with_background_color(BACKGROUND)
                .with_transparent(see_through(pane))
                .with_ipc_handler(move |r| {
                    if let Ok(body) = serde_json::from_str(r.body()) {
                        inbox.lock().unwrap().push(Msg { pane, body });
                        ctx.request_repaint();
                    }
                })
                .build_as_child(frame)
                .map_err(|e| e.to_string())?;
            views.push((pane, view));
        }
        // the tip's panel waits off the window until a tip shows
        for (pane, view) in &views {
            if *pane == Pane::Tip {
                let _ = view.set_bounds(Rect {
                    position: PhysicalPosition::new(-4000, -4000).into(),
                    size: PhysicalSize::new(10u32, 10u32).into(),
                });
            }
        }
        Ok(Self {
            views,
            _context: context,
            inbox,
            sent: Default::default(),
            rects: [egui::Rect::NOTHING; PANES],
            zoom: 1.0,
            popup_shown: false,
            unshade: None,
            popup_here: false,
            here_wait: 0,
            shaded: false,
            card_ground: None,
            told: [None; PANES],
            dir: "right",
            #[cfg(target_os = "linux")]
            x11: X11Focus::new(frame),
        })
    }

    fn view(&self, pane: Pane) -> &WebView {
        &self.views.iter().find(|(p, _)| *p == pane).unwrap().1
    }

    /// Let the pages run: on Linux they live on GTK, which needs pumping.
    pub fn pump() {
        #[cfg(target_os = "linux")]
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
    }

    pub fn take(&self) -> Vec<Msg> {
        std::mem::take(&mut *self.inbox.lock().unwrap())
    }

    #[cfg(feature = "live-mcp")]
    pub(crate) fn has_pending(&self) -> bool {
        self.inbox
            .lock()
            .map_or(true, |messages| !messages.is_empty())
    }

    /// Put a pane over `rect` (in points).
    pub fn place(&mut self, pane: Pane, rect: egui::Rect, ppp: f32) {
        let i = pane as usize;
        self.rects[i] = rect;
        // cover every pixel the band touches: rounding the corner and the
        // size apart left a one-pixel row between a band and the views,
        // which the splash's shade showed as a bright line (MU81). Placed
        // every frame, so a new interface scale (its pixels per point)
        // moves the bands at once
        let (x0, y0, w, h) = device_bounds(rect, ppp);
        let _ = self.view(pane).set_bounds(Rect {
            position: PhysicalPosition::new(x0, y0).into(),
            size: PhysicalSize::new(w, h).into(),
        });
        // a menu's or card's frame runs round its page's edge: tell the
        // page the size it really shows, so the frame's right and bottom
        // lines fall inside its window (MU151: at 125 % the page took a
        // whole pixel more than its window showed, and lost those lines)
        if matches!(pane, Pane::Popup | Pane::Tip) && self.told[i] != Some((w, h)) {
            self.told[i] = Some((w, h));
            let (cw, ch) = shown_size(w, h, ppp);
            let _ = self.view(pane).evaluate_script(&format!(
                "document.documentElement.style.setProperty('--shown-w','{cw}px');                 document.documentElement.style.setProperty('--shown-h','{ch}px')"
            ));
        }
    }

    /// Zoom every page by the interface scale (#96), so the bars grow with
    /// the slots egui gives them.
    pub fn set_zoom(&mut self, zoom: f32) {
        if self.zoom != zoom {
            for (_, view) in &self.views {
                let _ = view.zoom(f64::from(zoom));
            }
            self.zoom = zoom;
        }
    }

    /// Send a pane its state, if it changed since last time.
    pub fn send(&mut self, pane: Pane, state: &Value) {
        let mut state = state.clone();
        if let Some(o) = state.as_object_mut() {
            o.insert("dir".into(), self.dir.into());
        }
        let text = state.to_string();
        let i = pane as usize;
        if self.sent[i] == text {
            return;
        }
        let _ = self
            .view(pane)
            .evaluate_script(&format!("window.CB && CB.state({text})"));
        self.sent[i] = text;
    }

    /// Run a script in a pane's page.
    pub fn run(&self, pane: Pane, script: &str) {
        let _ = self.view(pane).evaluate_script(script);
    }

    /// Shade every pane but the popup, as egui shades what it draws behind
    /// a modal window, so the whole app dims behind the question as one.
    pub fn shade(&mut self, on: bool) {
        if !on {
            // lifted a frame later (see `tick`): the views' shade goes
            // with the frame now being drawn, which reaches the screen
            // after the pages have acted, so the bars were bright a
            // frame before the views (MU103)
            if self.shaded && self.unshade.is_none() {
                self.unshade = Some(1);
            }
            return;
        }
        self.unshade = None;
        self.set_shade(true);
    }

    /// Once a frame: lift the shade asked to go.
    pub fn tick(&mut self) -> bool {
        match self.unshade {
            Some(0) => {
                self.unshade = None;
                self.set_shade(false);
                false
            }
            Some(n) => {
                self.unshade = Some(n - 1);
                true
            }
            None => false,
        }
    }

    fn set_shade(&mut self, on: bool) {
        if self.shaded == on {
            return;
        }
        self.shaded = on;
        let script = format!("document.documentElement.classList.toggle('cb-shade', {on})");
        for pane in Pane::ALL {
            if pane != Pane::Popup {
                self.run(pane, &script);
            }
        }
    }

    /// Run a script in every pane's page.
    pub fn run_all(&self, script: &str) {
        for (_, v) in &self.views {
            let _ = v.evaluate_script(script);
        }
    }

    /// Paint the popup and tip panels' ground in the theme's card colour,
    /// so a menu or card not yet painted shows as itself rather than dark.
    pub fn set_card_ground(&mut self, c: egui::Color32) {
        let rgba = (c.r(), c.g(), c.b(), 255);
        if self.card_ground == Some(rgba) {
            return;
        }
        self.card_ground = Some(rgba);
        for (pane, view) in &self.views {
            if matches!(pane, Pane::Popup | Pane::Tip) {
                let _ = view.set_background_color(rgba);
            }
        }
    }

    /// Re-send every state next frame (after a page reloads).
    pub fn resend(&mut self, pane: Pane) {
        self.sent[pane as usize].clear();
        // a reloaded page has forgotten the size it shows
        self.told[pane as usize] = None;
    }

    /// Is a menu or card showing (not parked off the window)?
    pub fn popup_showing(&self) -> bool {
        self.popup_shown
    }

    /// Bring the popup onto the window. A card told to show itself waits
    /// two frames off the window first, to paint: true while it waits.
    pub fn show_popup(&mut self, rect: egui::Rect, ppp: f32) -> bool {
        // its page draws its card from now (see `stage_popup`)
        if !self.popup_here {
            self.popup_here = true;
            self.here_wait = if self.rects[Pane::Popup as usize].min.x < -1000.0 {
                2
            } else {
                0
            };
            self.run(
                Pane::Popup,
                "document.documentElement.classList.add('cb-here')",
            );
        }
        self.popup_shown = true;
        if self.here_wait > 0 {
            self.here_wait -= 1;
            return true;
        }
        self.place(Pane::Popup, rect, ppp);
        false
    }

    /// Lay a card's page out at its own size, off the window: it is
    /// measured and painted there, and comes on only when it is whole. A
    /// popup put on the window before its page had drawn showed as an
    /// empty pale block for a moment (MU103).
    pub fn stage_popup(&mut self, rect: egui::Rect, ppp: f32) {
        if self.popup_here {
            self.popup_here = false;
            self.run(
                Pane::Popup,
                "document.documentElement.classList.remove('cb-here')",
            );
        }
        self.place(
            Pane::Popup,
            rect.translate(egui::vec2(-8000.0, -8000.0) - rect.min.to_vec2()),
            ppp,
        );
        self.popup_shown = true;
    }

    /// The popup is parked off the window rather than hidden: GTK child
    /// windows don't always hide and show again cleanly.
    pub fn hide_popup(&mut self) {
        if self.popup_shown || self.rects[Pane::Popup as usize] == egui::Rect::NOTHING {
            self.place(Pane::Popup, PARKED, 1.0);
            self.popup_shown = false;
            if self.popup_here {
                self.popup_here = false;
                self.run(
                    Pane::Popup,
                    "document.documentElement.classList.remove('cb-here')",
                );
            }
            let _ = self
                .view(Pane::Popup)
                .evaluate_script("window.CB && CB.state({items: []})");
            self.sent[Pane::Popup as usize].clear();
        }
    }

    /// Hide the panes for the classic panels, or let them come back.
    /// Like the popup, they are parked off the window rather than hidden.
    pub fn hide_all(&mut self, hidden: bool) {
        for pane in Pane::ALL {
            if hidden {
                self.place(pane, PARKED, 1.0);
            } else {
                // placed again on the next frame
                self.rects[pane as usize] = egui::Rect::NOTHING;
            }
        }
        self.popup_shown = false;
    }

    /// Park a pane off the window, as for the popup.
    pub fn park(&mut self, pane: Pane) {
        self.place(pane, PARKED, 1.0);
    }

    /// Give a pane the keyboard, for typing in it straight away.
    pub fn focus(&mut self, pane: Pane) {
        let _ = self.view(pane).focus();
    }

    /// Hand the keyboard back to the views after a click on a page.
    pub fn focus_views(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(x) = &self.x11 {
            x.focus();
            return;
        }
        let _ = self.view(Pane::Top).focus_parent();
    }
}

const PARKED: egui::Rect = egui::Rect {
    min: egui::pos2(-4000.0, -4000.0),
    max: egui::pos2(-3990.0, -3990.0),
};

/// The page background, so a page never flashes white while loading.
const BACKGROUND: (u8, u8, u8, u8) = (23, 25, 30, 255);

/// A pane's window in device pixels: its corner and size, covering every
/// pixel `rect` (in points) touches.
fn device_bounds(rect: egui::Rect, ppp: f32) -> (i32, i32, u32, u32) {
    let (x0, y0) = ((rect.min.x * ppp).floor(), (rect.min.y * ppp).floor());
    let (x1, y1) = ((rect.max.x * ppp).ceil(), (rect.max.y * ppp).ceil());
    (
        x0 as i32,
        y0 as i32,
        (x1 - x0).max(1.0) as u32,
        (y1 - y0).max(1.0) as u32,
    )
}

/// What a page shows of itself, in its own pixels, in a window `w` × `h`
/// device pixels: the window's size at the page's scale, not rounded up.
fn shown_size(w: u32, h: u32, ppp: f32) -> (f32, f32) {
    let ppp = if ppp > 0.0 { ppp } else { 1.0 };
    (w as f32 / ppp, h as f32 / ppp)
}

/// Is `pane` a see-through window? None is: menus, cards and tips are solid
/// panels (#112, #121), with square corners, as in 1.1.
fn see_through(_pane: Pane) -> bool {
    false
}

/// Giving the keyboard back to the app window. GTK's own call only reaches
/// the page's container, so this asks the X server directly.
#[cfg(target_os = "linux")]
struct X11Focus {
    xlib: x11_dl::xlib::Xlib,
    display: *mut x11_dl::xlib::Display,
    window: std::ffi::c_ulong,
}

#[cfg(target_os = "linux")]
impl X11Focus {
    fn new(frame: &eframe::Frame) -> Option<Self> {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let window = match frame.window_handle().ok()?.as_raw() {
            RawWindowHandle::Xlib(w) => w.window,
            _ => return None,
        };
        let xlib = x11_dl::xlib::Xlib::open().ok()?;
        let display = unsafe { (xlib.XOpenDisplay)(std::ptr::null()) };
        if display.is_null() {
            return None;
        }
        Some(Self {
            xlib,
            display,
            window,
        })
    }

    fn focus(&self) {
        unsafe {
            (self.xlib.XSetInputFocus)(
                self.display,
                self.window,
                x11_dl::xlib::RevertToParent,
                x11_dl::xlib::CurrentTime,
            );
            (self.xlib.XFlush)(self.display);
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for X11Focus {
    fn drop(&mut self) {
        unsafe { (self.xlib.XCloseDisplay)(self.display) };
    }
}

#[cfg(test)]
mod approved_icons {
    //! The toolbar and action icons are the owner's approved set, copied
    //! verbatim from `ui/approved/*.json`. Changing one needs the owner's
    //! OK: change the JSON first, then this test says where icons.js lags.

    use std::collections::BTreeMap;

    /// Each approved icon's name in the JSON, and its key in icons.js.
    const KEYS: &[(&str, &str)] = &[
        ("Select", "mouse-pointer-2"),
        ("Align", "align-start-vertical"),
        ("Wall", "wall"),
        ("Door", "door"),
        ("Shaft", "shaft"),
        ("Mass", "box"),
        ("Column", "column"),
        ("Beam", "beam"),
        ("Slab", "slab"),
        ("Stair", "stair"),
        ("Footing", "footing"),
        ("Object", "object"),
        ("Extrusion", "extrusion"),
        ("Sweep", "sweep"),
        ("Void", "void"),
        ("Room", "room"),
        ("Ceiling", "ceiling"),
        ("Window", "window"),
        ("Opening", "opening"),
        ("Roof", "roof"),
        ("Railing", "railing"),
        ("Room separator", "room-separator"),
        ("Curtain wall", "curtain-wall"),
        ("Screen wall", "screen-wall"),
        ("Fixtures", "fixtures"),
        ("Terrain", "terrain"),
        ("Building pad", "pad"),
        ("Paving", "paving"),
        ("Planting", "planting"),
        ("Section", "scissors-line-dashed"),
        ("Callout", "callout"),
        ("Elevation", "elevation"),
        ("Crop box", "crop-box"),
        ("Frame all", "frame-all"),
        ("Hidden elements", "hidden-elements"),
        ("X-ray", "xray"),
        ("Dimension", "dimension"),
        ("Tag", "tag"),
        ("Angle", "angle"),
        ("Radius", "radius"),
        ("Diameter", "diameter"),
        ("Arc length", "arc-length"),
        ("Note", "message-square-text"),
        ("Symbol", "crosshair"),
        ("Spot level", "spot-level"),
        ("Detail component", "detail-component"),
        ("Sketch", "pen-tool"),
        ("Guide", "guide"),
        ("Guide lines", "guide-lines"),
        ("Level", "level"),
        ("Copy to levels", "copy-levels"),
        ("Grid", "grid-lines"),
        ("Switch join order", "switch-join"),
        ("Units", "units"),
        ("Material", "palette"),
        ("Graphic styles", "anno-styles"),
        ("Colour scheme", "colour-scheme"),
        ("View range", "view-range"),
        ("Underlay", "underlay"),
        ("Workspace", "workspace"),
        ("Maximise area", "maximise"),
        ("Frame selection (view header)", "frame-selection"),
        ("Edit outline", "edit-outline"),
        ("Duplicate", "duplicate"),
        ("Select similar", "select-similar"),
        ("Hide (H)", "eye-off"),
        ("Isolate (/)", "isolate"),
        ("Frame (.)", "frame-selection"),
        ("Open view", "open-view"),
        ("Flip", "flip"),
        ("Make a type", "make-type"),
        ("Attach top", "attach-top"),
        ("Select tagged", "select-tagged"),
        ("Straighten leader", "straighten-leader"),
        ("More", "ellipsis"),
        ("Delete", "trash-2"),
        ("FreeCAD file (F and gear)", "freecad"),
    ];

    /// icons.js as key -> markup: one `"key": "markup",` per line.
    fn icons_js() -> BTreeMap<String, String> {
        super::ICONS
            .lines()
            .filter(|l| l.starts_with('"'))
            .map(|l| {
                let l = l.trim_end().trim_end_matches(',');
                let v: serde_json::Value =
                    serde_json::from_str(&format!("{{{l}}}")).expect("an icons.js line");
                let (k, v) = v.as_object().unwrap().iter().next().unwrap();
                (k.clone(), v.as_str().unwrap().to_string())
            })
            .collect()
    }

    fn approved(json: &str) -> Vec<(String, String)> {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        let mut out = Vec::new();
        let mut take = |m: &serde_json::Map<String, serde_json::Value>| {
            for (k, v) in m {
                match v {
                    serde_json::Value::String(s) => out.push((k.clone(), s.clone())),
                    serde_json::Value::Object(_) => {}
                    _ => panic!("{k}: not markup"),
                }
            }
        };
        let top = v.as_object().unwrap();
        take(top);
        for g in top.values() {
            if let Some(m) = g.as_object() {
                take(m);
            }
        }
        out
    }

    #[test]
    fn icons_js_matches_the_approved_set_verbatim() {
        let js = icons_js();
        let keys: BTreeMap<_, _> = KEYS.iter().copied().collect();
        let mut drift = Vec::new();
        for json in [
            include_str!("../ui/approved/icons_final.json"),
            include_str!("../ui/approved/icons_actions.json"),
        ] {
            for (name, markup) in approved(json) {
                let Some(key) = keys.get(name.as_str()) else {
                    drift.push(format!(
                        "{name}: approved, but no icons.js key is named for it"
                    ));
                    continue;
                };
                match js.get(*key) {
                    None => drift.push(format!("{name}: icons.js lacks {key}")),
                    Some(m) if *m != markup => drift.push(format!("{name}: {key} differs")),
                    _ => {}
                }
            }
        }
        assert!(drift.is_empty(), "icons drifted:\n{}", drift.join("\n"));
    }

    /// The New project window's choices scroll between its title and its
    /// buttons, so a window too short for them keeps Create in reach
    /// (issue 24).
    #[test]
    fn the_new_project_windows_choices_scroll() {
        let rule = super::CSS
            .lines()
            .find(|l| l.starts_with(".np-body {"))
            .expect("a rule for the choices");
        assert!(rule.contains("overflow-y: auto") && rule.contains("min-height: 0"));
        // the page puts the choices, and only them, in that part
        let body = super::JS
            .find("class=\"np-body\"")
            .expect("the choices' part");
        // (REV05: the sources at its left, the picked start at its right,
        // all three closed before the buttons)
        let side = super::JS.find("class=\"np-side\"").unwrap();
        let prev = super::JS.find("class=\"np-prev\"").unwrap();
        let foot = super::JS.find("class=\"np-foot\"").unwrap();
        assert!(side < body && body < prev && prev < foot);
        let between = &super::JS[prev..foot];
        assert!(
            between.contains("</div></div>"),
            "the part closes before the buttons"
        );
    }
}

#[cfg(test)]
mod solid_panels {
    use super::Pane;
    use eframe::egui;

    /// `css` without its comments.
    fn uncommented(css: &str) -> String {
        let mut out = String::new();
        let mut rest = css;
        while let Some(a) = rest.find("/*") {
            out.push_str(&rest[..a]);
            rest = rest[a..].find("*/").map_or("", |b| &rest[a + b + 2..]);
        }
        out.push_str(rest);
        out
    }

    /// A selector naming the page itself (html or body), not something in it.
    fn names_the_page(sel: &str) -> bool {
        let mut depth = 0;
        let mut spaced = false;
        for ch in sel.chars() {
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                ' ' if depth == 0 => spaced = true,
                _ => {}
            }
        }
        !spaced || sel.ends_with(" body")
    }

    #[test]
    fn popup_and_tip_panels_are_solid_with_no_shadow_round_them() {
        // chopinregis/CreaBIM#112 (Windows) and #121 (Linux): see-through
        // popup and tip panels painted black where the system doesn't
        // blend them; the clear margin round a menu showed as a black band
        for pane in Pane::ALL {
            assert!(!super::see_through(pane), "{pane:?} is see-through");
        }
        let css = uncommented(super::CSS);
        let mut sized = false;
        for rule in css.split('}') {
            let Some((sel, body)) = rule.rsplit_once('{') else {
                continue;
            };
            let body: String = body.chars().filter(|c| !c.is_whitespace()).collect();
            for s in sel.split(',').map(str::trim) {
                let popup = s.contains("data-pane=\"popup\"");
                let ours = popup || s.contains("data-pane=\"tip\"");
                if ours && names_the_page(s) {
                    assert!(
                        !body.contains("background:transparent"),
                        "`{s}` leaves its panel see-through"
                    );
                }
                if ours && s.ends_with(" body") && names_the_page(s) {
                    sized |= body.contains("width:var(--shown-w")
                        && body.contains("height:var(--shown-h");
                }
                if popup && s.contains(".mwrap") {
                    assert!(
                        !body.contains("box-shadow:0"),
                        "`{s}` draws a shadow outside the menu, in its panel"
                    );
                    assert!(
                        !body.contains("calc(100%-17px)"),
                        "`{s}` leaves a margin round the menu in its panel"
                    );
                }
            }
        }
        // MU151: the page is as big as its window shows, as the app tells
        // it, so a frame round its edge keeps its right and bottom lines
        assert!(
            sized,
            "the popup and tip pages take the viewport's size, not the size their window shows"
        );
    }

    #[test]
    fn a_frame_round_the_page_stays_inside_its_window_at_every_scale() {
        // MU151: at 125 % the page laid itself out on a viewport rounded up
        // to a whole pixel, past what its window showed; a menu's frame
        // drawn round that lost its right and bottom lines
        let mut cut_before = 0;
        for ppp in [1.0f32, 1.25, 1.5] {
            for w10 in 1500..2600 {
                let x = 100.3 + (w10 % 7) as f32 * 0.37;
                let rect = egui::Rect::from_min_size(
                    egui::pos2(x, 40.6),
                    egui::vec2(w10 as f32 / 10.0, 300.0),
                );
                let (_, _, w, h) = super::device_bounds(rect, ppp);
                // the frame's far line, in device pixels from the window's corner
                let (cw, ch) = super::shown_size(w, h, ppp);
                assert!(
                    cw * ppp <= w as f32 + 1e-3 && ch * ppp <= h as f32 + 1e-3,
                    "{ppp} {w10}"
                );
                assert!(
                    cw * ppp > w as f32 - 1.0 && ch * ppp > h as f32 - 1.0,
                    "{ppp} {w10}"
                );
                // as before: the viewport rounded up to whole page pixels
                if (w as f32 / ppp).ceil() * ppp > w as f32 + 1e-3 {
                    cut_before += 1;
                }
            }
        }
        assert!(cut_before > 0, "the rounding the fix answers never happens");
    }
}
