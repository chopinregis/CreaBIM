//! The splash screen (Mockup_Splash_REV00, design A, owner 2026-11-16),
//! laid out as the owner's 1.2 picture (#1, the woven roof): the logo and
//! name top left on the picture, the version top right, the tagline at its
//! foot; below, New project and Recent projects side by side, then Manual,
//! Release notes and Community. The first time, the quick setup (where you
//! come from, the drawing standard, the theme) takes the lower part. A
//! click outside, × or Esc closes it. Each release brings its own picture:
//! `assets/splash/splash-<version>.jpg`, with no text in it.

use super::*;

/// The backgrounds people come from (first-run setup, decision 2): a
/// background sets keys and the mouse to match, through the keymap.
pub(crate) const BACKGROUNDS: [(&str, &str); 6] = [
    ("creabim", "CreaBIM"),
    ("revit", "Revit"),
    ("archicad", "ArchiCAD"),
    ("autocad", "AutoCAD"),
    ("sketchup", "SketchUp"),
    ("blender", "Blender"),
];

/// The drawing standards offered at first: US first (standards plan).
pub(crate) const STANDARDS: [(&str, &str, &str); 3] = [
    ("us", "US", "US customary: feet and inches, ANSI sheets"),
    ("metric", "Metric", "ISO: millimetres, A sheets"),
    ("uk", "UK", "BS 8888: millimetres, A sheets"),
];

const WIDTH: f32 = 724.0;

/// The card's width this frame: its full width, or the window's less a
/// margin each side on a small screen (MU84: a 724-point card was pushed
/// off the window's left edge).
fn card_w(ctx: &egui::Context) -> f32 {
    ctx.data(|d| d.get_temp(egui::Id::new("splash.w")))
        .unwrap_or(WIDTH)
}
/// The picture's height at most; less on a short screen.
const PICTURE_H: f32 = 355.0;
/// The picture's least height in a short window, with its lockup and
/// tagline drawn smaller to stay clear of each other (MU154).
const PICTURE_MIN: f32 = 96.0;
/// The height the lockup and tagline keep their full size down to.
const PICTURE_FULL: f32 = 200.0;
/// The room kept between the card and the bars above and below it.
const MARGIN: f32 = 12.0;
/// The card's border, which adds to its height top and bottom.
const BORDER: f32 = 1.0;
/// What the body under the picture measured the frame before.
const BODY_KEY: &str = "splash.body";
/// The body under the picture is the same height on both pages, the setup
/// and the start (MU106: Save and continue changed the card's height, a
/// jump to the eye). The pages' own content is at least this tall; the
/// footer stays put under it.
const CONTENT_H: f32 = 186.0;
/// The room the setup page adds above its last line to reach CONTENT_H,
/// worked out from what it measured the frame before.
const PAD_KEY: &str = "splash.pad";
const COMMUNITY: &str = "https://github.com/chopinregis/CreaBIM";

fn label_of(list: &[(&'static str, &'static str)], id: &str) -> &'static str {
    list.iter()
        .find(|(k, _)| *k == id)
        .map_or(list[0].1, |(_, l)| *l)
}

/// How far down the release's picture the bird on the roof and the roof's
/// crest end, as a share of its height: a short strip of it keeps them.
const BIRD_LINE: f32 = 0.42;

/// The release's picture.
fn picture(ctx: &egui::Context) -> egui::TextureHandle {
    static TEX: std::sync::OnceLock<egui::TextureHandle> = std::sync::OnceLock::new();
    TEX.get_or_init(|| {
        let img = image::load_from_memory(include_bytes!("../assets/splash/splash-1.2.jpg"))
            .map(|i| i.to_rgba8())
            .unwrap_or_default();
        let (w, h) = img.dimensions();
        ctx.load_texture(
            "creabim-splash",
            egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw()),
            egui::TextureOptions::LINEAR,
        )
    })
    .clone()
}

/// The brand's lockup in white: the woven mark with its bird, and the
/// name as the brand writes it.
fn lockup(ctx: &egui::Context) -> egui::TextureHandle {
    static TEX: std::sync::OnceLock<egui::TextureHandle> = std::sync::OnceLock::new();
    TEX.get_or_init(|| {
        let img = image::load_from_memory(include_bytes!(
            "../assets/logo/png/creabim-lockup-white-640x200.png"
        ))
        .map(|i| i.to_rgba8())
        .unwrap_or_default();
        let (w, h) = img.dimensions();
        ctx.load_texture(
            "creabim-lockup",
            egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw()),
            egui::TextureOptions::LINEAR,
        )
    })
    .clone()
}

/// The units a standard draws new projects in.
pub(crate) fn standard_length(id: &str) -> Option<creabim_core::units::LengthUnit> {
    use creabim_core::units::LengthUnit;
    match id {
        "us" => Some(LengthUnit::FeetInches),
        "metric" | "uk" => Some(LengthUnit::Millimetres),
        _ => None,
    }
}

/// A shade from `from` to `to` over `r`, top to bottom, for text to read
/// on any picture.
fn shade(p: &egui::Painter, r: egui::Rect, top: egui::Color32, bottom: egui::Color32) {
    let mut m = egui::Mesh::default();
    m.colored_vertex(r.left_top(), top);
    m.colored_vertex(r.right_top(), top);
    m.colored_vertex(r.right_bottom(), bottom);
    m.colored_vertex(r.left_bottom(), bottom);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(m);
}

/// A web band's rect kept clear of the splash card: web bands draw over
/// egui, so where one meets the card it would hide it (MU84: on a small
/// window the Project band covered the card's left side). The largest
/// part of the band beside the card, or none when too little is left.
pub(crate) fn clear_of_card(ctx: &egui::Context, r: egui::Rect) -> Option<egui::Rect> {
    if !SPLASH_UP.load(std::sync::atomic::Ordering::Relaxed) {
        return Some(r);
    }
    let Some(card) = ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new("splash.rect"))) else {
        return Some(r);
    };
    if !r.intersects(card) {
        return Some(r);
    }
    let parts = [
        egui::Rect::from_min_max(r.min, egui::pos2(card.min.x.min(r.max.x), r.max.y)),
        egui::Rect::from_min_max(egui::pos2(card.max.x.max(r.min.x), r.min.y), r.max),
        egui::Rect::from_min_max(r.min, egui::pos2(r.max.x, card.min.y.min(r.max.y))),
        egui::Rect::from_min_max(egui::pos2(r.min.x, card.max.y.max(r.min.y)), r.max),
    ];
    parts
        .into_iter()
        .filter(|p| p.width() >= 40.0 && p.height() >= 24.0)
        .max_by(|a, b| a.area().total_cmp(&b.area()))
}

static SPLASH_UP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The layer for things floating over the views (tool strips, grip menus,
/// badges): the top one, or, while the splash is up, the windows' layer, so
/// the splash's shade covers them too (MU82: a tool strip stood out bright
/// on the shade).
pub(crate) fn overlay_order() -> egui::Order {
    if SPLASH_UP.load(std::sync::atomic::Ordering::Relaxed) {
        egui::Order::Middle
    } else {
        egui::Order::Foreground
    }
}

impl CreaBimApp {
    pub(super) fn splash_window(&mut self, ctx: &egui::Context) {
        // the panels' fold handles step aside under the shade (MU79)
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("splash.open"), self.splash_open));
        SPLASH_UP.store(self.splash_open, std::sync::atomic::Ordering::Relaxed);
        if !self.splash_open {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.splash_open = false;
            return;
        }
        let screen = ctx.content_rect();
        let p = crate::theme::Palette::of(ctx);
        // the app dimmed behind it; a click there closes it and goes no
        // further
        // on the windows' layer, under popups: the lists' popups opened
        // behind it on the top layer (MU73)
        // both there from the first frame painted: an area is laid out
        // unseen on its first pass and then fades in, so at start the
        // project showed first and the splash came after it (MU103)
        {
            use std::sync::atomic::{AtomicBool, Ordering};
            static SEEN: AtomicBool = AtomicBool::new(false);
            if !SEEN.swap(true, Ordering::Relaxed) {
                ctx.request_discard("the splash's first pass");
            }
        }
        let dim = egui::Area::new(egui::Id::new("splash.dim"))
            .fade_in(false)
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (r, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter()
                    .rect_filled(r, 0.0, egui::Color32::from_black_alpha(150));
                resp.clicked()
            });
        let first = !self.shell.settings.setup_done;
        // the card fits between the bars (`screen` is the window less the
        // top bar and the status bar). The body is as tall as it measured
        // the frame before; the picture takes what is left, down to its
        // least height, and past that the body scrolls. MU154: at 150 % the
        // card was laid out 470 points tall in 420. egui cut it at the
        // status bar, and the bar, cut round the card, left black where it
        // had been
        let room = (screen.height() - 2.0 * (MARGIN + BORDER)).max(0.0);
        let body_h: f32 = ctx
            .data(|d| d.get_temp(egui::Id::new(BODY_KEY)))
            .unwrap_or(270.0);
        let pic_h = (room - body_h).clamp(PICTURE_MIN, PICTURE_H);
        let body_room = (room - pic_h).max(60.0);
        let card_h = pic_h + body_h.min(body_room) + 2.0 * BORDER;
        let mut close = dim.inner;
        let mut act: Option<&'static str> = None;
        let mut open_file: Option<String> = None;
        let w = WIDTH.min(screen.width() - 24.0).max(320.0);
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("splash.w"), w));
        let at = screen.center() - egui::vec2(w / 2.0, card_h / 2.0);
        let area = egui::Area::new(egui::Id::new("splash"))
            .fade_in(false)
            .order(egui::Order::Middle)
            .fixed_pos(at.max(screen.min + egui::vec2(MARGIN, MARGIN)))
            .constrain(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(p.panel)
                    .stroke(egui::Stroke::new(BORDER, p.border))
                    .corner_radius(10)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 16],
                        blur: 48,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(120),
                    })
                    .show(ui, |ui| {
                        ui.set_width(w);
                        // its room said outright: an area's room is the size
                        // it had last frame (400 points the first), and the
                        // body's scroll area never grows past the room it's
                        // given, so the card stayed 400 points tall
                        ui.set_max_height(pic_h + body_room);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if self.splash_picture(ui, pic_h) {
                            close = true;
                        }
                        // a bar over the body's edge when it scrolls, so the
                        // columns keep their width
                        ui.spacing_mut().scroll = egui::style::ScrollStyle::floating();
                        let body = egui::ScrollArea::vertical()
                            .id_salt("splash.body")
                            .max_height(body_room)
                            .auto_shrink([false, true])
                            .show(ui, |ui| self.splash_body(ui, &p, first));
                        (act, open_file) = body.inner;
                        // the body's own height, for the picture's next frame
                        let natural = body.content_size.y;
                        if (natural - body_h).abs() > 0.5 {
                            ui.ctx()
                                .data_mut(|d| d.insert_temp(egui::Id::new(BODY_KEY), natural));
                            ui.ctx().request_discard("the splash's body height");
                        }
                    });
            });
        ctx.move_to_top(dim.response.layer_id);
        ctx.move_to_top(area.response.layer_id);
        // where the web bands must stay clear of, with its shadow's edge
        ctx.data_mut(|d| {
            d.insert_temp(egui::Id::new("splash.rect"), area.response.rect.expand(6.0))
        });
        match act {
            Some("keep") => {
                self.shell.settings.background = "creabim".into();
                self.finish_setup();
            }
            Some("save") => self.finish_setup(),
            Some("change") => {
                self.shell.settings.setup_done = false;
                self.shell.settings.store();
            }
            Some("open") => {
                self.splash_open = false;
                self.open_dialog();
            }
            Some("manual") => {
                self.splash_open = false;
                self.open_help("welcome");
            }
            Some("releases") => ctx.open_url(egui::OpenUrl::new_tab(crate::app::appmenu::RELEASES)),
            Some("community") => ctx.open_url(egui::OpenUrl::new_tab(COMMUNITY)),
            Some(id) => {
                self.splash_open = false;
                self.run_action(ctx, id);
            }
            None => {}
        }
        if let Some(f) = open_file {
            self.splash_open = false;
            self.open_path(&f);
        }
        if close {
            self.splash_open = false;
        }
    }

    /// Under the picture: the setup the first time, else the start page,
    /// then the footer. What was asked for, and a recent file to open.
    fn splash_body(
        &mut self,
        ui: &mut egui::Ui,
        p: &crate::theme::Palette,
        first: bool,
    ) -> (Option<&'static str>, Option<String>) {
        let mut act = None;
        let mut open_file = None;
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 28,
                right: 28,
                top: 18,
                bottom: 14,
            })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                // one height for both pages: the taller of the two, so the
                // card never jumps (MU106)
                let top = ui.cursor().top();
                if first {
                    act = self.splash_setup(ui, p);
                } else {
                    (act, open_file) = self.splash_start(ui, p);
                }
                let natural = ui.min_rect().bottom() - top;
                if first {
                    // the setup's room goes above its last line, where the
                    // start page has its own
                    let pad: f32 = ui
                        .ctx()
                        .data(|d| d.get_temp(egui::Id::new(PAD_KEY)))
                        .unwrap_or(0.0);
                    let want = (CONTENT_H - (natural - pad)).max(0.0);
                    if (want - pad).abs() > 0.25 {
                        ui.ctx()
                            .data_mut(|d| d.insert_temp(egui::Id::new(PAD_KEY), want));
                        ui.ctx().request_discard("the splash's setup height");
                    }
                } else if natural < CONTENT_H {
                    ui.add_space(CONTENT_H - natural);
                }
                ui.add_space(8.0);
                if let Some(a) = self.splash_footer(ui, p) {
                    act = Some(a);
                }
            });
        (act, open_file)
    }

    /// The picture with the lockup, version and tagline on it, and ×.
    /// True when × was clicked.
    fn splash_picture(&mut self, ui: &mut egui::Ui, h: f32) -> bool {
        let width = card_w(ui.ctx());
        let (r, _) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
        let tex = picture(ui.ctx());
        // fill the width; when shorter, keep the top (MU73: cropped from
        // the middle, the bird on the roof was cut away). A strip too short
        // to reach the bird from the top slides down just enough to keep it
        // and the roof's crest (MU154: at 150 % the top showed sky)
        let [tw, th] = tex.size();
        let want = (width / h) / (tw as f32 / th.max(1) as f32);
        let uv = if want > 1.0 {
            let shown = 1.0 / want;
            let top = (BIRD_LINE - shown).clamp(0.0, 1.0 - shown);
            egui::Rect::from_min_max(egui::pos2(0.0, top), egui::pos2(1.0, top + shown))
        } else {
            let u = (1.0 - want) / 2.0;
            egui::Rect::from_min_max(egui::pos2(u, 0.0), egui::pos2(1.0 - u, 1.0))
        };
        egui::Image::new((tex.id(), r.size()))
            .uv(uv)
            .corner_radius(egui::CornerRadius {
                nw: 10,
                ne: 10,
                sw: 0,
                se: 0,
            })
            .paint_at(ui, r);
        let pa = ui.painter_at(r);
        let clear = egui::Color32::TRANSPARENT;
        // in a short window the lockup, the tagline and their shades come
        // down with the picture, so the two stay clear of each other (MU154)
        let k = (h / PICTURE_FULL).clamp(PICTURE_MIN / PICTURE_FULL, 1.0);
        shade(
            &pa,
            egui::Rect::from_min_max(r.min, egui::pos2(r.right(), r.top() + 120.0 * k)),
            egui::Color32::from_black_alpha(120),
            clear,
        );
        shade(
            &pa,
            egui::Rect::from_min_max(egui::pos2(r.left(), r.bottom() - 110.0 * k), r.max),
            clear,
            egui::Color32::from_black_alpha(150),
        );
        let white = egui::Color32::WHITE;
        // the brand's own lockup, the bird and "creabim" (MU73: the name in
        // the interface font read wrong)
        let lockup = lockup(ui.ctx());
        let [lw, lh] = lockup.size();
        let lh_px = 70.0 * k;
        let lr = egui::Rect::from_min_size(
            egui::pos2(r.left() + 20.0, r.top() + 16.0 * k),
            egui::vec2(lh_px * lw as f32 / lh.max(1) as f32, lh_px),
        );
        egui::Image::new((lockup.id(), lr.size())).paint_at(ui, lr);
        // ×, a dark square (as the first splash, MU73)
        let xr = egui::Rect::from_min_size(
            egui::pos2(r.right() - 38.0, r.top() + 12.0),
            egui::vec2(26.0, 26.0),
        );
        let x = ui.interact(xr, ui.id().with("splash.x"), egui::Sense::click());
        pa.rect_filled(
            xr,
            6.0,
            egui::Color32::from_black_alpha(if x.hovered() { 170 } else { 110 }),
        );
        pa.text(
            xr.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            egui::FontId::proportional(16.0),
            white,
        );
        // the version as a pill under it (as the first splash)
        let g = pa.layout_no_wrap(
            format!("v{}", env!("CARGO_PKG_VERSION")),
            egui::FontId::monospace(12.0),
            white,
        );
        let vr = egui::Rect::from_min_size(
            egui::pos2(xr.right() - g.size().x - 14.0, xr.bottom() + 8.0),
            g.size() + egui::vec2(14.0, 6.0),
        );
        pa.rect_filled(vr, 4.0, egui::Color32::from_black_alpha(110));
        pa.galley(vr.min + egui::vec2(7.0, 3.0), g, white);
        // the tagline
        pa.text(
            egui::pos2(r.left() + 28.0, r.bottom() - 24.0 * k),
            egui::Align2::LEFT_BOTTOM,
            "One model. Every drawing.",
            egui::FontId::proportional(24.0 * k),
            white,
        );
        x.on_hover_text("Close (Esc)").clicked()
    }

    fn finish_setup(&mut self) {
        let s = &mut self.shell.settings;
        s.setup_done = true;
        // the background's mouse (its keys come from the keymap)
        s.navigation = crate::keymap::mouse_of(&s.background).to_string();
        s.store();
        let bg = label_of(&BACKGROUNDS, &s.background);
        let std = STANDARDS
            .iter()
            .find(|(k, ..)| *k == s.standard)
            .map_or("", |(_, l, _)| *l);
        crate::actlog::line(format!(
            "setup: background {bg}, standard {std}, theme {}",
            self.shell.theme.label()
        ));
        self.status = format!("Set up: {bg} background, {std} standard. Change it from the splash (Help › Splash screen) or Settings.");
    }

    /// A heading as the picture's: larger, strong.
    fn splash_head(ui: &mut egui::Ui, p: &crate::theme::Palette, t: &str) {
        ui.label(egui::RichText::new(t).size(18.0).strong().color(p.text));
        ui.add_space(4.0);
    }

    /// The quick setup, first time only. Returns "keep" or "save" when done.
    fn splash_setup(
        &mut self,
        ui: &mut egui::Ui,
        p: &crate::theme::Palette,
    ) -> Option<&'static str> {
        let mut out = None;
        Self::splash_head(ui, p, "Set it up your way");
        let mut bg = self.shell.settings.background.clone();
        crate::props::row(ui, "I come from", |ui| {
            // to the row's edge, as Settings' lists (MU73: the label went
            // and the list stopped short of the choice under it)
            egui::ComboBox::from_id_salt("splash.background")
                .icon(crate::props::combo_icon)
                .width(ui.available_width())
                .selected_text(label_of(&BACKGROUNDS, &bg))
                .show_ui(ui, |ui| {
                    for (k, l) in BACKGROUNDS {
                        ui.selectable_value(&mut bg, k.to_string(), l);
                    }
                })
                .response
                .on_hover_text("The program your hands know: CreaBIM sets its keys and the mouse to match, and you can change any key later.");
        });
        if bg != self.shell.settings.background {
            self.shell.settings.background = bg;
            self.shell.settings.store();
        }
        let mut std = self.shell.settings.standard.clone();
        if std.is_empty() {
            std = "us".into();
        }
        let opts: Vec<(String, &str, &str)> = STANDARDS
            .iter()
            .map(|(k, l, t)| (k.to_string(), *l, *t))
            .collect();
        let opts_ref: Vec<(&String, &str, &str)> =
            opts.iter().map(|(k, l, t)| (k, *l, *t)).collect();
        let mut pick: &String = opts_ref
            .iter()
            .find(|o| *o.0 == std)
            .map_or(opts_ref[0].0, |o| o.0);
        crate::props::choice(ui, "Standard", &mut pick, &opts_ref);
        if *pick != self.shell.settings.standard {
            self.shell.settings.standard = pick.clone();
            self.shell.settings.store();
        }
        let mut theme = self.shell.theme;
        crate::props::row(ui, "Theme", |ui| {
            egui::ComboBox::from_id_salt("splash.theme")
                .icon(crate::props::combo_icon)
                .width(ui.available_width())
                .selected_text(theme.label())
                .show_ui(ui, |ui| {
                    for t in crate::theme::Theme::ALL {
                        ui.selectable_value(&mut theme, t, t.label());
                    }
                });
        });
        if theme != self.shell.theme {
            self.set_theme(ui.ctx(), theme);
        }
        // as tall as the start page that follows (MU106)
        let pad: f32 = ui
            .ctx()
            .data(|d| d.get_temp(egui::Id::new(PAD_KEY)))
            .unwrap_or(0.0);
        ui.add_space(8.0 + pad);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("You can change these later in Settings.")
                    .size(12.0)
                    .color(p.muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let go = egui::Button::new(
                    egui::RichText::new("Save and continue")
                        .color(crate::theme::ink_on(crate::theme::interaction()))
                        .strong(),
                )
                .fill(crate::theme::interaction())
                .min_size(egui::vec2(0.0, 28.0));
                if ui.add(go).clicked() {
                    out = Some("save");
                }
                if ui
                    .add(egui::Button::new("Keep CreaBIM's").min_size(egui::vec2(0.0, 28.0)))
                    .on_hover_text("CreaBIM's own keys and mouse; the standard and theme as picked")
                    .clicked()
                {
                    out = Some("keep");
                }
            });
        });
        out
    }

    /// A row of the start lists: icon, name, an arrow at the end, a rule
    /// under it, as the picture's.
    fn splash_row(
        ui: &mut egui::Ui,
        p: &crate::theme::Palette,
        w: f32,
        icon: &str,
        text: &str,
        last: bool,
    ) -> egui::Response {
        let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 38.0), egui::Sense::click());
        if resp.hovered() {
            ui.painter()
                .rect_filled(r.shrink2(egui::vec2(0.0, 2.0)), 6.0, p.raise);
        }
        crate::svgicon::paint(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(r.left() + 16.0, r.center().y),
                egui::Vec2::splat(18.0),
            ),
            icon,
            p.muted,
        );
        let g = ui.painter().layout(
            text.to_string(),
            egui::FontId::proportional(14.5),
            p.text,
            w - 90.0,
        );
        ui.painter().galley(
            egui::pos2(r.left() + 46.0, r.center().y - g.size().y / 2.0),
            g,
            p.text,
        );
        crate::svgicon::paint(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(r.right() - 14.0, r.center().y),
                egui::Vec2::splat(14.0),
            ),
            "chevron-right",
            if resp.hovered() { p.text } else { p.muted },
        );
        if !last {
            ui.painter().line_segment(
                [r.left_bottom(), r.right_bottom()],
                egui::Stroke::new(1.0, p.border),
            );
        }
        resp
    }

    /// New project and Recent projects side by side; the setup in a line.
    fn splash_start(
        &mut self,
        ui: &mut egui::Ui,
        p: &crate::theme::Palette,
    ) -> (Option<&'static str>, Option<String>) {
        let mut act = None;
        let mut file = None;
        let gap = 36.0;
        let col = ((card_w(ui.ctx()) - 56.0 - gap) / 2.0).floor();
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.vertical(|ui| {
                ui.set_width(col);
                ui.spacing_mut().item_spacing.y = 0.0;
                Self::splash_head(ui, p, "New project");
                let rows: [(&str, &str, &'static str); 3] = [
                    ("file-plus", "Blank project", "new.empty"),
                    ("folder-open", "Open project…", "open"),
                    ("house", "Starter house", "new.sample"),
                ];
                for (k, (icon, text, id)) in rows.iter().enumerate() {
                    if Self::splash_row(ui, p, col, icon, text, k + 1 == rows.len()).clicked() {
                        act = Some(*id);
                    }
                }
            });
            // the rule between the columns
            let (r, _) = ui.allocate_exact_size(egui::vec2(gap, 150.0), egui::Sense::hover());
            ui.painter().line_segment(
                [
                    egui::pos2(r.center().x, r.top() + 4.0),
                    egui::pos2(r.center().x, r.bottom()),
                ],
                egui::Stroke::new(1.0, p.border),
            );
            ui.vertical(|ui| {
                ui.set_width(col);
                ui.spacing_mut().item_spacing.y = 0.0;
                Self::splash_head(ui, p, "Recent projects");
                if self.recent.files.is_empty() {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Nothing opened yet. The projects you open show here.")
                            .size(12.5)
                            .color(p.muted),
                    );
                }
                let shown: Vec<String> = self.recent.files.iter().take(3).cloned().collect();
                for (k, f) in shown.iter().enumerate() {
                    let name = std::path::Path::new(f)
                        .file_stem()
                        .map_or(f.clone(), |n| n.to_string_lossy().into_owned());
                    if Self::splash_row(ui, p, col, "layers", &name, k + 1 == shown.len())
                        .on_hover_text(f.as_str())
                        .clicked()
                    {
                        file = Some(f.clone());
                    }
                }
            });
        });
        // how it is set up, and the way back to the setup
        let s = &self.shell.settings;
        let std = STANDARDS
            .iter()
            .find(|(k, ..)| *k == s.standard)
            .map_or("no standard", |(_, l, _)| *l);
        let working = format!(
            "Working as: {} · {} · {}",
            label_of(&BACKGROUNDS, &s.background),
            std,
            self.shell.theme.label()
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(working).size(12.0).color(p.muted));
            if ui.link(egui::RichText::new("Change").size(12.0)).clicked() {
                act = Some("change");
            }
        });
        (act, file)
    }

    /// Manual, Release notes, Community; the lab's name at the right.
    fn splash_footer(
        &mut self,
        ui: &mut egui::Ui,
        p: &crate::theme::Palette,
    ) -> Option<&'static str> {
        let mut act = None;
        let (r, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter().line_segment(
            [r.left_center(), r.right_center()],
            egui::Stroke::new(1.0, p.border),
        );
        ui.add_space(6.0);
        // narrow: the links share the row, the lab's name steps out
        let narrow = ui.available_width() < 600.0;
        let link_w = if narrow {
            ((ui.available_width() - 24.0) / 3.0).min(120.0)
        } else {
            120.0
        };
        ui.horizontal(|ui| {
            for (k, (icon, text, id, tip)) in [
                (
                    "book-open",
                    "Manual",
                    "manual",
                    "The help pages, from Getting started",
                ),
                (
                    "file-text",
                    "Release notes",
                    "releases",
                    "What changed, release by release (opens the web page)",
                ),
                (
                    "message-square-text",
                    "Community",
                    "community",
                    "Questions, ideas and the source (opens the web page)",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                if k > 0 {
                    ui.label(egui::RichText::new("|").color(p.border));
                }
                let (lr, resp) =
                    ui.allocate_exact_size(egui::vec2(link_w, 24.0), egui::Sense::click());
                let ink = if resp.hovered() { p.text } else { p.muted };
                crate::svgicon::paint(
                    ui.painter(),
                    egui::Rect::from_center_size(
                        egui::pos2(lr.left() + 9.0, lr.center().y),
                        egui::Vec2::splat(15.0),
                    ),
                    icon,
                    ink,
                );
                ui.painter().text(
                    egui::pos2(lr.left() + 24.0, lr.center().y),
                    egui::Align2::LEFT_CENTER,
                    text,
                    egui::FontId::proportional(13.0),
                    ink,
                );
                if resp.on_hover_text(tip).clicked() {
                    act = Some(id);
                }
            }
            if !narrow {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("Open Source Architecture Lab")
                            .size(12.5)
                            .color(p.muted),
                    );
                    ui.label(
                        egui::RichText::new("OSAL ·")
                            .size(12.5)
                            .strong()
                            .color(p.text),
                    );
                });
            }
        });
        act
    }
}

#[cfg(test)]
mod fit_tests {
    use super::*;

    /// The start window between the bars at every interface scale (MU154).
    /// On the owner's window, 1938 × 1038 pixels at 125 %, the card at
    /// 150 % was laid out 470 points tall in 420: egui cut it at the status
    /// bar, and the bar, cut round the card, left black where it had been.
    /// Also on a 1280 × 720 laptop at 100 %, where at 125 % and up the body
    /// has to scroll. The start page and the first-run setup both.
    #[test]
    fn the_start_window_fits_between_the_bars_at_every_scale() {
        let windows = [
            ("owner's", 1938.0, 1038.0, 1.25),
            ("laptop", 1280.0, 720.0, 1.0),
        ];
        for (name, px_w, px_h, system) in windows {
            for scale in [80u16, 90, 100, 110, 125, 150] {
                for first in [false, true] {
                    let page = if first { "setup" } else { "start" };
                    let mut app = crate::app::test_app();
                    app.splash_open = true;
                    app.shell.settings.setup_done = !first;
                    let ctx = egui::Context::default();
                    crate::app::start_zoom(&ctx, scale);
                    let ppp = system * f32::from(scale) / 100.0;
                    let window = egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(px_w / ppp, px_h / ppp),
                    );
                    let raw = egui::RawInput {
                        screen_rect: Some(window),
                        safe_area_insets: Some(egui::SafeAreaInsets(crate::app::band_insets(true))),
                        ..Default::default()
                    };
                    // a few frames: the body's height is measured, then used
                    for _ in 0..4 {
                        let mut out = ctx.run_ui(raw.clone(), |ui| app.splash_window(ui.ctx()));
                        out.textures_delta.clear();
                    }
                    let at = format!("{name} window at {scale} %, {page} page");
                    let room = ctx.content_rect();
                    let card = ctx
                        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("splash.rect")))
                        .expect("the card's rect")
                        .shrink(6.0);
                    assert!(
                        room.expand(0.5).contains_rect(card),
                        "{at}: the card {card:?} runs past the bars, which leave {room:?}"
                    );
                    // the status bar and the top bar are never cut round it
                    let status = egui::Rect::from_min_max(
                        egui::pos2(window.left(), room.bottom()),
                        window.max,
                    );
                    let top = egui::Rect::from_min_max(
                        window.min,
                        egui::pos2(window.right(), room.top()),
                    );
                    assert_eq!(
                        clear_of_card(&ctx, status),
                        Some(status),
                        "{at}: status bar"
                    );
                    assert_eq!(clear_of_card(&ctx, top), Some(top), "{at}: top bar");
                    // where there is room for the whole body under the
                    // least picture, the card is the picture and the whole
                    // body; where there isn't, it takes all the room there is
                    // and the body scrolls
                    let body: f32 = ctx
                        .data(|d| d.get_temp(egui::Id::new(BODY_KEY)))
                        .expect("the body's height");
                    let room_h = room.height() - 2.0 * (MARGIN + BORDER);
                    let (inside, how) = if room_h >= body + PICTURE_MIN {
                        (
                            (room_h - body).clamp(PICTURE_MIN, PICTURE_H) + body,
                            "its picture and whole body",
                        )
                    } else {
                        (room_h.max(PICTURE_MIN + 60.0), "all the room")
                    };
                    let want = inside + 2.0 * BORDER;
                    assert!(
                        (card.height() - want).abs() < 1.0,
                        "{at}: the card is {} tall, {how} {want} (body {body})",
                        card.height()
                    );
                }
            }
        }
    }
}
