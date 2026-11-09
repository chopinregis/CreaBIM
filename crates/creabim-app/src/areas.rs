//! View areas and workspaces (brief 02, 2.6): the window body is split into
//! areas, each showing one view (the 3D view, or a plan, section, sheet or
//! schedule) under its own header. A workspace is a named layout of areas.
//!
//! This is the layout alone: a tree of splits whose leaves are areas, the
//! rectangles it gives, and splitting, joining and resizing. The app draws
//! it (see `areasui.rs`).

use creabim_core::ElementId;
use eframe::egui::{self, Rect};
use serde::{Deserialize, Serialize};

/// What an area shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shows {
    /// The 3D view (there is one: a second 3D area shows the first's camera).
    Model,
    /// A plan, section, elevation, detail, sheet or schedule.
    View(ElementId),
    /// In a workspace not yet opened: the drawing open now.
    Current,
    /// In a workspace not yet opened: the sheet the drawing open now is
    /// placed on, or the first sheet.
    SheetOfCurrent,
    /// The project's notepad, as a view of its own (MU28, as Blender's text
    /// editor in an area).
    Notes,
    /// The profile library and its editor (MU36: a profile as its own view).
    Profiles,
}

/// A layout of areas: one area, or two side by side (`across`) or one
/// above the other, the first taking `frac` of the room.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Node {
    Area(Shows),
    Split {
        across: bool,
        frac: f32,
        kids: Box<[Node; 2]>,
    },
}

/// The gap between two areas, in points: what is dragged to resize them.
pub const GAP: f32 = 4.0;
/// The smallest an area gets, in points.
pub const MIN: f32 = 120.0;

/// A divider: the path to its split (0 the first child, 1 the second), the
/// strip it is drawn in, whether its areas are side by side, and the whole
/// rectangle of its split.
#[derive(Clone, Debug, PartialEq)]
pub struct Divider {
    pub path: Vec<u8>,
    pub strip: Rect,
    pub across: bool,
    pub whole: Rect,
}

impl Node {
    pub fn split(across: bool, frac: f32, a: Node, b: Node) -> Node {
        Node::Split {
            across,
            frac,
            kids: Box::new([a, b]),
        }
    }

    /// Every area, in order (left to right, top to bottom).
    pub fn areas(&self) -> Vec<Shows> {
        match self {
            Node::Area(s) => vec![*s],
            Node::Split { kids, .. } => {
                let mut v = kids[0].areas();
                v.extend(kids[1].areas());
                v
            }
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Node::Area(_) => 1,
            Node::Split { kids, .. } => kids[0].len() + kids[1].len(),
        }
    }

    /// Each area's rectangle in `r`, in order, and the dividers between.
    pub fn layout(&self, r: Rect) -> (Vec<Rect>, Vec<Divider>) {
        let mut rects = vec![];
        let mut dividers = vec![];
        self.lay(r, &mut vec![], &mut rects, &mut dividers);
        (rects, dividers)
    }

    fn lay(&self, r: Rect, path: &mut Vec<u8>, rects: &mut Vec<Rect>, divs: &mut Vec<Divider>) {
        match self {
            Node::Area(_) => rects.push(r),
            Node::Split { across, frac, kids } => {
                let (a, strip, b) = cut(r, *across, *frac);
                divs.push(Divider {
                    path: path.clone(),
                    strip,
                    across: *across,
                    whole: r,
                });
                path.push(0);
                kids[0].lay(a, path, rects, divs);
                path.pop();
                path.push(1);
                kids[1].lay(b, path, rects, divs);
                path.pop();
            }
        }
    }

    /// The area at `i` (in [`Self::areas`] order).
    pub fn get_mut(&mut self, i: usize) -> Option<&mut Shows> {
        match self {
            Node::Area(s) => (i == 0).then_some(s),
            Node::Split { kids, .. } => {
                let n = kids[0].len();
                if i < n {
                    kids[0].get_mut(i)
                } else {
                    kids[1].get_mut(i - n)
                }
            }
        }
    }

    /// The split at `path`.
    pub fn split_at(&mut self, path: &[u8]) -> Option<&mut Node> {
        match path.split_first() {
            None => Some(self),
            Some((k, rest)) => match self {
                Node::Split { kids, .. } => kids.get_mut(*k as usize)?.split_at(rest),
                Node::Area(_) => None,
            },
        }
    }

    /// Set the divider at `path` so its line is at `at` (x when side by
    /// side, else y) inside `whole`, keeping each side at least [`MIN`].
    pub fn drag(&mut self, path: &[u8], whole: Rect, at: f32) {
        if let Some(Node::Split { across, frac, .. }) = self.split_at(path) {
            let (lo, size) = if *across {
                (whole.left(), whole.width())
            } else {
                (whole.top(), whole.height())
            };
            let room = (size - GAP).max(1.0);
            let min = (MIN / room).min(0.5);
            *frac = ((at - lo - GAP / 2.0) / room).clamp(min, 1.0 - min);
        }
    }

    /// Split area `i` in two, side by side (`across`) or one above the
    /// other, the new half showing `new`. Returns the new area's index.
    pub fn divide(&mut self, i: usize, across: bool, new: Shows) -> usize {
        self.divide_at(i, across, new, 0.5)
    }

    /// [`Self::divide`], the old area keeping `frac` of the room.
    pub fn divide_at(&mut self, i: usize, across: bool, new: Shows, frac: f32) -> usize {
        self.divide_inner(i, across, new, frac.clamp(0.1, 0.9), 0)
    }

    fn divide_inner(
        &mut self,
        i: usize,
        across: bool,
        new: Shows,
        frac: f32,
        base: usize,
    ) -> usize {
        match self {
            Node::Area(s) => {
                let old = *s;
                *self = Node::split(across, frac, Node::Area(old), Node::Area(new));
                base + 1
            }
            Node::Split { kids, .. } => {
                let n = kids[0].len();
                if i < n {
                    kids[0].divide_inner(i, across, new, frac, base)
                } else {
                    kids[1].divide_inner(i - n, across, new, frac, base + n)
                }
            }
        }
    }

    /// Close area `i`: the area or areas beside it take its room. The last
    /// area can't be closed.
    pub fn close(&mut self, i: usize) -> bool {
        match self {
            Node::Area(_) => false,
            Node::Split { kids, .. } => {
                let n = kids[0].len();
                let (k, j) = if i < n { (0, i) } else { (1, i - n) };
                if matches!(kids[k], Node::Area(_)) {
                    *self = kids[1 - k].clone();
                    true
                } else {
                    kids[k].close(j)
                }
            }
        }
    }

    /// Does any area show the 3D view?
    pub fn has_model(&self) -> bool {
        self.areas().contains(&Shows::Model)
    }

    /// Put the views a workspace leaves open (`Current`, `SheetOfCurrent`)
    /// in: `current` and `sheet` are what they are now.
    pub fn resolve(&mut self, current: Option<ElementId>, sheet: Option<ElementId>) {
        match self {
            Node::Area(s) => {
                *s = match *s {
                    Shows::Current => current.map_or(Shows::Model, Shows::View),
                    Shows::SheetOfCurrent => sheet.or(current).map_or(Shows::Model, Shows::View),
                    s => s,
                }
            }
            Node::Split { kids, .. } => {
                for k in kids.iter_mut() {
                    k.resolve(current, sheet);
                }
            }
        }
    }

    /// Views that no longer exist show the current drawing instead.
    pub fn forget(&mut self, exists: &dyn Fn(ElementId) -> bool, current: Option<ElementId>) {
        match self {
            Node::Area(s) => {
                if let Shows::View(v) = *s {
                    if !exists(v) {
                        *s = current
                            .filter(|c| exists(*c))
                            .map_or(Shows::Model, Shows::View);
                    }
                }
            }
            Node::Split { kids, .. } => {
                for k in kids.iter_mut() {
                    k.forget(exists, current);
                }
            }
        }
    }
}

/// `r` cut in two at `frac`, with the divider's strip between.
fn cut(r: Rect, across: bool, frac: f32) -> (Rect, Rect, Rect) {
    if across {
        let w = (r.width() - GAP).max(0.0);
        let x = r.left() + (w * frac).round();
        (
            Rect::from_min_max(r.min, egui::pos2(x, r.bottom())),
            Rect::from_min_max(egui::pos2(x, r.top()), egui::pos2(x + GAP, r.bottom())),
            Rect::from_min_max(egui::pos2(x + GAP, r.top()), r.max),
        )
    } else {
        let h = (r.height() - GAP).max(0.0);
        let y = r.top() + (h * frac).round();
        (
            Rect::from_min_max(r.min, egui::pos2(r.right(), y)),
            Rect::from_min_max(egui::pos2(r.left(), y), egui::pos2(r.right(), y + GAP)),
            Rect::from_min_max(egui::pos2(r.left(), y + GAP), r.max),
        )
    }
}

/// A named layout of areas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub root: Node,
    /// What it is for, for its tooltip.
    #[serde(default)]
    pub hint: String,
}

/// The workspaces that come with the app.
pub fn built_in() -> Vec<Workspace> {
    let ws = |name: &str, hint: &str, root: Node| Workspace {
        name: name.into(),
        root,
        hint: hint.into(),
    };
    vec![
        ws(
            "Model",
            "One 3D area, for modelling",
            Node::Area(Shows::Model),
        ),
        ws(
            "Drawing",
            "The drawing open now beside the 3D view",
            Node::split(
                true,
                0.5,
                Node::Area(Shows::Model),
                Node::Area(Shows::Current),
            ),
        ),
        ws(
            "Document",
            "The 3D view, the drawing open now, and the sheet it is placed on",
            Node::split(
                true,
                0.42,
                Node::Area(Shows::Model),
                Node::split(
                    false,
                    0.5,
                    Node::Area(Shows::Current),
                    Node::Area(Shows::SheetOfCurrent),
                ),
            ),
        ),
        ws(
            "Sheets",
            "A sheet beside the drawing being placed on it",
            Node::split(
                true,
                0.58,
                Node::Area(Shows::SheetOfCurrent),
                Node::Area(Shows::Current),
            ),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(w: f32, h: f32) -> Rect {
        Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h))
    }

    #[test]
    fn areas_split_resize_and_close() {
        let v = ElementId::new();
        let mut n = Node::Area(Shows::Model);
        assert_eq!(n.divide(0, true, Shows::View(v)), 1);
        let (rects, divs) = n.layout(r(1000.0, 600.0));
        assert_eq!(rects.len(), 2);
        assert_eq!(divs.len(), 1);
        // the halves and the gap fill the width
        assert!((rects[0].width() + GAP + rects[1].width() - 1000.0).abs() < 0.01);
        // split the drawing one above the other
        assert_eq!(n.divide(1, false, Shows::Model), 2);
        assert_eq!(n.areas(), [Shows::Model, Shows::View(v), Shows::Model]);
        let (rects, divs) = n.layout(r(1000.0, 600.0));
        assert!(rects[1].bottom() < rects[2].top());
        // drag the first divider: an area keeps at least MIN
        n.drag(&divs[0].path, divs[0].whole, 20.0);
        let (rects, _) = n.layout(r(1000.0, 600.0));
        assert!(rects[0].width() >= MIN - 1.0);
        // close the middle one: its neighbour below takes its room
        assert!(n.close(1));
        assert_eq!(n.areas(), [Shows::Model, Shows::Model]);
        assert!(n.close(0));
        assert!(!n.close(0), "the last area stays");
        assert_eq!(n.len(), 1);
    }

    #[test]
    fn workspaces_open_with_the_drawing_and_its_sheet() {
        let (plan, sheet) = (ElementId::new(), ElementId::new());
        let names: Vec<String> = built_in().into_iter().map(|w| w.name).collect();
        assert_eq!(names, ["Model", "Drawing", "Document", "Sheets"]);
        let mut doc = built_in()[2].root.clone();
        doc.resolve(Some(plan), Some(sheet));
        assert_eq!(
            doc.areas(),
            [Shows::Model, Shows::View(plan), Shows::View(sheet)]
        );
        // a view deleted since shows the current drawing
        doc.forget(&|id| id != sheet, Some(plan));
        assert_eq!(doc.areas()[2], Shows::View(plan));
        // a layout survives saving
        let json = serde_json::to_string(&doc).unwrap();
        assert_eq!(serde_json::from_str::<Node>(&json).unwrap(), doc);
    }
}
