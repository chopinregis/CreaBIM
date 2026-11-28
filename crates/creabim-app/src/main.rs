//! CreaBIM desktop app.

// A released app opens its window alone, with no terminal beside it
// (users took the console for a bug). Debug builds keep the console for
// what they print while being worked on.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actlog;
mod align3d;
mod aligntool;
mod annotate;
mod app;
mod areas;
mod boxsel;
mod browser;
mod builds;
mod camera;
mod centres;
mod choices;
mod commands;
mod detailtool;
mod editmode;
mod families;
mod files;
mod fillpreview;
mod ghost;
mod handles;
mod help;
mod hoverpick;
mod icons;
mod investigationui;
mod keymap;
mod kindpick;
mod library;
#[cfg(feature = "live-mcp")]
mod live_mcp;
#[cfg(target_os = "macos")]
mod mac_drag;
mod matcard;
mod memberstool;
mod modal;
mod modguides;
mod multiedit;
mod objecttool;
mod optionbar;
mod outline;
mod paletteshapes;
mod paneface;
mod partstool;
mod phaselens;
mod planview;
mod projecttime;
mod props;
mod render3d;
mod rendered;
mod roomtool;
mod schedulepane;
mod selection;
mod servicesui;
mod sheetview;
mod sitelevel;
mod sketch;
mod slabtool;
mod snaps;
mod stretchtool;
mod svgicon;
mod tempdims;
mod textures;
mod theme;
mod timecard;
mod tooltabs;
mod touchpad;
mod trace;
mod tracker;
mod ui_model;
mod units;
mod viewcube;
mod webui;

/// The window's and the taskbar's icon: the app tile, sage.
fn app_icon() -> std::sync::Arc<eframe::egui::IconData> {
    let png = include_bytes!("../assets/logo/png/creabim-app-icon-256.png");
    std::sync::Arc::new(eframe::icon_data::from_png_bytes(png).unwrap_or_default())
}

fn main() -> eframe::Result {
    // the activity log first, before the window and the graphics: a start
    // that fails there still leaves the log, and a crash goes in it (#31:
    // 1.2.0 closed at once and left no CreaBIM folder at all)
    actlog::start();
    // the window's thread answers first on a busy machine
    creabim_geom::prio::ui();
    // a road following the ground takes its levels from it after each change
    let _ = creabim_core::document::SITE_SETTLE.set(creabim_geom::earth::settle_roads);
    // the house the program opens on is built while the window is made
    app::appmenu::start_opening_house();
    // a crash says why in the activity log, with where it happened (MU39:
    // one closed the app with nothing to go on)
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let trace = std::backtrace::Backtrace::force_capture();
        actlog::line(format!(
            "CRASH: {info}
{trace}"
        ));
        before(info);
    }));
    #[cfg(target_os = "linux")]
    linux_setup();
    let viewport = eframe::egui::ViewportBuilder::default()
        .with_title("CreaBIM")
        // The size the window returns to after leaving its initial maximized state.
        .with_inner_size([1200.0, 760.0])
        .with_icon(app_icon());
    #[cfg(target_os = "macos")]
    let viewport = viewport
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false);
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        #[cfg(target_os = "linux")]
        event_loop_builder: Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        })),
        ..Default::default()
    };
    let started = eframe::run_native(
        "CreaBIM",
        options,
        Box::new(|cc| Ok(Box::new(app::CreaBimApp::new(cc)))),
    );
    if let Err(e) = &started {
        start_failed(&e.to_string());
    }
    started
}

/// A start that failed says why: in the activity log, and in a message
/// box, since a released app has no console to print it to (#31). The
/// box is left out when `CREABIM_NO_DIALOG` is set, for tests.
fn start_failed(error: &str) {
    actlog::line(format!("START FAILED: {error}"));
    actlog::line(format!("system: {}", app::reportui::os_version()));
    if std::env::var_os("CREABIM_NO_DIALOG").is_none() {
        let _ = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("CreaBIM couldn't start")
            .set_description(start_failed_text(error, actlog::path().as_deref()))
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
}

/// What the message box says: what failed in plain words, the error
/// itself, and the log to attach to a report.
fn start_failed_text(error: &str, log: Option<&std::path::Path>) -> String {
    let lower = error.to_lowercase();
    let graphics = ["wgpu", "adapter", "graphics", "surface", "device"]
        .iter()
        .any(|w| lower.contains(w));
    let what = if graphics {
        "Its graphics couldn't start. CreaBIM draws through your graphics card with DirectX 12, Vulkan or OpenGL, and none of them answered. Updating the graphics card's driver often fixes this."
    } else {
        "Its window couldn't be opened."
    };
    let mut text = format!("{what}\n\nThe error was: {error}");
    if let Some(log) = log {
        text += &format!(
            "\n\nIt is also written in the activity log:\n{}",
            log.display()
        );
    }
    text
        + "\n\nTo report it, attach that file to an issue at github.com/chopinregis/CreaBIM/issues."
}

/// The web panes are GTK child windows of an X11 window, so on Wayland the
/// app runs through XWayland, and GTK has to be started first.
#[cfg(target_os = "linux")]
fn linux_setup() {
    // Without this WebKit draws nothing on some GPUs and in virtual displays.
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    std::env::set_var("GDK_BACKEND", "x11");
    if gtk::init().is_err() {
        // The app still runs, with the classic panels.
        std::env::set_var("CREABIM_CLASSIC_UI", "1");
    }
}

#[cfg(test)]
mod start_tests {
    use super::start_failed_text;
    use std::path::Path;

    /// #31: no graphics adapter says so in plain words, with the error
    /// and where the log is.
    #[test]
    fn a_graphics_failure_says_so_with_the_error_and_the_log() {
        let error = "wgpu error: Failed to create wgpu adapter, no suitable adapter found.";
        let log = Path::new(r"C:\Users\someone\AppData\Local\CreaBIM\activity.log");
        let text = start_failed_text(error, Some(log));
        assert!(text.starts_with("Its graphics couldn't start."), "{text}");
        assert!(text.contains(error), "{text}");
        assert!(
            text.contains(r"AppData\Local\CreaBIM\activity.log"),
            "{text}"
        );
        assert!(
            text.contains("github.com/chopinregis/CreaBIM/issues"),
            "{text}"
        );
    }

    /// Anything else is the window, and a missing log path is left out.
    #[test]
    fn another_failure_is_the_window_and_needs_no_log() {
        let text = start_failed_text("Os error: the window class could not be registered", None);
        assert!(text.starts_with("Its window couldn't be opened."), "{text}");
        assert!(!text.contains("activity log"), "{text}");
    }
}
