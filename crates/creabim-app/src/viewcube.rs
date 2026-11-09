//! The view cube in the corner of the 3D view: 26 hot zones (six faces,
//! twelve edges, eight corners), each turning the view to look from
//! there; a compass ring under it; and a column of round buttons beside
//! it: home (frame everything), a quarter turn each way, and perspective
//! or orthographic.
//!
//! The faces are named from the project's north (+Y): FRONT is the south
//! face, so it shows the building's front as its south elevation does.

use crate::camera::OrbitCamera;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use glam::Vec3;
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

/// How long a turn to a picked view takes, in seconds.
const TURN: f64 = 0.35;

/// Where the band of an edge or corner zone starts on a face, from its
/// centre (the face runs from -1 to 1).
const BAND: f32 = 0.55;

/// A turn of the camera in progress.
#[derive(Clone, Copy, Debug)]
struct Turn {
    from: (f32, f32),
    to: (f32, f32),
    start: f64,
}

#[derive(Default)]
pub struct ViewCube {
    turn: Option<Turn>,
    /// What the drag in progress started on: the cube (orbit), or the zoom
    /// or pan button.
    grab: Option<Nav>,
    /// Where the cube and its buttons were last drawn.
    area: Option<Rect>,
    /// The parts that take the pointer: the cube's disc and the buttons.
    hot: Option<(Pos2, f32, Rect)>,
}

/// A drag on the cube or its buttons, by this frame's pointer movement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Nav {
    Orbit(Vec2),
    Pan(Vec2),
    Zoom(Vec2),
}

/// What a click on the cube asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    /// Look from this direction (each part -1, 0 or 1).
    From([i8; 3]),
    /// Turn a quarter round the vertical: +1 to the left, -1 to the right.
    Quarter(i8),
    /// Frame everything, from the three-quarter view.
    Home,
    /// Switch between perspective and orthographic.
    Projection,
    /// The zoom button: dragged, not clicked.
    Zoom,
    /// The pan button: dragged, not clicked.
    Pan,
    /// Frame everything, from where it is looked at (MU24: from the header).
    FitAll,
    /// Frame the selection.
    FitSel,
}

/// The yaw and pitch that look from `d` toward the target. Straight down
/// keeps north up, as plans do, and straight up shows north up as well.
pub fn angles_from(d: [i8; 3]) -> (f32, f32) {
    let v = Vec3::new(d[0] as f32, d[1] as f32, d[2] as f32).normalize();
    let pitch = v.z.asin();
    let yaw = match (d[0], d[1], d[2]) {
        (0, 0, 1) => -FRAC_PI_2,
        (0, 0, _) => FRAC_PI_2,
        _ => v.y.atan2(v.x),
    };
    (yaw, pitch)
}

/// The name of a zone, for the status bar: "front", "top, front, right".
pub fn zone_name(d: [i8; 3]) -> String {
    let mut parts = vec![];
    match d[2] {
        1 => parts.push("top"),
        -1 => parts.push("bottom"),
        _ => {}
    }
    match d[1] {
        -1 => parts.push("front"),
        1 => parts.push("back"),
        _ => {}
    }
    match d[0] {
        1 => parts.push("right"),
        -1 => parts.push("left"),
        _ => {}
    }
    parts.join(", ")
}

/// The compass point a direction faces from: "south", "north-east".
fn compass(d: [i8; 3]) -> Option<&'static str> {
    Some(match (d[0], d[1]) {
        (0, -1) => "south",
        (0, 1) => "north",
        (1, 0) => "east",
        (-1, 0) => "west",
        (1, -1) => "south-east",
        (-1, -1) => "south-west",
        (1, 1) => "north-east",
        (-1, 1) => "north-west",
        _ => return None,
    })
}

/// The camera's screen axes: right, up and toward the eye.
fn axes(cam: &OrbitCamera) -> (Vec3, Vec3, Vec3) {
    let fwd = (cam.target - cam.eye()).normalize_or_zero();
    let right = fwd.cross(cam.up()).normalize_or_zero();
    let up = right.cross(fwd);
    (right, up, -fwd)
}

/// A face: its normal, its two axes across, and its name.
type Face = ([i8; 3], [i8; 3], [i8; 3], &'static str);

const FACES: [Face; 6] = [
    ([0, 0, 1], [1, 0, 0], [0, 1, 0], "TOP"),
    ([0, 0, -1], [1, 0, 0], [0, -1, 0], "BOTTOM"),
    ([0, -1, 0], [1, 0, 0], [0, 0, 1], "FRONT"),
    ([0, 1, 0], [-1, 0, 0], [0, 0, 1], "BACK"),
    ([1, 0, 0], [0, 1, 0], [0, 0, 1], "RIGHT"),
    ([-1, 0, 0], [0, -1, 0], [0, 0, 1], "LEFT"),
];

/// A letter of the cube's labels: its width and its strokes, on a box one
/// unit high, y up. Drawn on the face itself, the labels foreshorten and
/// turn with it, as if printed there.
fn glyph(ch: char) -> (f32, Vec<Vec<(f32, f32)>>) {
    let arc = |from: f32, to: f32, cx: f32, rx: f32| -> Vec<(f32, f32)> {
        (0..=16)
            .map(|i| {
                let t = (from + (to - from) * i as f32 / 16.0).to_radians();
                (cx + t.cos() * rx, 0.5 + t.sin() * 0.5)
            })
            .collect()
    };
    let bowl = |y0: f32| -> Vec<(f32, f32)> {
        // the round part of P, R and B: from the stem out and back
        let h = 1.0 - y0;
        vec![
            (0.0, 1.0),
            (0.42, 1.0),
            (0.6, 1.0 - h * 0.12),
            (0.66, 1.0 - h * 0.5),
            (0.6, y0 + h * 0.12),
            (0.42, y0),
            (0.0, y0),
        ]
    };
    match ch {
        'T' => (
            0.7,
            vec![vec![(0.0, 1.0), (0.7, 1.0)], vec![(0.35, 1.0), (0.35, 0.0)]],
        ),
        'O' => (0.76, vec![arc(0.0, 360.0, 0.38, 0.38)]),
        'C' => (0.72, vec![arc(50.0, 310.0, 0.38, 0.38)]),
        'G' => (
            0.74,
            vec![
                arc(50.0, 360.0, 0.38, 0.38),
                vec![(0.76, 0.5), (0.76, 0.42), (0.44, 0.42)],
            ],
        ),
        'P' => (0.66, vec![[vec![(0.0, 0.0)], bowl(0.48)].concat()]),
        'R' => (
            0.68,
            vec![
                [vec![(0.0, 0.0)], bowl(0.48)].concat(),
                vec![(0.36, 0.48), (0.68, 0.0)],
            ],
        ),
        'B' => (
            0.68,
            vec![
                [vec![(0.0, 0.0)], bowl(0.54)].concat(),
                vec![
                    (0.42, 0.54),
                    (0.62, 0.46),
                    (0.68, 0.27),
                    (0.62, 0.08),
                    (0.44, 0.0),
                    (0.0, 0.0),
                ],
            ],
        ),
        'M' => (
            0.84,
            vec![vec![
                (0.0, 0.0),
                (0.0, 1.0),
                (0.42, 0.32),
                (0.84, 1.0),
                (0.84, 0.0),
            ]],
        ),
        'N' => (
            0.7,
            vec![vec![(0.0, 0.0), (0.0, 1.0), (0.7, 0.0), (0.7, 1.0)]],
        ),
        'F' => (
            0.6,
            vec![
                vec![(0.6, 1.0), (0.0, 1.0), (0.0, 0.0)],
                vec![(0.0, 0.52), (0.48, 0.52)],
            ],
        ),
        'E' => (
            0.6,
            vec![
                vec![(0.6, 1.0), (0.0, 1.0), (0.0, 0.0), (0.6, 0.0)],
                vec![(0.0, 0.52), (0.5, 0.52)],
            ],
        ),
        'A' => (
            0.72,
            vec![
                vec![(0.0, 0.0), (0.36, 1.0), (0.72, 0.0)],
                vec![(0.13, 0.36), (0.59, 0.36)],
            ],
        ),
        'K' => (
            0.66,
            vec![
                vec![(0.0, 0.0), (0.0, 1.0)],
                vec![(0.66, 1.0), (0.0, 0.36)],
                vec![(0.2, 0.56), (0.66, 0.0)],
            ],
        ),
        'L' => (0.58, vec![vec![(0.0, 1.0), (0.0, 0.0), (0.58, 0.0)]]),
        'I' => (0.0, vec![vec![(0.0, 0.0), (0.0, 1.0)]]),
        'H' => (
            0.7,
            vec![
                vec![(0.0, 0.0), (0.0, 1.0)],
                vec![(0.7, 0.0), (0.7, 1.0)],
                vec![(0.0, 0.52), (0.7, 0.52)],
            ],
        ),
        _ => (0.6, vec![]),
    }
}

/// A face label's strokes in the face's own coordinates (-1 to 1 across),
/// centred and sized to fit.
fn label_strokes(label: &str) -> Vec<Vec<(f32, f32)>> {
    let gap = 0.24;
    let glyphs: Vec<_> = label.chars().map(glyph).collect();
    let width: f32 = glyphs.iter().map(|g| g.0).sum::<f32>() + gap * (glyphs.len() as f32 - 1.0);
    let h = (1.5 / width).min(0.46);
    let mut x = -width * h / 2.0;
    let mut out = vec![];
    for (w, strokes) in glyphs {
        for st in strokes {
            out.push(st.iter().map(|(a, b)| (x + a * h, (b - 0.5) * h)).collect());
        }
        x += (w + gap) * h;
    }
    out
}

/// A face's axes across it, chosen so its label reads left to right and
/// is never mirrored: sides keep their label upright; the top and bottom
/// turn theirs to whichever way reads best from where the eye is.
fn label_axes(n: [i8; 3], u: [i8; 3], right: Vec3) -> (Vec3, Vec3) {
    let n3 = v3(n);
    let u3 = if n[2] != 0 {
        [Vec3::X, Vec3::Y, -Vec3::X, -Vec3::Y]
            .into_iter()
            .max_by(|a, b| a.dot(right).total_cmp(&b.dot(right)))
            .expect("four")
    } else {
        v3(u)
    };
    (u3, n3.cross(u3))
}

fn v3(a: [i8; 3]) -> Vec3 {
    Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32)
}

/// One cell of a face, on screen, and the zone it belongs to.
struct Cell {
    zone: [i8; 3],
    points: Vec<Pos2>,
    face: usize,
}

/// The cells of the faces turned toward the eye, projected about `c` with
/// half-size `s`.
fn cells(cam: &OrbitCamera, c: Pos2, s: f32) -> Vec<Cell> {
    let (right, up, toward) = axes(cam);
    let to_screen = |p: Vec3| c + Vec2::new(p.dot(right), -p.dot(up)) * s;
    let bands = [(-1.0, -BAND), (-BAND, BAND), (BAND, 1.0)];
    let mut out = vec![];
    for (fi, (n, u, v, _)) in FACES.iter().enumerate() {
        if v3(*n).dot(toward) < 0.02 {
            continue;
        }
        for (i, a) in bands.iter().enumerate() {
            for (j, b) in bands.iter().enumerate() {
                let (n3, u3, v3_) = (v3(*n), v3(*u), v3(*v));
                let corner = |x: f32, y: f32| to_screen(n3 + u3 * x + v3_ * y);
                let points = vec![
                    corner(a.0, b.0),
                    corner(a.1, b.0),
                    corner(a.1, b.1),
                    corner(a.0, b.1),
                ];
                let mut zone = *n;
                for k in 0..3 {
                    zone[k] += u[k] * (i as i8 - 1) + v[k] * (j as i8 - 1);
                }
                out.push(Cell {
                    zone,
                    points,
                    face: fi,
                });
            }
        }
    }
    out
}

fn inside(p: Pos2, poly: &[Pos2]) -> bool {
    let mut odd = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            odd = !odd;
        }
        j = i;
    }
    odd
}

/// The zone under `p`, if any.
fn zone_at(cam: &OrbitCamera, c: Pos2, s: f32, p: Pos2) -> Option<[i8; 3]> {
    cells(cam, c, s)
        .into_iter()
        .find(|cell| inside(p, &cell.points))
        .map(|cell| cell.zone)
}

/// Shortest way round from `a` to `b`.
fn wrap(a: f32, b: f32) -> f32 {
    let d = (b - a).rem_euclid(TAU);
    a + if d > PI { d - TAU } else { d }
}

impl ViewCube {
    /// Whether a drag that started on the cube or its buttons is going on,
    /// so the view itself leaves the pointer alone.
    pub fn grabbing(&self) -> bool {
        self.grab.is_some()
    }

    /// Whether `p` is on the cube or its buttons, so a click there is the
    /// cube's and not a pick in the view.
    pub fn covers(&self, p: Pos2) -> bool {
        self.hot
            .is_some_and(|(c, r, col)| p.distance(c) < r || col.expand(2.0).contains(p))
    }

    /// Start turning the camera to `to`.
    pub fn turn_to(&mut self, cam: &OrbitCamera, to: (f32, f32), now: f64) {
        let from = cam.view_angles();
        self.turn = Some(Turn {
            from,
            to: (wrap(from.0, to.0), to.1),
            start: now,
        });
    }

    /// Move a turn on to `now`; true while it is still turning.
    pub fn step(&mut self, cam: &mut OrbitCamera, now: f64) -> bool {
        let Some(t) = self.turn else {
            return false;
        };
        let k = ((now - t.start) / TURN).clamp(0.0, 1.0) as f32;
        let e = k * k * (3.0 - 2.0 * k);
        cam.yaw = t.from.0 + (t.to.0 - t.from.0) * e;
        cam.pitch = t.from.1 + (t.to.1 - t.from.1) * e;
        if k >= 1.0 {
            self.turn = None;
            return false;
        }
        true
    }

    /// Draw the cube in the top right of `rect` and take its clicks. A
    /// drag on the cube orbits the view (returned as the drag delta).
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        cam: &OrbitCamera,
        accent: Color32,
        ortho: bool,
    ) -> (Option<Pick>, Option<Nav>) {
        let s = 25.0;
        // the buttons in a boxed column at the area's right edge, as in
        // the 2D areas, the cube to their left (MU19)
        let col_x = rect.right() - 24.0;
        let c = Pos2::new(col_x - 96.0, rect.top() + 72.0);
        let area = Rect::from_min_max(
            Pos2::new(c.x - 80.0, rect.top() + 4.0),
            // eight buttons now (MU24): the column reaches 250 px down
            Pos2::new(rect.right() - 4.0, rect.top() + 250.0),
        );
        if !rect.contains_rect(area) {
            self.area = None;
            self.hot = None;
            return (None, None);
        }
        self.area = Some(area);
        // only the cube's disc and the button column take the pointer: the
        // empty corners of the box pan and pick as the rest of the view
        // does (MU31: a pan beside the cube orbited instead)
        let column = Rect::from_min_size(
            Pos2::new(col_x - 17.0, rect.top() + 7.0),
            Vec2::new(34.0, 8.0 * 28.0 + 6.0),
        );
        self.hot = Some((c, 2.05 * s + 8.0, column));
        let hot = move |p: Pos2| p.distance(c) < 2.05 * s + 8.0 || column.expand(2.0).contains(p);
        let over = ui.input(|i| i.pointer.hover_pos()).is_some_and(hot);
        let sense = if over || self.grab.is_some() { Sense::click_and_drag() } else { Sense::hover() };
        let response = ui.interact(area, ui.id().with("viewcube"), sense);
        let mut painter = ui.painter_at(rect);
        let hover = response.hover_pos();
        // the theme's colours, light or dark (MU20)
        let pal = crate::theme::Palette::of(ui.ctx());
        // faded until the pointer comes near or it is in use
        let near = ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|h| area.expand(40.0).contains(h))
            || self.grab.is_some();
        painter.multiply_opacity(if near { 1.0 } else { 0.45 });
        // the buttons' box: the same box, gap and pitch as the drawing
        // areas' column (MU21), 28 px buttons, 10 px under the header
        painter.rect(
            column,
            8.0,
            pal.panel,
            Stroke::new(1.0, pal.border),
            egui::StrokeKind::Inside,
        );
        let (right, up, toward) = axes(cam);
        let to_screen = |p: Vec3| c + Vec2::new(p.dot(right), -p.dot(up)) * s;

        // the compass ring, on the ground under the cube
        let ring_r = 2.05;
        let ring_z = -1.05;
        let ring: Vec<Pos2> = (0..48)
            .map(|i| {
                let t = i as f32 / 48.0 * TAU;
                to_screen(Vec3::new(t.cos() * ring_r, t.sin() * ring_r, ring_z))
            })
            .collect();
        let ink = pal.text;
        let faint = pal.muted.gamma_multiply(0.6);
        painter.add(Shape::closed_line(
            ring.clone(),
            Stroke::new(5.0, pal.panel.gamma_multiply(0.6)),
        ));
        painter.add(Shape::closed_line(ring, Stroke::new(1.0, faint)));
        let mut pick = None;
        let mut tip = None;
        let clicked = response.clicked();
        let points = [
            ("N", [0i8, 1, 0]),
            ("E", [1, 0, 0]),
            ("S", [0, -1, 0]),
            ("W", [-1, 0, 0]),
        ];
        let mut letters = vec![];
        for (letter, d) in points {
            let at =
                to_screen(Vec3::new(d[0] as f32, d[1] as f32, 0.0) * ring_r + Vec3::Z * ring_z);
            let behind = (Vec3::new(d[0] as f32, d[1] as f32, 0.0) * ring_r + Vec3::Z * ring_z)
                .dot(toward)
                < 0.0;
            // a letter behind the cube is hidden where the cube covers it
            let near = hover.is_some_and(|h| {
                h.distance(at) < 8.0 && !(behind && zone_at(cam, c, s, h).is_some())
            });
            if near {
                tip = Some(format!("Look from the {}", compass(d).unwrap_or("")));
                if clicked {
                    pick = Some(Pick::From(d));
                }
            }
            letters.push((letter, at, near, behind));
        }

        let draw_letter = |letter: &str, at: Pos2, near: bool| {
            if near {
                painter.circle_filled(at, 7.5, accent);
            } else {
                painter.circle_filled(at, 6.5, pal.panel);
            }
            let colour = if near {
                Color32::WHITE
            } else if letter == "N" {
                Color32::from_rgb(170, 40, 40)
            } else {
                ink
            };
            painter.text(
                at,
                egui::Align2::CENTER_CENTER,
                letter,
                egui::FontId::proportional(9.5),
                colour,
            );
        };
        // the letters behind the cube, which it hides
        for &(letter, at, near, _) in letters.iter().filter(|l| l.3) {
            draw_letter(letter, at, near);
        }

        // the faces turned toward the eye, cell by cell
        let all = cells(cam, c, s);
        let hovered_zone = if tip.is_none() {
            hover.and_then(|h| zone_at(cam, c, s, h))
        } else {
            None
        };
        let face_fill = if pal.dark {
            pal.raise
        } else {
            Color32::from_rgb(250, 250, 251)
        };
        // whole faces first, so no seam shows between cells
        for (n, u, v, _) in FACES.iter() {
            if v3(*n).dot(toward) < 0.02 {
                continue;
            }
            let (n3, u3, v3_) = (v3(*n), v3(*u), v3(*v));
            let face: Vec<Pos2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .iter()
                .map(|(a, b)| to_screen(n3 + u3 * *a + v3_ * *b))
                .collect();
            painter.add(Shape::convex_polygon(face, face_fill, Stroke::NONE));
        }
        // a lit edge or corner is see-through enough for the face's name
        // to read under it (MU19)
        for cell in all.iter().filter(|c| hovered_zone == Some(c.zone)) {
            let whole_face = FACES.get(cell.face).is_some_and(|f| f.0 == cell.zone);
            painter.add(Shape::convex_polygon(
                cell.points.clone(),
                accent.gamma_multiply(if whole_face { 0.85 } else { 0.45 }),
                Stroke::NONE,
            ));
        }
        for (fi, (n, u, v, label)) in FACES.iter().enumerate() {
            let facing = v3(*n).dot(toward);
            if facing < 0.02 {
                continue;
            }
            let (n3, u3, v3_) = (v3(*n), v3(*u), v3(*v));
            let outline: Vec<Pos2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .iter()
                .map(|(a, b)| to_screen(n3 + u3 * *a + v3_ * *b))
                .collect();
            painter.add(Shape::closed_line(outline, Stroke::new(1.0, pal.edge)));
            if facing > 0.25 {
                // the label printed on the face, so it turns and
                // foreshortens with it and always reads the right way
                let lit = all.iter().any(|cell| {
                    cell.face == fi && Some(cell.zone) == hovered_zone && cell.zone == *n
                });
                let (lu, lv) = label_axes(*n, *u, right);
                let fade = ((facing - 0.25) / 0.35).clamp(0.0, 1.0);
                let colour = if lit {
                    Color32::WHITE
                } else {
                    ink.gamma_multiply(0.35 + 0.65 * fade)
                };
                let width = 1.15 + 0.35 * facing;
                let strokes: Vec<Vec<Pos2>> = label_strokes(label)
                    .iter()
                    .map(|st| {
                        st.iter()
                            .map(|(a, b)| to_screen(n3 + lu * *a + lv * *b))
                            .collect()
                    })
                    .collect();
                // a light halo first, so a lit edge never swallows the name
                if !lit {
                    for pts in &strokes {
                        painter.add(Shape::line(
                            pts.clone(),
                            Stroke::new(width + 2.4, face_fill.gamma_multiply(0.7)),
                        ));
                    }
                }
                for pts in strokes {
                    painter.add(Shape::line(pts, Stroke::new(width, colour)));
                }
            }
        }
        // the compass letters in front of the cube
        for &(letter, at, near, _) in letters.iter().filter(|l| !l.3) {
            draw_letter(letter, at, near);
        }
        if let Some(z) = hovered_zone {
            tip = Some(match compass(z) {
                Some(p) => format!("Look from the {} ({p})", zone_name(z)),
                None => format!("Look from the {}", zone_name(z)),
            });
            if clicked {
                pick = Some(Pick::From(z));
            }
        }

        // a press in the cube's corner starts its own drag: on the zoom or
        // pan button that (set below), anywhere else an orbit
        let (pressed, down, delta) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.delta(),
            )
        });
        let press = if pressed {
            ui.input(|i| i.pointer.press_origin())
                .filter(|p| area.contains(*p) && hot(*p))
        } else {
            None
        };
        if press.is_some() {
            self.grab = Some(Nav::Orbit(Vec2::ZERO));
        }
        // a column of round buttons beside the cube
        let buttons = [
            (Pick::Zoom, "Zoom: drag up to zoom in, down to zoom out"),
            (Pick::Pan, "Pan: drag to move the view"),
            (Pick::FitAll, "Frame everything (Home)"),
            (Pick::FitSel, "Frame the selection (.)"),
            (
                Pick::Home,
                "Home: frame everything from the three-quarter view",
            ),
            (Pick::Quarter(1), "Turn the view a quarter to the left"),
            (Pick::Quarter(-1), "Turn the view a quarter to the right"),
            (
                Pick::Projection,
                if ortho {
                    "Orthographic: click for perspective"
                } else {
                    "Perspective: click for orthographic"
                },
            ),
        ];
        for (k, (what, hint)) in buttons.into_iter().enumerate() {
            let at = Pos2::new(col_x, rect.top() + 24.0 + 28.0 * k as f32);
            let held = self.grab.is_some_and(|g| {
                matches!(
                    (g, what),
                    (Nav::Zoom(_), Pick::Zoom) | (Nav::Pan(_), Pick::Pan)
                )
            });
            let lit = held || hover.is_some_and(|h| h.distance(at) < 11.5);
            if press.is_some_and(|p| p.distance(at) < 11.5) {
                match what {
                    Pick::Zoom => self.grab = Some(Nav::Zoom(Vec2::ZERO)),
                    Pick::Pan => self.grab = Some(Nav::Pan(Vec2::ZERO)),
                    _ => {}
                }
            }
            // a rounded square with a faint accent tint when lit, as the
            // drawing areas' buttons (MU20)
            let mark = ink;
            let square = egui::Rect::from_center_size(at, Vec2::splat(24.0));
            if lit {
                painter.rect_filled(square, 6.0, accent.gamma_multiply(0.18));
            }
            // the drawing areas' icons, 16 px in a 28 px cell, where one
            // exists (MU24: the two columns looked different)
            let svg = match what {
                Pick::Zoom => Some("zoom-in"),
                Pick::Pan => Some("hand"),
                Pick::FitAll => Some("frame-all"),
                Pick::FitSel => Some("frame-selection"),
                Pick::Home => Some("house"),
                _ => None,
            };
            match svg {
                Some(name) => crate::svgicon::paint(
                    &painter,
                    egui::Rect::from_center_size(at, Vec2::splat(16.0)),
                    name,
                    mark,
                ),
                None => button_icon(&painter, at, what, ortho, mark),
            }
            if lit {
                tip = Some(hint.to_string());
                if clicked && !matches!(what, Pick::Zoom | Pick::Pan) {
                    pick = Some(what);
                }
            }
        }
        if !down {
            self.grab = None;
        }
        // the press frame's movement came before the press
        let nav = self
            .grab
            .filter(|_| !pressed && delta != Vec2::ZERO)
            .map(|g| match g {
                Nav::Orbit(_) => Nav::Orbit(delta),
                Nav::Pan(_) => Nav::Pan(delta),
                Nav::Zoom(_) => Nav::Zoom(delta),
            });
        if let Some(t) = tip.filter(|_| !response.dragged()) {
            response.on_hover_text(t);
        }
        (pick, nav)
    }
}

/// The icon on one of the cube's round buttons, centred on `c`.
fn button_icon(painter: &egui::Painter, c: Pos2, what: Pick, ortho: bool, colour: Color32) {
    let stroke = Stroke::new(1.6, colour);
    let line = |pts: &[(f32, f32)]| {
        painter.add(Shape::line(
            pts.iter().map(|(x, y)| c + Vec2::new(*x, *y)).collect(),
            stroke,
        ));
    };
    match what {
        Pick::Home => {
            line(&[(-5.5, -0.5), (0.0, -5.5), (5.5, -0.5)]);
            line(&[(-3.8, -1.8), (-3.8, 4.8), (3.8, 4.8), (3.8, -1.8)]);
            line(&[(-1.0, 4.8), (-1.0, 1.6), (1.0, 1.6), (1.0, 4.8)]);
        }
        Pick::Quarter(q) => {
            // three quarters of a circle and its arrowhead
            // left turns anticlockwise, right clockwise
            let dir = -(q as f32);
            let r = 6.0;
            let pts: Vec<(f32, f32)> = (0..=18)
                .map(|i| {
                    let t = (-60.0 + 270.0 * i as f32 / 18.0).to_radians();
                    (-dir * t.cos() * r, -t.sin() * r)
                })
                .collect();
            line(&pts);
            let (ex, ey) = pts[pts.len() - 1];
            let tip = c + Vec2::new(ex, ey);
            let head = vec![
                tip + Vec2::new(dir * 3.2, -2.4),
                tip + Vec2::new(dir * 3.2, 2.6),
                tip + Vec2::new(-dir * 1.6, 0.0),
            ];
            painter.add(Shape::convex_polygon(head, colour, Stroke::NONE));
        }
        Pick::Projection if ortho => {
            // a square grid: lines stay parallel
            line(&[
                (-5.5, -4.5),
                (5.5, -4.5),
                (5.5, 4.5),
                (-5.5, 4.5),
                (-5.5, -4.5),
            ]);
            line(&[(-1.8, -4.5), (-1.8, 4.5)]);
            line(&[(1.8, -4.5), (1.8, 4.5)]);
            line(&[(-5.5, 0.0), (5.5, 0.0)]);
        }
        Pick::Projection => {
            // a grid going away: lines meet toward the distance
            line(&[
                (-3.2, -4.5),
                (3.2, -4.5),
                (6.5, 4.5),
                (-6.5, 4.5),
                (-3.2, -4.5),
            ]);
            line(&[(-1.0, -4.5), (-2.2, 4.5)]);
            line(&[(1.0, -4.5), (2.2, 4.5)]);
            line(&[(-4.6, -0.6), (4.6, -0.6)]);
        }
        Pick::Zoom => {
            // a magnifier
            painter.circle_stroke(c + Vec2::new(-1.2, -1.2), 4.2, stroke);
            line(&[(1.9, 1.9), (5.6, 5.6)]);
            line(&[(-3.2, -1.2), (0.8, -1.2)]);
            line(&[(-1.2, -3.2), (-1.2, 0.8)]);
        }
        Pick::Pan => {
            // four ways out from the middle
            line(&[(-6.0, 0.0), (6.0, 0.0)]);
            line(&[(0.0, -6.0), (0.0, 6.0)]);
            for (dx, dy) in [(1.0f32, 0.0f32), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                let tip = c + Vec2::new(dx * 6.5, dy * 6.5);
                let back = Vec2::new(-dx, -dy) * 2.8;
                let side = Vec2::new(-dy, dx) * 2.2;
                painter.add(Shape::convex_polygon(
                    vec![tip, tip + back + side, tip + back - side],
                    colour,
                    Stroke::NONE,
                ));
            }
        }
        // drawn from the icon set (see the column)
        Pick::From(_) | Pick::FitAll | Pick::FitSel => {}
    }
}

/// The yaw and pitch a pick asks for, or None for Home (which the caller
/// frames).
pub fn target_of(pick: Pick, cam: &OrbitCamera) -> Option<(f32, f32)> {
    let (yaw, pitch) = cam.view_angles();
    match pick {
        Pick::From(d) => Some(angles_from(d)),
        // squared to the nearest eighth, so a corner view stays a corner
        Pick::Quarter(q) => Some((
            (yaw / FRAC_PI_4).round() * FRAC_PI_4 + q as f32 * FRAC_PI_2,
            pitch,
        )),
        Pick::Home | Pick::Projection | Pick::Zoom | Pick::Pan | Pick::FitAll | Pick::FitSel => None,
    }
}

/// What the status bar says once the view has turned.
pub fn said(pick: Pick) -> String {
    match pick {
        Pick::From(d) => match compass(d) {
            Some(p) => format!("Looking from the {} ({p}).", zone_name(d)),
            None => format!("Looking from the {}.", zone_name(d)),
        },
        Pick::Quarter(1) => "Turned a quarter to the left.".into(),
        Pick::Quarter(_) => "Turned a quarter to the right.".into(),
        Pick::Home => "Framed everything.".into(),
        Pick::FitAll => "Framed everything.".into(),
        Pick::FitSel => "Framed the selection.".into(),
        Pick::Projection | Pick::Zoom | Pick::Pan => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look_dir(cam: &OrbitCamera) -> Vec3 {
        (cam.eye() - cam.target).normalize()
    }

    #[test]
    fn there_are_26_zones_each_looking_from_its_own_side() {
        let mut zones = std::collections::HashSet::new();
        for x in -1..=1i8 {
            for y in -1..=1i8 {
                for z in -1..=1i8 {
                    if (x, y, z) != (0, 0, 0) {
                        zones.insert([x, y, z]);
                    }
                }
            }
        }
        assert_eq!(zones.len(), 26);
        for d in zones {
            let mut cam = OrbitCamera::default();
            (cam.yaw, cam.pitch) = angles_from(d);
            let want = v3(d).normalize();
            assert!(look_dir(&cam).dot(want) > 0.999, "{d:?}");
        }
    }

    #[test]
    fn top_keeps_north_up_from_any_side() {
        for yaw in [0.3, 2.5, -2.9] {
            let mut cam = OrbitCamera {
                yaw,
                ..OrbitCamera::default()
            };
            let to = target_of(Pick::From([0, 0, 1]), &cam).unwrap();
            (cam.yaw, cam.pitch) = to;
            assert!(cam.up().y > 0.99, "north up from yaw {yaw}");
        }
    }

    #[test]
    fn front_is_the_south_face() {
        let mut cam = OrbitCamera::default();
        (cam.yaw, cam.pitch) = angles_from([0, -1, 0]);
        assert!(
            look_dir(&cam).y < -0.99,
            "the eye stands south of the model"
        );
        assert_eq!(
            said(Pick::From([0, -1, 0])),
            "Looking from the front (south)."
        );
    }

    #[test]
    fn every_zone_the_eye_faces_can_be_clicked() {
        // from the three-quarter view, the front-left-top corner and the
        // three faces round it are on the cube
        let cam = OrbitCamera::default();
        let c = Pos2::new(100.0, 100.0);
        let cells = cells(&cam, c, 22.0);
        let zones: std::collections::HashSet<_> = cells.iter().map(|c| c.zone).collect();
        assert_eq!(zones.len(), 19, "3 faces, 9 edges, 7 corners");
        for want in [[0, 0, 1], [0, -1, 0], [-1, 0, 0], [-1, -1, 1]] {
            let cell = cells.iter().find(|c| c.zone == want).expect("seen");
            let mid = cell.points.iter().fold(Vec2::ZERO, |a, p| a + p.to_vec2()) / 4.0;
            assert_eq!(zone_at(&cam, c, 22.0, mid.to_pos2()), Some(want));
        }
    }

    #[test]
    fn a_turn_takes_the_short_way_and_ends_on_the_view() {
        let mut cam = OrbitCamera {
            yaw: 3.0,
            pitch: 0.2,
            ..OrbitCamera::default()
        };
        let mut cube = ViewCube::default();
        cube.turn_to(&cam, (-3.0, 0.0), 0.0);
        assert!(cube.step(&mut cam, 0.1));
        assert!(cam.yaw > 3.0, "through π, not back round through 0");
        assert!(!cube.step(&mut cam, 1.0));
        assert!((cam.yaw.rem_euclid(TAU) - (-3.0f32).rem_euclid(TAU)).abs() < 1e-4);
        assert_eq!(cam.pitch, 0.0);
    }

    #[test]
    fn a_quarter_turn_squares_the_view() {
        let mut cam = OrbitCamera {
            yaw: 0.1,
            ..OrbitCamera::default()
        };
        let (yaw, _) = target_of(Pick::Quarter(1), &cam).unwrap();
        assert!((yaw - FRAC_PI_2).abs() < 1e-5);
        // a corner view turns to the next corner
        cam.yaw = -FRAC_PI_4 + 0.05;
        let (yaw, _) = target_of(Pick::Quarter(-1), &cam).unwrap();
        assert!((yaw + 3.0 * FRAC_PI_4).abs() < 1e-5);
    }
}
