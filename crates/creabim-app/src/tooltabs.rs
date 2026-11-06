//! The tools row's tabs (brief 02, 2.1): Model, Annotate, View and Manage
//! each show their own tools and commands, icons only, while Select and
//! Align stay at the left on every tab. A tool picked by its key shows its
//! tab. A tool can be on two tabs where it belongs to both: the callout.

use crate::planview::Tool;

/// One button of a tab.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Entry {
    Tool(Tool),
    /// A command of the menus, by its action id.
    Act {
        id: &'static str,
        label: &'static str,
        icon: &'static str,
        hint: &'static str,
    },
    /// A short menu of commands (see [`menu`]).
    Menu {
        key: &'static str,
        label: &'static str,
        icon: &'static str,
        hint: &'static str,
    },
    /// A divider between groups.
    Sep,
}

pub const TABS: [&str; 4] = ["Model", "Annotate", "View", "Manage"];

/// On every tab, before it.
pub const ALWAYS: [Tool; 2] = [Tool::Select, Tool::Align];

/// What tab `tab` holds.
pub fn entries(tab: usize) -> Vec<Entry> {
    use Entry::{Sep, Tool as T};
    let act = |id, label, icon, hint| Entry::Act {
        id,
        label,
        icon,
        hint,
    };
    match tab {
        0 => vec![
            T(Tool::Wall),
            act(
                "tool.curtain",
                "Curtain wall",
                "curtain-wall",
                "The Wall tool drawing a curtain wall type: mullions and glass on a grid",
            ),
            T(Tool::Door),
            T(Tool::Window),
            T(Tool::Opening),
            Sep,
            T(Tool::Slab),
            T(Tool::Shaft),
            T(Tool::Roof),
            T(Tool::Stair),
            T(Tool::Railing),
            Sep,
            T(Tool::Column),
            T(Tool::Beam),
            T(Tool::Footing),
            Sep,
            T(Tool::Object),
            act(
                "catalogue",
                "Catalogue",
                "sofa",
                "Furniture and fixtures to place, and your own from Blender, FreeCAD or glTF",
            ),
            Sep,
            T(Tool::Mass),
            T(Tool::Extrusion),
            T(Tool::Sweep),
            T(Tool::Void),
            T(Tool::Sketch),
            Sep,
            T(Tool::Room),
            T(Tool::RoomSeparator),
            T(Tool::Ceiling),
            Sep,
            T(Tool::Terrain),
            T(Tool::Paving),
            T(Tool::Planting),
        ],
        1 => vec![
            T(Tool::Dimension),
            T(Tool::Angle),
            T(Tool::Radius),
            T(Tool::Diameter),
            T(Tool::ArcLength),
            T(Tool::SpotLevel),
            Sep,
            T(Tool::Tag),
            T(Tool::Note),
            Sep,
            T(Tool::Detail),
            T(Tool::Callout),
            Sep,
            act(
                "styles",
                "Annotation styles",
                "anno-styles",
                "Named styles for dimensions, notes and tags: text, ends, tag shape, colour",
            ),
            act(
                "marks",
                "Mark numbering",
                "tag",
                "How doors, windows and other elements are numbered; renumber, and check for marks used twice",
            ),
        ],
        2 => vec![
            // levels are what plans are cut from, so they sit with the
            // views that come from them (MU20)
            T(Tool::Level),
            act(
                "levels",
                "Levels",
                "level",
                "Add, duplicate, rename, move and delete levels",
            ),
            Sep,
            T(Tool::Section),
            T(Tool::Elevation),
            T(Tool::Callout),
            Sep,
            act(
                "crop.add",
                "Crop box",
                "crop-box",
                "Add a crop box to the 3D view: it crops everything outside it",
            ),
            act("new.sheet", "New sheet", "file-text", "A new drawing sheet"),
            Entry::Menu {
                key: "tab:schedules",
                label: "New schedule",
                icon: "table-2",
                hint: "A schedule of doors, windows, walls, rooms and more",
            },
            Sep,
            act(
                "graphicstyles",
                "Graphic styles",
                "align-left",
                "Style packs: pens by scale, object styles, hatch patterns and view templates",
            ),
            act(
                "templates",
                "Sheet templates",
                "sheet-template",
                "Title blocks for sheets",
            ),
        ],
        _ => vec![
            T(Tool::Grid),
            T(Tool::Guide),
            Sep,
            act(
                "project.info",
                "Project information",
                "info",
                "Name, client, address, site and building: they fill the title blocks and the IFC export",
            ),

            act(
                "types.browser",
                "Types",
                "element-types",
                "Wall, curtain wall, door, window, slab, roof, column, beam and object types",
            ),
            act(
                "mat.browser",
                "Materials",
                "palette",
                "Add, edit, delete and assign the project's materials",
            ),
            act(
                "graphicstyles",
                "Style packs",
                "align-left",
                "Pens by scale, object styles, hatch patterns and view templates",
            ),
            act(
                "units",
                "Project units",
                "units",
                "Metric or imperial: how lengths are shown and typed",
            ),
            Sep,
            Entry::Menu {
                key: "tab:ifc",
                label: "IFC",
                icon: "upload",
                hint: "Open an IFC file, or export this project as IFC4 or IFC5",
            },
        ],
    }
}

/// Does tab `tab` show `tool`?
pub fn holds(tab: usize, tool: Tool) -> bool {
    ALWAYS.contains(&tool) || entries(tab).contains(&Entry::Tool(tool))
}

/// The first tab that shows `tool`.
pub fn home_of(tool: Tool) -> usize {
    (0..TABS.len()).find(|t| holds(*t, tool)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_is_on_a_tab() {
        for t in Tool::ALL {
            assert!(
                (0..TABS.len()).any(|tab| holds(tab, t)),
                "{} is on no tab",
                t.label()
            );
        }
        assert_eq!(home_of(Tool::Wall), 0);
        assert_eq!(home_of(Tool::Dimension), 1);
        assert_eq!(home_of(Tool::Section), 2);
        assert_eq!(home_of(Tool::Grid), 3);
        // the callout is on both of its tabs
        assert!(holds(1, Tool::Callout) && holds(2, Tool::Callout));
        // Select is on every one
        assert!((0..TABS.len()).all(|t| holds(t, Tool::Select)));
    }
}
