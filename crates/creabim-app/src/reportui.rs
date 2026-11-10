//! The Log view and Report a problem (MU40): what testers send when
//! something goes wrong, so it can be rebuilt and fixed.
//!
//! The Log is a view kind like Notes: the activity log as it is written,
//! problems in red, a filter, copy and save. Report a problem writes one
//! zip the tester attaches to an issue or an email: a summary (version,
//! build, system, what is open, the last few hundred lines), the whole
//! activity log, the settings and, only if they say yes, the project as
//! it is now. Nothing is sent anywhere by the app.

use super::*;

/// The Log view between frames.
#[derive(Clone, Debug, Default)]
pub struct LogUi {
    pub filter: String,
    pub problems_only: bool,
}

/// A zip of `files` (name, bytes), stored without compression: plain to
/// read in any unzip tool.
pub fn zip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out: Vec<u8> = vec![];
    let mut central: Vec<u8> = vec![];
    let le16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    let le32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    for (name, data) in files {
        let crc = creabim_geom::raster::crc32(data);
        let offset = out.len() as u32;
        let n = name.as_bytes();
        // local header
        le32(&mut out, 0x0403_4b50);
        le16(&mut out, 20);
        le16(&mut out, 0x0800); // names in UTF-8
        le16(&mut out, 0); // stored
        le16(&mut out, 0);
        le16(&mut out, 0x21);
        le32(&mut out, crc);
        le32(&mut out, data.len() as u32);
        le32(&mut out, data.len() as u32);
        le16(&mut out, n.len() as u16);
        le16(&mut out, 0);
        out.extend_from_slice(n);
        out.extend_from_slice(data);
        // its central directory entry
        le32(&mut central, 0x0201_4b50);
        le16(&mut central, 20);
        le16(&mut central, 20);
        le16(&mut central, 0x0800);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le16(&mut central, 0x21);
        le32(&mut central, crc);
        le32(&mut central, data.len() as u32);
        le32(&mut central, data.len() as u32);
        le16(&mut central, n.len() as u16);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le32(&mut central, 0);
        le32(&mut central, offset);
        central.extend_from_slice(n);
    }
    let at = out.len() as u32;
    out.extend_from_slice(&central);
    le32(&mut out, 0x0605_4b50);
    le16(&mut out, 0);
    le16(&mut out, 0);
    le16(&mut out, files.len() as u16);
    le16(&mut out, files.len() as u16);
    le32(&mut out, central.len() as u32);
    le32(&mut out, at);
    le16(&mut out, 0);
    out
}

impl CreaBimApp {
    /// The Log view: the activity log as it is written.
    pub(super) fn log_area(&mut self, ui: &mut egui::Ui, body: Rect) {
        let pal = self.shell.theme.palette();
        ui.painter().rect_filled(body, 0.0, pal.panel2);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(body.shrink2(egui::vec2(12.0, 8.0)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ui = &mut child;
        let (lines, _) = crate::actlog::recent();
        let mut copy = false;
        let mut report = false;
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.log_ui.filter)
                    .hint_text("Filter the log")
                    .desired_width(220.0),
            );
            ui.checkbox(&mut self.log_ui.problems_only, "Problems only")
                .on_hover_text("Only crashes, errors and what could not be done");
            copy = ui
                .button("Copy")
                .on_hover_text("Copy what is shown, to paste in a message")
                .clicked();
            report = ui
                .button("Report a problem…")
                .on_hover_text("Save one file to attach to a bug report: this log, your settings and, if you like, the project")
                .clicked();
            if ui
                .button("Open the folder")
                .on_hover_text("Where the whole activity log is kept")
                .clicked()
            {
                self.run_action(ui.ctx(), "app.log");
            }
        });
        ui.add_space(4.0);
        let f = self.log_ui.filter.to_lowercase();
        let shown: Vec<&String> = lines
            .iter()
            .filter(|l| !self.log_ui.problems_only || crate::actlog::is_problem(l))
            .filter(|l| f.is_empty() || l.to_lowercase().contains(&f))
            .collect();
        if copy {
            let text: Vec<&str> = shown.iter().map(|s| s.as_str()).collect();
            ui.ctx().copy_text(text.join("\n"));
            self.status = format!("{} log lines copied.", shown.len());
        }
        let red = egui::Color32::from_rgb(214, 72, 72);
        egui::ScrollArea::both()
            .auto_shrink(false)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if shown.is_empty() {
                    ui.label(egui::RichText::new("Nothing yet.").color(pal.muted));
                }
                for l in &shown {
                    let ink = if crate::actlog::is_problem(l) {
                        red
                    } else {
                        pal.text
                    };
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(l.as_str())
                                .monospace()
                                .size(11.5)
                                .color(ink),
                        )
                        .extend(),
                    );
                }
            });
        if report {
            self.report_problem();
        }
    }

    /// Help › Report a problem: one zip to attach to a bug report.
    pub(super) fn report_problem(&mut self) {
        let with_project = rfd::MessageDialog::new()
            .set_title("Report a problem")
            .set_description(
                "CreaBIM saves one file to attach to a bug report: what it did lately, your settings and your system. Nothing is sent anywhere.\n\nInclude the project as it is now? It helps rebuild the problem exactly, but it is your work: say no if it is private.",
            )
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .show();
        let with_project = match with_project {
            rfd::MessageDialogResult::Yes => true,
            rfd::MessageDialogResult::No => false,
            _ => return,
        };
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let Some(to) = rfd::FileDialog::new()
            .set_title("Save the problem report")
            .add_filter("Zip", &["zip"])
            .set_file_name(format!("CreaBIM-report-{secs}.zip"))
            .save_file()
        else {
            return;
        };
        let bytes = zip(&self.report_files(with_project));
        self.status = match std::fs::write(&to, bytes) {
            Ok(()) => {
                let _ = crate::files::open_folder(&to);
                format!(
                    "Report saved: {}. Attach it to a GitHub issue or an email{}.",
                    to.display(),
                    if with_project {
                        "; it holds your project"
                    } else {
                        ""
                    }
                )
            }
            Err(e) => format!("Couldn't save the report: {e}"),
        };
    }

    /// The report's files: a summary, the activity log, the settings and,
    /// if asked, the project.
    fn report_files(&self, with_project: bool) -> Vec<(String, Vec<u8>)> {
        let model = self.doc.model();
        let (lines, _) = crate::actlog::recent();
        let tail: Vec<&String> = lines.iter().rev().take(400).collect();
        let problems: Vec<&String> = lines
            .iter()
            .filter(|l| crate::actlog::is_problem(l))
            .collect();
        let areas: Vec<String> = self
            .areas
            .root
            .areas()
            .iter()
            .map(|a| format!("{a:?}"))
            .collect();
        let mut summary = String::new();
        let mut put = |k: &str, v: String| summary.push_str(&format!("{k}: {v}\n"));
        put("CreaBIM", env!("CARGO_PKG_VERSION").to_string());
        put("Build", env!("CREABIM_COMMIT").to_string());
        put(
            "System",
            format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        );
        put("Theme", self.shell.theme.id().to_string());
        put("Tool", self.plan.tool.label().to_string());
        put("Elements", model.iter().count().to_string());
        put("Undo steps", self.doc.can_undo().to_string());
        put(
            "Last edit",
            self.doc.undo_label().unwrap_or("none").to_string(),
        );
        put("Areas", areas.join(", "));
        put("Status", self.status.clone());
        put("Project included", with_project.to_string());
        summary.push_str("\n--- Problems in this run ---\n");
        for p in &problems {
            summary.push_str(p);
            summary.push('\n');
        }
        summary.push_str("\n--- The last 400 lines, newest last ---\n");
        for l in tail.iter().rev() {
            summary.push_str(l);
            summary.push('\n');
        }
        let mut files = vec![("summary.txt".to_string(), summary.into_bytes())];
        if let Some(log) = crate::actlog::path().and_then(|p| std::fs::read(p).ok()) {
            files.push(("activity.log".into(), log));
        }
        if let Some(old) = crate::actlog::path()
            .map(|p| p.with_file_name("activity.old.log"))
            .and_then(|p| std::fs::read(p).ok())
        {
            files.push(("activity.old.log".into(), old));
        }
        if let Some(s) =
            crate::files::settings_dir().and_then(|d| std::fs::read(d.join("settings.json")).ok())
        {
            files.push(("settings.json".into(), s));
        }
        if with_project {
            // as it is now, unsaved changes and all
            files.push(("project.json".into(), model.to_json().into_bytes()));
        }
        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_zip_has_its_files_whole() {
        let files = vec![
            ("summary.txt".to_string(), b"CreaBIM: 1.1.0".to_vec()),
            ("activity.log".to_string(), vec![7u8; 1000]),
        ];
        let z = zip(&files);
        // its end record counts both, and each file sits whole after its
        // header, with its checksum
        let end = z.len() - 22;
        assert_eq!(&z[end..end + 4], &[0x50, 0x4b, 0x05, 0x06]);
        assert_eq!(u16::from_le_bytes([z[end + 10], z[end + 11]]), 2);
        let i = z
            .windows(11)
            .position(|w| w == b"summary.txt")
            .expect("its name");
        assert_eq!(&z[i + 11..i + 11 + 14], b"CreaBIM: 1.1.0");
        let crc = u32::from_le_bytes([z[14], z[15], z[16], z[17]]);
        assert_eq!(crc, creabim_geom::raster::crc32(b"CreaBIM: 1.1.0"));
    }
}
