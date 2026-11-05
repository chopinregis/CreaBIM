//! The CreaBIM application menu (About, Check for updates, Settings,
//! Exit), the Units switch in the status bar, the Settings window and the
//! New project window's choices.

use super::*;
use crate::ui_model::Item;
use creabim_core::{LengthUnit, Units};

/// Where a manual update check looks: the project's releases page. Nothing
/// is fetched in the background; the page opens in the browser when asked.
pub(crate) const RELEASES: &str = "https://github.com/kingbenoitbrice-fun/CreaBIM/releases";

/// One way to start a project in the New project window.
pub(crate) struct Start {
    /// The action that opens it.
    pub id: &'static str,
    pub title: &'static str,
    /// One line on what it is, or what it shows.
    pub line: &'static str,
    pub icon: &'static str,
}

/// The two large choices at the top of New project.
pub(crate) const STARTS: [Start; 2] = [
    Start {
        id: "new.empty",
        title: "Empty project",
        line: "Two levels and nothing else, in your units. The everyday start.",
        icon: "file-plus",
    },
    Start {
        id: "new.sample",
        title: "Starter house",
        line: "A complete two-storey house with drawings and sheets, ready to change.",
        icon: "house",
    },
];

/// The samples to learn from, each showing one thing.
pub(crate) const SAMPLES: [Start; 6] = [
    Start {
        id: "new.simple",
        title: "Simple house",
        line: "Walls, doors, windows, a roof",
        icon: "house",
    },
    Start {
        id: "new.layered",
        title: "Layered walls",
        line: "Assemblies and how layers join",
        icon: "layers",
    },
    Start {
        id: "new.buried",
        title: "Beam in a wall",
        line: "Structure cutting through walls",
        icon: "beam",
    },
    Start {
        id: "new.typed",
        title: "Wall types",
        line: "Types, and editing many at once",
        icon: "make-type",
    },
    Start {
        id: "new.furnished",
        title: "Furnished house",
        line: "Objects, fixtures and clearances",
        icon: "sofa",
    },
    Start {
        id: "new.garden",
        title: "House in its garden",
        line: "Terrain, paving and planting",
        icon: "planting",
    },
];

/// A short name for a unit, as the status bar shows it.
pub(crate) fn unit_short(u: LengthUnit) -> &'static str {
    match u {
        LengthUnit::Millimetres => "mm",
        LengthUnit::Centimetres => "cm",
        LengthUnit::Metres => "m",
        LengthUnit::FeetInches => "ft-in",
        LengthUnit::Inches => "in",
        LengthUnit::Feet => "ft",
    }
}

/// A unit's name in the Units list, with an example beside it.
fn unit_row(u: LengthUnit) -> (&'static str, &'static str) {
    match u {
        LengthUnit::Millimetres => ("Millimetres", "3200"),
        LengthUnit::Centimetres => ("Centimetres", "320.0"),
        LengthUnit::Metres => ("Metres", "3.200"),
        LengthUnit::FeetInches => ("Feet and inches", "10'-6\""),
        LengthUnit::Inches => ("Inches", "126\""),
        LengthUnit::Feet => ("Decimal feet", "10.50'"),
    }
}

/// The Units list's order: metric, then imperial.
const UNIT_ORDER: [LengthUnit; 6] = [
    LengthUnit::Millimetres,
    LengthUnit::Centimetres,
    LengthUnit::Metres,
    LengthUnit::FeetInches,
    LengthUnit::Inches,
    LengthUnit::Feet,
];

impl CreaBimApp {
    /// The CreaBIM menu, first in the menu bar.
    pub(super) fn app_menu(&self) -> Vec<Item> {
        vec![
            Item::new("app.about", "About CreaBIM")
                .icon("info")
                .hint("The version, the licence and the guide"),
            Item::Header(crate::help::version_line()),
            Item::Separator,
            Item::new("app.updates", "Check for updates…")
                .icon("refresh-cw")
                .hint("Opens the releases page in your browser. CreaBIM never checks by itself: it stays offline."),
            Item::new("app.settings", "Settings…")
                .key("Ctrl+,")
                .icon("settings")
                .hint("Your theme and the project's units"),
            Item::new("help:shortcuts", "Keyboard shortcuts")
                .key("Ctrl+/")
                .icon("keyboard"),
            Item::Separator,
            Item::new("app.exit", "Exit").key("Alt+F4").icon("log-out"),
        ]
    }

    /// The status bar's Units list: metric, then imperial, each with how a
    /// length looks in it.
    pub(super) fn unit_items(&self) -> Vec<Item> {
        let now = self.doc.model().units().length;
        let mut items = vec![Item::Header("Metric".into())];
        for (i, u) in UNIT_ORDER.into_iter().enumerate() {
            if u == LengthUnit::FeetInches {
                items.push(Item::Header("Imperial".into()));
            }
            let (name, example) = unit_row(u);
            items.push(
                Item::new(format!("units.set:{i}"), name)
                    .key(example)
                    .checked(u == now)
                    .hint("Applies at once to typing, readouts and dimensions, and is saved with the project"),
            );
        }
        items.push(Item::Separator);
        items.push(
            Item::new("units", "Rounding and more…")
                .icon("units")
                .hint("Saved with this project. You can still type any unit: 3 m, 10'6\", 450 mm."),
        );
        items
    }

    /// Switch the project to a unit, keeping the rounding when it stays in
    /// the same kind (decimal or fractional).
    pub(super) fn set_unit(&mut self, u: LengthUnit) {
        let before = self.doc.model().units();
        if before.length == u {
            return;
        }
        let units = Units::of(u);
        let tx = creabim_core::units::set_units(self.doc.model(), units);
        let r = self.doc.commit(tx);
        crate::units::set(units);
        self.report(
            r,
            format!(
                "Units: {}. Typed values with a unit (3 m, 10'6\") still work.",
                unit_row(u).0.to_lowercase()
            ),
        );
    }

    /// The application menu's actions. Returns false for any other.
    pub(super) fn app_action(&mut self, ctx: &egui::Context, head: &str, arg: &str) -> bool {
        match head {
            "app.about" => self.open_help("about"),
            "app.updates" => {
                ctx.open_url(egui::OpenUrl::new_tab(RELEASES));
                self.status = format!(
                    "This is CreaBIM {}. The releases page is open in your browser; nothing was downloaded.",
                    crate::help::version_line().trim_start_matches("Version ")
                );
            }
            "app.settings" => self.settings_open = true,
            "app.exit" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            "units.set" => {
                if let Some(u) = arg.parse::<usize>().ok().and_then(|i| UNIT_ORDER.get(i)) {
                    self.set_unit(*u);
                }
            }
            _ => return false,
        }
        true
    }

    /// Settings: what each person sets for themselves (the theme), and the
    /// project's units.
    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = true;
        let mut theme = self.shell.theme;
        let mut unit = self.doc.model().units().length;
        let mut keys = false;
        let area = self.mat.area.unwrap_or_else(|| ctx.content_rect());
        egui::Window::new("Settings")
            .open(&mut open)
            .default_pos(area.center() - Vec2::new(180.0, 140.0))
            .constrain_to(area)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.set_width(360.0);
                crate::props::section(ui, "Appearance (yours)");
                crate::props::row(ui, "Theme", |ui| {
                    egui::ComboBox::from_id_salt("settings.theme")
                        .selected_text(theme.label())
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for t in crate::theme::Theme::ALL {
                                ui.selectable_value(&mut theme, t, t.label());
                            }
                        });
                });
                crate::props::section(ui, "This project");
                crate::props::row(ui, "Units", |ui| {
                    egui::ComboBox::from_id_salt("settings.units")
                        .selected_text(unit_row(unit).0)
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for u in UNIT_ORDER {
                                ui.selectable_value(&mut unit, u, unit_row(u).0);
                            }
                        });
                });
                ui.add_space(8.0);
                keys = ui.button("Keyboard shortcuts…").clicked();
            });
        self.settings_open = open;
        if theme != self.shell.theme {
            self.set_theme(ctx, theme);
        }
        if unit != self.doc.model().units().length {
            self.set_unit(unit);
        }
        if keys {
            self.open_help("shortcuts");
        }
    }
}

impl CreaBimApp {
    /// New project with the classic panels: the same choices as the web
    /// window, as a list.
    pub(super) fn new_project_window(&mut self, ctx: &egui::Context) {
        if !self.new_project_classic {
            return;
        }
        let mut open = true;
        let mut pick = None;
        egui::Window::new("New project")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                for s in STARTS.iter() {
                    if ui
                        .button(egui::RichText::new(s.title).strong())
                        .on_hover_text(s.line)
                        .clicked()
                    {
                        pick = Some(s.id);
                    }
                }
                crate::props::section(ui, "Samples to learn from");
                for s in SAMPLES.iter() {
                    if ui.button(s.title).on_hover_text(s.line).clicked() {
                        pick = Some(s.id);
                    }
                }
            });
        self.new_project_classic = open && pick.is_none();
        if let Some(id) = pick {
            self.run_action(ctx, id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn after_an_export_the_status_bar_offers_its_folder() {
        let mut app = test_app();
        let dir = std::env::temp_dir().join(format!("creabim-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        app.path = dir.join("house.creabim").display().to_string();
        app.export("export.ifcx");
        assert!(app.status.starts_with("Exported "), "{}", app.status);
        assert!(dir.join("house.ifcx").exists());
        let has = |app: &CreaBimApp| {
            app.status_state()["controls"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == "export.folder")
        };
        assert!(has(&app));
        assert_eq!(
            app.last_export_dir
                .as_deref()
                .map(|d| d.canonicalize().unwrap()),
            Some(dir.canonicalize().unwrap())
        );
        // once the bar says something else, the button goes
        app.status = "Moved.".into();
        assert!(!has(&app));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_placed_footing_offers_the_foundation_plan_and_makes_one_if_need_be() {
        let mut app = test_app();
        assert!(crate::memberstool::foundation_plan(app.doc.model()).is_none());
        let placed = |app: &mut CreaBimApp| {
            app.plan.footing_placed = true;
            app.status = "Strip footing placed under GF East. Ctrl+Z to undo.".into();
        };
        let offered = |app: &CreaBimApp| {
            app.status_state()["controls"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == "footing.foundation")
                .map(|c| c["label"].as_str().unwrap().to_string())
        };
        // without one, it is made: 600 below the lowest level, the floor
        // above as its underlay, and opened
        placed(&mut app);
        assert_eq!(offered(&app).as_deref(), Some("Make a Foundation plan"));
        app.show_foundation_plan();
        let made = crate::memberstool::foundation_plan(app.doc.model()).unwrap();
        assert_eq!(app.active_view, Some(made));
        let ElementKind::View(v) = &app.doc.model().get(made).unwrap().kind else {
            panic!()
        };
        assert_eq!(v.underlay, creabim_core::Underlay::Above);
        // once there is one, it is shown
        app.active_view = None;
        placed(&mut app);
        assert_eq!(offered(&app).as_deref(), Some("Show in Foundation plan"));
        app.show_foundation_plan();
        assert_eq!(app.active_view, Some(made));
        // and the button goes once the bar says something else
        placed(&mut app);
        app.status = "Moved.".into();
        assert!(offered(&app).is_none());
    }

    #[test]
    fn frame_all_reframes_only_the_pane_it_is_asked_in() {
        let mut app = test_app();
        let (a, b) = (ElementId::new(), ElementId::new());
        app.active_view = Some(a);
        app.rect_2d = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 300.0),
        ));
        app.plan.cameras.insert(a, (glam::DVec2::ZERO, 3.0));
        app.plan.cameras.insert(b, (glam::DVec2::ZERO, 3.0));
        app.camera.distance = 1234.0;
        app.frame_pane(true);
        assert!(!app.plan.cameras.contains_key(&a));
        assert!(app.plan.cameras.contains_key(&b));
        assert_eq!(app.camera.distance, 1234.0);
        app.plan.cameras.insert(a, (glam::DVec2::ZERO, 3.0));
        app.frame_pane(false);
        assert_ne!(app.camera.distance, 1234.0);
        assert!(app.plan.cameras.contains_key(&a));
    }

    #[test]
    fn the_units_switch_changes_the_project_units_in_one_undo() {
        let mut app = test_app();
        assert_eq!(app.doc.model().units().length, LengthUnit::Millimetres);
        let items = app.unit_items();
        let ids: Vec<String> = items
            .iter()
            .filter_map(|i| match i {
                Item::Action { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(ids.len(), 7, "six units and Rounding: {ids:?}");
        let ctx = egui::Context::default();
        app.run_action(&ctx, "units.set:3");
        assert_eq!(app.doc.model().units().length, LengthUnit::FeetInches);
        assert_eq!(crate::units::get().length, LengthUnit::FeetInches);
        app.run_command(&ctx, crate::commands::Command::Undo, None);
        assert_eq!(app.doc.model().units().length, LengthUnit::Millimetres);
    }

    #[test]
    fn the_application_menu_has_about_updates_settings_and_exit() {
        let app = test_app();
        let ids: Vec<String> = app
            .app_menu()
            .into_iter()
            .filter_map(|i| match i {
                Item::Action { id, .. } => Some(id),
                _ => None,
            })
            .collect();
        for want in ["app.about", "app.updates", "app.settings", "app.exit"] {
            assert!(ids.iter().any(|i| i == want), "{want} in {ids:?}");
        }
    }

    #[test]
    fn samples_open_as_new_unsaved_projects() {
        let mut app = test_app();
        app.path = "/work/my house.creabim".into();
        let ctx = egui::Context::default();
        app.run_action(&ctx, "new.garden");
        assert_eq!(app.path, "House in its garden.creabim");
        app.run_action(&ctx, "new.empty");
        assert_eq!(app.path, "Untitled.creabim");
        assert_eq!(app.doc.model().levels().count(), 2);
        // one New project… in the File menu, the samples under Help
        let file: Vec<String> = app
            .menu_items("menu:File")
            .into_iter()
            .filter_map(|i| match i {
                Item::Action { id, .. } if id.starts_with("new.") => Some(id),
                _ => None,
            })
            .collect();
        assert_eq!(file, ["new.project", "new.sheet"]);
        let help = app.help_menu();
        assert!(help.iter().any(|i| matches!(i, Item::Sub { label, items, .. } if label == "Samples to learn from" && items.len() == 6)));
    }

    #[test]
    fn a_file_named_on_the_command_line_is_opened() {
        let args = ["--verbose", "house.creabim"].map(String::from);
        assert_eq!(
            launch_file(args.into_iter()).as_deref(),
            Some("house.creabim")
        );
        assert_eq!(launch_file(std::iter::empty()), None);
        let dir = std::env::temp_dir().join("creabim-launch-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("small.creabim.json");
        let model = sample_house();
        std::fs::write(&path, model.to_json()).unwrap();
        let mut app = CreaBimApp::blank(None);
        app.open_path(path.to_str().unwrap());
        assert_eq!(app.doc.model().len(), model.len());
    }
}
