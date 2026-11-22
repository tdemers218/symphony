use std::path::{Path, PathBuf};

use eframe::egui;
use symphony_ui::{theme, widgets};

pub enum Action {
    Save { path: PathBuf, create: bool },
    Cancel,
}

pub struct SaveDialog {
    name: String,
    folder: Option<PathBuf>,
    replace: Option<PathBuf>,
    error: Option<String>,
}

impl SaveDialog {
    pub fn new(name: String, current: Option<&Path>) -> Self {
        Self {
            name,
            folder: current
                .and_then(Path::parent)
                .map(Path::to_owned)
                .or_else(dirs::document_dir),
            replace: None,
            error: None,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, current: Option<&Path>) -> Option<Action> {
        let response = theme::glass_modal(ctx, "save-project").show(ctx, |ui| {
            ui.set_width(390.0);
            if let Some(path) = self.replace.clone() {
                ui.heading("Replace existing project?");
                ui.label(format!("Save over {}?", symphony_io::project_name(&path)));
                ui.add_space(8.0);
                return ui
                    .horizontal(|ui| {
                        if widgets::primary_button(ui, "Replace project", true).clicked() {
                            Some(Action::Save {
                                path,
                                create: false,
                            })
                        } else if ui.button("Back").clicked() {
                            self.replace = None;
                            None
                        } else {
                            None
                        }
                    })
                    .inner;
            }

            ui.heading("Save scene");
            ui.add_space(8.0);
            egui::Grid::new("save-scene-fields")
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Name");
                    ui.add_sized([250.0, 24.0], egui::TextEdit::singleline(&mut self.name));
                    ui.end_row();
                    ui.label("Save location");
                    ui.horizontal(|ui| {
                        if ui.button("Choose folder…").clicked() {
                            let mut dialog =
                                rfd::FileDialog::new().set_title("Choose where to save the scene");
                            if let Some(folder) = &self.folder {
                                dialog = dialog.set_directory(folder);
                            }
                            if let Some(folder) = dialog.pick_folder() {
                                self.folder = Some(folder);
                            }
                        }
                        if let Some(folder) = &self.folder {
                            let label = folder
                                .file_name()
                                .unwrap_or(folder.as_os_str())
                                .to_string_lossy();
                            ui.label(label).on_hover_text(folder.display().to_string());
                        }
                    });
                    ui.end_row();
                });
            if let Some(error) = &self.error {
                ui.label(error);
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, "Save scene", self.folder.is_some()).clicked() {
                    self.destination(current)
                } else if ui.button("Cancel").clicked() {
                    Some(Action::Cancel)
                } else {
                    None
                }
            })
            .inner
        });
        if response.should_close() {
            Some(Action::Cancel)
        } else {
            response.inner
        }
    }

    fn destination(&mut self, current: Option<&Path>) -> Option<Action> {
        let folder = self.folder.as_deref()?;
        let name = self.name.trim();
        if name.is_empty()
            || name.starts_with('.')
            || name.ends_with('.')
            || name
                .chars()
                .any(|ch| ch.is_control() || "/\\:*?\"<>|".contains(ch))
        {
            self.error = Some("Enter a scene name without slashes or filename punctuation.".into());
            return None;
        }
        let path = symphony_io::with_extension(&folder.join(name));
        if current == Some(path.as_path()) {
            return Some(Action::Save {
                path,
                create: false,
            });
        }
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && path.join("project.json").is_file() => {
                self.replace = Some(path);
                None
            }
            Ok(_) => {
                self.error =
                    Some("A file or folder already has that name. Choose a different name.".into());
                None
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Some(Action::Save { path, create: true })
            }
            Err(e) => {
                self.error = Some(format!("Couldn’t check the project folder ({e})."));
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_confirmation_only_for_an_existing_project() {
        let temp = tempfile::tempdir().unwrap();
        let mut dialog = SaveDialog::new("Walk".into(), None);
        dialog.folder = Some(temp.path().to_owned());
        let path = temp.path().join("Walk.symphony");
        assert!(matches!(
            dialog.destination(None),
            Some(Action::Save { path: candidate, create: true }) if candidate == path
        ));
        std::fs::create_dir(&path).unwrap();
        assert!(dialog.destination(None).is_none());
        assert!(dialog.replace.is_none());
        std::fs::write(path.join("project.json"), b"{}").unwrap();
        assert!(dialog.destination(None).is_none());
        assert_eq!(dialog.replace.as_deref(), Some(path.as_path()));
    }
}
