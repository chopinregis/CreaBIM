//! The interface's line icons (ui/icons.js) drawn in egui too, so a panel
//! drawn by egui shows the same icons as the web rows: each icon's paths,
//! lines, circles and rectangles turned into polylines on its 24 × 24
//! grid once, then stroked at any size.

use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use std::collections::HashMap;
use std::sync::OnceLock;

/// One stroke of an icon: its points on the 24 × 24 grid, closed or not,
/// and dashed or not.
#[derive(Clone, Debug, Default)]
struct Line {
    pts: Vec<[f32; 2]>,
    closed: bool,
    dash: Option<(f32, f32)>,
}

fn all() -> &'static HashMap<String, Vec<Line>> {
    static ICONS: OnceLock<HashMap<String, Vec<Line>>> = OnceLock::new();
    ICONS.get_or_init(|| {
        crate::webui::icon_markup()
            .into_iter()
            .map(|(k, v)| (k, parse(&v)))
            .collect()
    })
}

/// `name` as the icon set holds it, for as long as the app runs ("" when
/// the set has no such icon).
pub fn named(name: &str) -> &'static str {
    all().get_key_value(name).map_or("", |(k, _)| k.as_str())
}

/// Does the icon set have `name`?
#[cfg(test)]
pub fn has(name: &str) -> bool {
    all().contains_key(name)
}

/// Draw icon `name` in `r` (square), in `color`.
pub fn paint(painter: &egui::Painter, r: Rect, name: &str, color: Color32) {
    let Some(lines) = all().get(name) else {
        return;
    };
    let s = r.width().min(r.height()) / 24.0;
    let o = r.center() - egui::vec2(12.0 * s, 12.0 * s);
    let at = |p: [f32; 2]| Pos2::new(o.x + p[0] * s, o.y + p[1] * s);
    let w = (1.75 * s).max(1.0);
    let stroke = Stroke::new(w, color);
    for l in lines {
        let pts: Vec<Pos2> = l.pts.iter().map(|p| at(*p)).collect();
        let len: f32 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
        if pts.len() < 2 || len < 0.5 * s {
            // a dot (h.01): round, as the stroke's cap draws it
            if let Some(p) = pts.first() {
                painter.circle_filled(*p, w * 0.7, color);
            }
            continue;
        }
        let mut pts = pts;
        if l.closed {
            pts.push(pts[0]);
        }
        match l.dash {
            Some((on, off)) => {
                let (on, off) = (on.max(0.6) * s, off * s);
                painter.extend(egui::Shape::dashed_line(&pts, stroke, on, off));
            }
            None => {
                painter.add(egui::Shape::line(pts.clone(), stroke));
                // rounded joins and ends, as the web draws them
                for p in [pts[0], pts[pts.len() - 1]] {
                    painter.circle_filled(p, w * 0.5, color);
                }
            }
        }
    }
}

/// The strokes of one icon's markup.
fn parse(svg: &str) -> Vec<Line> {
    let mut out = vec![];
    let mut rest = svg;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        let end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..end];
        rest = &rest[end..];
        let name: String = tag.chars().take_while(|c| c.is_alphanumeric()).collect();
        let attr = |a: &str| attr(tag, a);
        let num = |a: &str| {
            attr(a)
                .and_then(|v| v.trim().parse::<f32>().ok())
                .unwrap_or(0.0)
        };
        let dash = attr("stroke-dasharray").and_then(|d| {
            let v: Vec<f32> = numbers(&d);
            Some((*v.first()?, *v.get(1).or(v.first())?))
        });
        let mut lines = match name.as_str() {
            "path" => path(&attr("d").unwrap_or_default()),
            "line" => vec![Line {
                pts: vec![[num("x1"), num("y1")], [num("x2"), num("y2")]],
                ..Default::default()
            }],
            "polyline" | "polygon" => {
                let v = numbers(&attr("points").unwrap_or_default());
                vec![Line {
                    pts: v
                        .chunks(2)
                        .filter(|c| c.len() == 2)
                        .map(|c| [c[0], c[1]])
                        .collect(),
                    closed: name == "polygon",
                    ..Default::default()
                }]
            }
            "circle" => vec![ellipse(num("cx"), num("cy"), num("r"), num("r"))],
            "ellipse" => vec![ellipse(num("cx"), num("cy"), num("rx"), num("ry"))],
            "rect" => {
                let (x, y, w, h) = (num("x"), num("y"), num("width"), num("height"));
                let rx = num("rx").max(num("ry")).min(w / 2.0).min(h / 2.0);
                if rx > 0.0 {
                    let d = format!(
                        "M{} {}h{}a{rx} {rx} 0 0 1 {rx} {rx}v{}a{rx} {rx} 0 0 1 -{rx} {rx}h-{}a{rx} {rx} 0 0 1 -{rx} -{rx}v-{}a{rx} {rx} 0 0 1 {rx} -{rx}z",
                        x + rx, y, w - 2.0 * rx, h - 2.0 * rx, w - 2.0 * rx, h - 2.0 * rx
                    );
                    path(&d)
                } else {
                    vec![Line {
                        pts: vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]],
                        closed: true,
                        ..Default::default()
                    }]
                }
            }
            _ => vec![],
        };
        // the mark's masks and fills are not line icons
        if attr("fill").is_some_and(|f| f != "none") && name != "path" {
            lines.clear();
        }
        for l in &mut lines {
            l.dash = dash;
        }
        out.extend(lines);
    }
    out
}

fn attr(tag: &str, a: &str) -> Option<String> {
    let key = format!(" {a}=\"");
    let i = tag.find(&key)? + key.len();
    let j = tag[i..].find('"')? + i;
    Some(tag[i..j].replace("\\\"", "\""))
}

fn numbers(s: &str) -> Vec<f32> {
    let mut out = vec![];
    let mut t = Tokens::new(s);
    while let Some(v) = t.num() {
        out.push(v);
    }
    out
}

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Line {
    let n = 28;
    Line {
        pts: (0..n)
            .map(|i| {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                [cx + rx * a.cos(), cy + ry * a.sin()]
            })
            .collect(),
        closed: true,
        dash: None,
    }
}

/// The numbers and commands of a path's `d`.
struct Tokens<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Tokens<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            s: s.as_bytes(),
            i: 0,
        }
    }
    fn skip(&mut self) {
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',')
        {
            self.i += 1;
        }
    }
    fn command(&mut self) -> Option<u8> {
        self.skip();
        let c = *self.s.get(self.i)?;
        if c.is_ascii_alphabetic() {
            self.i += 1;
            Some(c)
        } else {
            None
        }
    }
    fn at_number(&mut self) -> bool {
        self.skip();
        self.s
            .get(self.i)
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.'))
    }
    fn num(&mut self) -> Option<f32> {
        if !self.at_number() {
            return None;
        }
        let start = self.i;
        let mut seen_dot = false;
        let mut seen_e = false;
        if matches!(self.s[self.i], b'-' | b'+') {
            self.i += 1;
        }
        while self.i < self.s.len() {
            let c = self.s[self.i];
            if c.is_ascii_digit() {
                self.i += 1;
            } else if c == b'.' && !seen_dot && !seen_e {
                seen_dot = true;
                self.i += 1;
            } else if (c == b'e' || c == b'E') && !seen_e {
                seen_e = true;
                self.i += 1;
                if self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+') {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()?
            .parse()
            .ok()
    }
    /// An arc's flag: a lone 0 or 1, which may run into the next number.
    fn flag(&mut self) -> Option<bool> {
        if !self.at_number() {
            return None;
        }
        let c = self.s[self.i];
        self.i += 1;
        Some(c == b'1')
    }
}

/// A path's strokes, curves and arcs flattened.
fn path(d: &str) -> Vec<Line> {
    let mut out: Vec<Line> = vec![];
    let mut cur = Line::default();
    let mut t = Tokens::new(d);
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    // the last control point, for S and T
    let mut last_c: Option<[f32; 2]> = None;
    let mut cmd = b'M';
    let flush = |cur: &mut Line, out: &mut Vec<Line>| {
        if !cur.pts.is_empty() {
            out.push(std::mem::take(cur));
        }
    };
    loop {
        if let Some(c) = t.command() {
            cmd = c;
        } else if !t.at_number() {
            break;
        }
        let rel = cmd.is_ascii_lowercase();
        let (bx, by) = if rel { (x, y) } else { (0.0, 0.0) };
        // a path may go on from where it closed
        if cur.pts.is_empty() && !matches!(cmd.to_ascii_uppercase(), b'M' | b'Z') {
            cur.pts.push([x, y]);
        }
        match cmd.to_ascii_uppercase() {
            b'M' => {
                let (Some(a), Some(b)) = (t.num(), t.num()) else {
                    break;
                };
                flush(&mut cur, &mut out);
                x = bx + a;
                y = by + b;
                sx = x;
                sy = y;
                cur.pts.push([x, y]);
                // further pairs are lines
                cmd = if rel { b'l' } else { b'L' };
                last_c = None;
            }
            b'L' => {
                let (Some(a), Some(b)) = (t.num(), t.num()) else {
                    break;
                };
                x = bx + a;
                y = by + b;
                cur.pts.push([x, y]);
                last_c = None;
            }
            b'H' => {
                let Some(a) = t.num() else { break };
                x = if rel { x + a } else { a };
                cur.pts.push([x, y]);
                last_c = None;
            }
            b'V' => {
                let Some(a) = t.num() else { break };
                y = if rel { y + a } else { a };
                cur.pts.push([x, y]);
                last_c = None;
            }
            b'C' | b'S' => {
                let c1 = if cmd.eq_ignore_ascii_case(&b'C') {
                    let (Some(a), Some(b)) = (t.num(), t.num()) else {
                        break;
                    };
                    [bx + a, by + b]
                } else {
                    last_c.map_or([x, y], |c| [2.0 * x - c[0], 2.0 * y - c[1]])
                };
                let (Some(a), Some(b), Some(e), Some(f)) = (t.num(), t.num(), t.num(), t.num())
                else {
                    break;
                };
                let c2 = [bx + a, by + b];
                let p = [bx + e, by + f];
                let p0 = [x, y];
                for i in 1..=12 {
                    let u = i as f32 / 12.0;
                    let v = 1.0 - u;
                    let w = [v * v * v, 3.0 * v * v * u, 3.0 * v * u * u, u * u * u];
                    cur.pts.push([
                        w[0] * p0[0] + w[1] * c1[0] + w[2] * c2[0] + w[3] * p[0],
                        w[0] * p0[1] + w[1] * c1[1] + w[2] * c2[1] + w[3] * p[1],
                    ]);
                }
                last_c = Some(c2);
                [x, y] = p;
            }
            b'Q' | b'T' => {
                let c1 = if cmd.eq_ignore_ascii_case(&b'Q') {
                    let (Some(a), Some(b)) = (t.num(), t.num()) else {
                        break;
                    };
                    [bx + a, by + b]
                } else {
                    last_c.map_or([x, y], |c| [2.0 * x - c[0], 2.0 * y - c[1]])
                };
                let (Some(e), Some(f)) = (t.num(), t.num()) else {
                    break;
                };
                let p = [bx + e, by + f];
                let p0 = [x, y];
                for i in 1..=10 {
                    let u = i as f32 / 10.0;
                    let v = 1.0 - u;
                    cur.pts.push([
                        v * v * p0[0] + 2.0 * v * u * c1[0] + u * u * p[0],
                        v * v * p0[1] + 2.0 * v * u * c1[1] + u * u * p[1],
                    ]);
                }
                last_c = Some(c1);
                [x, y] = p;
            }
            b'A' => {
                let (Some(rx), Some(ry), Some(rot)) = (t.num(), t.num(), t.num()) else {
                    break;
                };
                let (Some(large), Some(sweep)) = (t.flag(), t.flag()) else {
                    break;
                };
                let (Some(e), Some(f)) = (t.num(), t.num()) else {
                    break;
                };
                let p = [bx + e, by + f];
                arc(&mut cur.pts, [x, y], p, rx, ry, rot, large, sweep);
                [x, y] = p;
                last_c = None;
            }
            b'Z' => {
                cur.closed = true;
                flush(&mut cur, &mut out);
                x = sx;
                y = sy;
                last_c = None;
            }
            _ => break,
        }
    }
    if !cur.pts.is_empty() {
        out.push(cur);
    }
    out
}

/// An SVG arc from `p0` to `p1`, flattened onto `pts` (the end point too).
#[allow(clippy::too_many_arguments)]
fn arc(
    pts: &mut Vec<[f32; 2]>,
    p0: [f32; 2],
    p1: [f32; 2],
    rx: f32,
    ry: f32,
    rot: f32,
    large: bool,
    sweep: bool,
) {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-6 || ry < 1e-6 || (p0[0] == p1[0] && p0[1] == p1[1]) {
        pts.push(p1);
        return;
    }
    let phi = rot.to_radians();
    let (cs, sn) = (phi.cos(), phi.sin());
    let dx = (p0[0] - p1[0]) / 2.0;
    let dy = (p0[1] - p1[1]) / 2.0;
    let x1 = cs * dx + sn * dy;
    let y1 = -sn * dx + cs * dy;
    let lam = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lam > 1.0 {
        rx *= lam.sqrt();
        ry *= lam.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut k = (num / den).max(0.0).sqrt();
    if large == sweep {
        k = -k;
    }
    let cxp = k * rx * y1 / ry;
    let cyp = -k * ry * x1 / rx;
    let cx = cs * cxp - sn * cyp + (p0[0] + p1[0]) / 2.0;
    let cy = sn * cxp + cs * cyp + (p0[1] + p1[1]) / 2.0;
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let t1 = ang(1.0, 0.0, (x1 - cxp) / rx, (y1 - cyp) / ry);
    let mut dt = ang(
        (x1 - cxp) / rx,
        (y1 - cyp) / ry,
        (-x1 - cxp) / rx,
        (-y1 - cyp) / ry,
    );
    let tau = std::f32::consts::TAU;
    if !sweep && dt > 0.0 {
        dt -= tau;
    } else if sweep && dt < 0.0 {
        dt += tau;
    }
    let n = ((dt.abs() / tau * 32.0).ceil() as usize).max(2);
    for i in 1..=n {
        let a = t1 + dt * i as f32 / n as f32;
        let (x, y) = (rx * a.cos(), ry * a.sin());
        pts.push([cs * x - sn * y + cx, sn * x + cs * y + cy]);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn report_icon_extents() {
        let mut v: Vec<(String, f32)> = all()
            .iter()
            .map(|(k, lines)| {
                let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
                for l in lines {
                    for p in &l.pts {
                        for a in 0..2 {
                            lo[a] = lo[a].min(p[a]);
                            hi[a] = hi[a].max(p[a]);
                        }
                    }
                }
                (k.clone(), (hi[0] - lo[0]).max(hi[1] - lo[1]))
            })
            .collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        for (k, e) in v {
            println!("EXT {e:5.1} {k}");
        }
    }

    use super::*;

    #[test]
    fn the_action_icons_turn_into_strokes() {
        for k in [
            "edit-outline",
            "duplicate",
            "select-similar",
            "eye-off",
            "isolate",
            "frame-selection",
            "open-view",
            "flip",
            "make-type",
            "attach-top",
            "select-tagged",
            "straighten-leader",
            "ellipsis",
            "trash-2",
            "door-open",
        ] {
            let l = all()
                .get(k)
                .unwrap_or_else(|| panic!("{k} is not in the set"));
            assert!(!l.is_empty(), "{k} has no strokes");
            for s in l {
                for p in &s.pts {
                    assert!(
                        (-1.0..=25.0).contains(&p[0]) && (-1.0..=25.0).contains(&p[1]),
                        "{k}: {p:?} is off its grid"
                    );
                }
            }
        }
        // a circle drawn by an arc pair stays round: every point 9 from the centre
        let iso = &all()["isolate"][0];
        assert!(iso
            .pts
            .iter()
            .all(|p| ((p[0] - 12.0).hypot(p[1] - 12.0) - 9.0).abs() < 0.05));
        assert!(iso.dash.is_some());
    }
}
