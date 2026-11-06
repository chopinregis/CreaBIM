//! The CreaBIM application menu (About, Check for updates, Settings,
//! Exit), the Units switch in the status bar, the Settings window and the
//! New project window's choices.

use super::*;
use crate::ui_model::Item;
use creabim_core::{LengthUnit, Units};

/// Where a manual update check looks: the project's releases page. Nothing
/// is fetched in the background; the page opens in the browser when asked.
pub(crate) const RELEASES: &str = "https://github.com/chopinregis/CreaBIM/releases";

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
            Item::new("app.log", "Open the activity log")
                .icon("file")
                .hint("What CreaBIM did, step by step (drags frame by frame, edits, tools): attach it to a bug report. It stays on this computer."),
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
            "app.log" => {
                self.status = match crate::actlog::path() {
                    Some(p) => match crate::files::open_folder(&p) {
                        Ok(()) => format!("The activity log is {}.", p.display()),
                        Err(e) => e,
                    },
                    None => "There is no activity log on this computer.".into(),
                };
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
        let mut grips = self.shell.settings.grips;
        let mut spacing = self.plan.snap.step_um;
        let mut marks = self.plan.snap.marks;
        let mut paper = self.shell.settings.drawing_paper.clone();
        let mut navigation = self.shell.settings.navigation.clone();
        let mut grip_hover = None;
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
                ui.label("Grips");
                for g in crate::handles::GripStyle::ALL {
                    let note = match g {
                        crate::handles::GripStyle::Quiet => "Default. Squares at corners, bars on edges",
                        crate::handles::GripStyle::Edges => "Almost nothing at rest; edges drag",
                        crate::handles::GripStyle::OneShape => "The same small circle everywhere",
                    };
                    let r = crate::choices::choice_row(ui, grips == g, g.label(), note, |p, r| {
                        crate::choices::paint_grips(p, r, g)
                    });
                    if r.clicked() {
                        grips = g;
                    } else if r.hovered() {
                        grip_hover = Some(g);
                    }
                }
                ui.label(
                    egui::RichText::new(
                        "Yours alone: it never changes anyone's drawings, and goes with you, not with the project.",
                    )
                    .weak(),
                );
                crate::props::row(ui, "Drawing paper", |ui| {
                    let label = |k: &str| match k {
                        "light" => "Always light",
                        "dark" => "Always dark",
                        _ => "Follow the theme",
                    };
                    egui::ComboBox::from_id_salt("settings.paper")
                        .selected_text(label(&paper))
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for k in ["theme", "light", "dark"] {
                                ui.selectable_value(&mut paper, k.to_string(), label(k));
                            }
                        })
                    .response
                    .on_hover_text("Plans, sections and the 3D view on light or dark paper. Prints and PDFs are always on white.");
                });
                crate::props::row(ui, "Mouse", |ui| {
                    let label = |k: &str| match k {
                        "classic" => "Classic: left drag orbits",
                        _ => "CreaBIM: middle orbits, left boxes",
                    };
                    egui::ComboBox::from_id_salt("settings.navigation")
                        .selected_text(label(&navigation))
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for k in ["creabim", "classic"] {
                                ui.selectable_value(&mut navigation, k.to_string(), label(k));
                            }
                        })
                        .response
                        .on_hover_text("CreaBIM (recommended): left click selects, a left drag on empty space draws a box, a middle drag orbits (Alt + left drag without one), a right drag or Shift + middle pans, a right click opens the menu, the wheel zooms. Classic: a left drag orbits in 3D.");
                });
                crate::props::section(ui, "Snapping (yours)");
                crate::props::row(ui, "Spacing", |ui| {
                    let steps: &[u32] = if crate::units::get().is_imperial() {
                        &crate::snaps::STEPS_IMPERIAL
                    } else {
                        &crate::snaps::STEPS_METRIC
                    };
                    egui::ComboBox::from_id_salt("settings.spacing")
                        .selected_text(crate::units::len(spacing as f64 / 1e6))
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for um in steps {
                                ui.selectable_value(
                                    &mut spacing,
                                    *um,
                                    crate::units::len(*um as f64 / 1e6),
                                );
                            }
                        });
                });
                ui.checkbox(&mut marks, "Distance marks while dragging")
                    .on_hover_text("Faint marks every spacing from where you grabbed; a mark holds the drag only when the pointer is near it.");
                ui.label(
                    egui::RichText::new(
                        "Drags and points are free. Grid snap (Alt+S, or the status bar) lands them on a grid of this spacing; it starts off each time.",
                    )
                    .weak(),
                );
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
        // resting on a grip look shows it at once; leaving puts yours back
        crate::handles::set_grip_style(grip_hover.filter(|_| open).unwrap_or(grips));
        if navigation != self.shell.settings.navigation {
            self.shell.settings.navigation = navigation;
            self.shell.settings.store();
        }
        if paper != self.shell.settings.drawing_paper {
            self.shell.settings.drawing_paper = paper;
            self.shell.settings.store();
        }
        if grips != self.shell.settings.grips {
            self.shell.settings.grips = grips;
            self.shell.settings.store();
            self.status = format!("Grips: {}.", grips.label());
        }
        if spacing != self.plan.snap.step_um || marks != self.plan.snap.marks {
            self.plan.snap.step_um = spacing;
            self.plan.snap.marks = marks;
            self.shell.settings.snap_spacing_um = spacing;
            self.shell.settings.distance_marks = marks;
            self.shell.settings.store();
        }
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

impl CreaBimApp {
    /// Project information (Manage › Project information, A7-5): a copy is
    /// edited, and Apply makes it one undo step.
    pub(super) fn project_window(&mut self, ctx: &egui::Context) {
        if !self.project_open {
            self.project_draft = None;
            return;
        }
        let current = self
            .doc
            .model()
            .project_info()
            .map(|(_, p)| p.clone())
            .unwrap_or_default();
        let mut draft = self.project_draft.take().unwrap_or_else(|| current.clone());
        let mut open = true;
        let mut apply = false;
        let mut close = false;
        let area = self.mat.area.unwrap_or_else(|| ctx.content_rect());
        egui::Window::new("Project information")
            .open(&mut open)
            .default_pos(area.center() - Vec2::new(200.0, 220.0))
            .constrain_to(area)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.set_width(400.0);
                let text = |ui: &mut egui::Ui, label: &str, v: &mut String, hint: &str| {
                    crate::props::row(ui, label, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(v)
                                .hint_text(hint)
                                .desired_width(ui.available_width()),
                        );
                    });
                };
                crate::props::section(ui, "Project");
                text(ui, "Name", &mut draft.name, "Maple Lane House");
                text(ui, "Number", &mut draft.number, "Job number");
                text(ui, "Client", &mut draft.client, "");
                text(ui, "Status", &mut draft.status, "Planning application");
                text(ui, "Issue date", &mut draft.issue_date, "");
                crate::props::section(ui, "Site");
                text(ui, "Site name", &mut draft.site_name, "");
                text(ui, "Address", &mut draft.address, "");
                let degrees = |ui: &mut egui::Ui, label: &str, v: &mut Option<f64>, range: f64| {
                    crate::props::row(ui, label, |ui| {
                        let mut on = v.is_some();
                        if ui.checkbox(&mut on, "").changed() {
                            *v = on.then_some(0.0);
                        }
                        if let Some(d) = v {
                            ui.add(
                                egui::DragValue::new(d)
                                    .range(-range..=range)
                                    .speed(0.0001)
                                    .max_decimals(6)
                                    .suffix("°"),
                            );
                        }
                    });
                };
                degrees(ui, "Latitude", &mut draft.latitude, 90.0);
                degrees(ui, "Longitude", &mut draft.longitude, 180.0);
                crate::props::row(ui, "Elevation", |ui| {
                    ui.add(
                        egui::DragValue::new(&mut draft.elevation)
                            .speed(0.01)
                            .suffix(" m"),
                    )
                    .on_hover_text("The project's zero above sea level");
                });
                crate::props::row(ui, "True north", |ui| {
                    ui.add(
                        egui::DragValue::new(&mut draft.true_north)
                            .range(-180.0..=180.0)
                            .speed(0.5)
                            .suffix("°"),
                    )
                    .on_hover_text("How far true north turns from the drawings' up, anticlockwise");
                });
                crate::props::section(ui, "Building");
                text(ui, "Name", &mut draft.building_name, "");
                text(ui, "Use", &mut draft.building_use, "Dwelling");
                ui.label(
                    egui::RichText::new(
                        "These fill the title blocks of every sheet (a value a sheet sets itself still wins) and name the project, site and building in IFC exports.",
                    )
                    .weak(),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    apply = ui
                        .add_enabled(draft != current, egui::Button::new("Apply"))
                        .clicked();
                    close = ui.button("Close").clicked();
                });
            });
        if apply {
            let tx = creabim_core::set_project(self.doc.model(), draft.clone());
            let r = self.doc.commit(tx);
            self.report(
                r,
                "Project information applied: the title blocks show it now.".to_string(),
            );
        }
        self.project_open = open && !close;
        self.project_draft = self.project_open.then_some(draft);
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
