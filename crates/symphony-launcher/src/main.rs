//! Symphony Launcher: the one thing testers install (ENGINEERING E13.4).
//! It checks GitHub Releases, installs and updates Symphony in the
//! background with checksum checks, keeps the previous version for
//! rollback, and launches the version you choose. It works offline with
//! whatever is already installed.

mod dev;
mod install;
mod release;

use std::process::Child;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use eframe::egui::{self, RichText};
use semver::Version;
use symphony_ui::theme;

use install::{Paths, Progress, State};
use release::{Channel, Release};

enum Msg {
    Releases(Result<Vec<Release>, String>),
    Progress(Progress),
    Installed(Result<Version, String>),
}

enum Job {
    Idle,
    Checking,
    Installing(Option<Progress>),
}

struct Launcher {
    paths: Paths,
    state: State,
    releases: Vec<Release>,
    job: Job,
    message: Option<String>,
    error: Option<String>,
    running: Vec<(Version, Child)>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl Launcher {
    fn new(cc: &eframe::CreationContext<'_>, paths: Paths) -> Self {
        theme::apply(&cc.egui_ctx);
        let (tx, rx) = channel();
        let state = paths.load_state();
        let mut l = Launcher {
            paths,
            state,
            releases: Vec::new(),
            job: Job::Idle,
            message: None,
            error: None,
            running: Vec::new(),
            tx,
            rx,
        };
        l.check(&cc.egui_ctx);
        l
    }

    fn check(&mut self, ctx: &egui::Context) {
        self.job = Job::Checking;
        self.error = None;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Releases(release::fetch_releases()));
            ctx.request_repaint();
        });
    }

    fn start_install(&mut self, ctx: &egui::Context, rel: Release) {
        self.job = Job::Installing(None);
        self.error = None;
        let (tx, ctx, paths) = (self.tx.clone(), ctx.clone(), self.paths.clone());
        std::thread::spawn(move || {
            let report = |p| {
                let _ = tx.send(Msg::Progress(p));
                ctx.request_repaint();
            };
            let result = install::install(&paths, &rel, &report);
            let _ = tx.send(Msg::Installed(result));
            ctx.request_repaint();
        });
    }

    fn receive(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Releases(Ok(r)) => {
                    self.releases = r;
                    self.job = Job::Idle;
                }
                Msg::Releases(Err(e)) => {
                    self.error = Some(e);
                    self.job = Job::Idle;
                }
                Msg::Progress(p) => self.job = Job::Installing(Some(p)),
                Msg::Installed(Ok(v)) => {
                    self.job = Job::Idle;
                    self.message = Some(format!("Symphony {v} is installed."));
                    // Keep the new version and the one before it, for rollback.
                    let keep: Vec<Version> = self.paths.installed().into_iter().take(2).collect();
                    let running: Vec<Version> =
                        self.running.iter().map(|(v, _)| v.clone()).collect();
                    install::prune(&self.paths, &keep, &running);
                    self.state.selected = None;
                    self.save();
                }
                Msg::Installed(Err(e)) => {
                    self.job = Job::Idle;
                    self.error = Some(e);
                }
            }
        }
        self.running
            .retain_mut(|(_, c)| matches!(c.try_wait(), Ok(None)));
    }

    fn save(&mut self) {
        if let Err(e) = self.paths.save_state(&self.state) {
            self.error = Some(format!("Couldn't save launcher settings ({e})."));
        }
    }

    fn selected(&self, installed: &[Version]) -> Option<Version> {
        let chosen = self
            .state
            .selected
            .as_deref()
            .and_then(|s| Version::parse(s).ok());
        chosen
            .filter(|v| installed.contains(v))
            .or_else(|| installed.first().cloned())
    }

    fn busy(&self) -> bool {
        !matches!(self.job, Job::Idle)
    }
}

/// Primary action button: white on cel-blue fill (DESIGN §4.1).
fn primary(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    let b = egui::Button::new(RichText::new(text).color(theme::ON_ACCENT))
        .fill(theme::ACCENT_SELECT_FILL);
    ui.add_enabled(enabled, b.min_size(egui::vec2(0.0, 28.0)))
        .clicked()
}

/// Errors: a red marker plus plain text (D4.1 red only as a marker, D10.3).
fn error_line(ui: &mut egui::Ui, text: &str) {
    ui.horizontal_wrapped(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(r.center(), 4.0, theme::STATE_DANGER);
        ui.label(text);
    });
}

fn open_folder(path: &std::path::Path) {
    let _ = std::fs::create_dir_all(path);
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd).arg(path).spawn();
}

impl eframe::App for Launcher {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.receive();
        if !self.running.is_empty() {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        let installed = self.paths.installed();
        let selected = self.selected(&installed);
        let newest = release::newest(&self.releases, self.state.channel).cloned();
        let newest_v = newest.as_ref().and_then(Release::version);
        let update = newest_v
            .as_ref()
            .filter(|nv| installed.first().is_none_or(|iv| *nv > iv))
            .cloned();

        let frame = egui::Frame::new()
            .fill(theme::BG_PANEL)
            .inner_margin(egui::Margin::same(16));
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            ui.heading("Symphony");
            ui.colored_label(
                theme::TEXT_SECONDARY,
                "Free 2D animation, familiar to Harmony animators.",
            );
            ui.add_space(12.0);

            // Launch.
            if let Some(v) = &selected {
                let is_running = self.running.iter().any(|(r, _)| r == v);
                let label = if is_running {
                    format!("Symphony {v} is running")
                } else {
                    format!("Launch Symphony {v}")
                };
                if primary(ui, &label, !is_running) {
                    match install::launch(&self.paths, v) {
                        Ok(child) => self.running.push((v.clone(), child)),
                        Err(e) => self.error = Some(e),
                    }
                }
                if installed.len() > 1 {
                    ui.add_space(8.0);
                    ui.colored_label(theme::TEXT_SECONDARY, "Installed versions");
                    for iv in &installed {
                        let mut label = iv.to_string();
                        if Some(iv) == installed.first() {
                            label.push_str(" (newest)");
                        }
                        if ui.radio(Some(iv) == selected.as_ref(), label).clicked() {
                            self.state.selected = Some(iv.to_string());
                            self.save();
                        }
                    }
                }
            } else {
                ui.label("Symphony isn't installed yet.");
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(4.0);

            // Updates.
            ui.horizontal(|ui| {
                ui.label("Updates");
                let mut ch = self.state.channel;
                egui::ComboBox::from_id_salt("channel")
                    .selected_text(ch.title())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut ch, Channel::Stable, "Stable");
                        ui.selectable_value(&mut ch, Channel::Preview, "Preview (for testers)");
                    });
                if ch != self.state.channel {
                    self.state.channel = ch;
                    self.save();
                }
            });

            match &self.job {
                Job::Checking => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Checking for updates");
                    });
                }
                Job::Installing(p) => {
                    let (frac, text) = match p {
                        Some(Progress::Downloading { done, total }) if *total > 0 => (
                            *done as f32 / *total as f32,
                            format!(
                                "Downloading {:.0} of {:.0} MB",
                                *done as f64 / 1e6,
                                *total as f64 / 1e6
                            ),
                        ),
                        Some(Progress::Verifying) => (1.0, "Checking the download".to_owned()),
                        Some(Progress::Installing) => (1.0, "Installing".to_owned()),
                        _ => (0.0, "Starting download".to_owned()),
                    };
                    ui.add(egui::ProgressBar::new(frac).text(text));
                }
                Job::Idle => match (&update, &newest) {
                    (Some(v), Some(rel)) => {
                        ui.label(format!("Symphony {v} is available."));
                        if let Some(notes) = rel.body.as_deref().filter(|b| !b.trim().is_empty()) {
                            egui::ScrollArea::vertical()
                                .max_height(160.0)
                                .show(ui, |ui| {
                                    ui.colored_label(theme::TEXT_SECONDARY, notes);
                                });
                        }
                        let text = if installed.is_empty() {
                            format!("Install Symphony {v}")
                        } else {
                            format!("Update to {v}")
                        };
                        if primary(ui, &text, true) {
                            self.start_install(&ctx, rel.clone());
                        }
                    }
                    _ if self.error.is_some() => {}
                    _ if !installed.is_empty() => {
                        ui.colored_label(theme::TEXT_SECONDARY, "You have the newest version.");
                    }
                    // Nothing installed and nothing on Stable yet: point testers
                    // at Preview instead of a dead end (DESIGN §17).
                    _ if self.state.channel == Channel::Stable
                        && release::newest(&self.releases, Channel::Preview).is_some() =>
                    {
                        ui.colored_label(
                            theme::TEXT_SECONDARY,
                            "There's no stable release yet. Testers can install the preview.",
                        );
                        if primary(ui, "Switch to Preview (for testers)", true) {
                            self.state.channel = Channel::Preview;
                            self.save();
                        }
                    }
                    _ => {
                        ui.colored_label(theme::TEXT_SECONDARY, "No releases on this channel yet.");
                    }
                },
            }

            if let Some(m) = &self.message {
                ui.colored_label(theme::TEXT_SECONDARY, m.as_str());
            }
            if let Some(e) = self.error.clone() {
                error_line(ui, &e);
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.busy(), egui::Button::new("Check again"))
                    .clicked()
                {
                    self.message = None;
                    self.check(&ctx);
                }
                if ui.button("Open install folder").clicked() {
                    open_folder(&self.paths.root);
                }
            });
        });
    }
}

fn main() -> eframe::Result {
    // Developer mode, used by Symphony Dev.app: build from source, then launch.
    let args: Vec<String> = std::env::args().collect();
    // Smoke tests without the window: --check [stable|preview], --install [stable|preview].
    if let Some(cmd @ ("--check" | "--install")) = args.get(1).map(String::as_str) {
        std::process::exit(cli(cmd, args.get(2).map(String::as_str)));
    }
    if args.get(1).map(String::as_str) == Some("--dev") {
        let repo = args
            .get(2)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        return dev::run(repo);
    }
    let Some(paths) = Paths::default_location() else {
        eprintln!("Couldn't find a folder to install Symphony into.");
        std::process::exit(1);
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Symphony Launcher")
            .with_inner_size([520.0, 520.0])
            .with_min_inner_size([420.0, 360.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Symphony Launcher",
        options,
        Box::new(|cc| Ok(Box::new(Launcher::new(cc, paths)))),
    )
}

/// Command-line checks used to test releases end to end.
fn cli(cmd: &str, channel: Option<&str>) -> i32 {
    let channel = if channel == Some("preview") {
        Channel::Preview
    } else {
        Channel::Stable
    };
    let Some(paths) = Paths::default_location() else {
        eprintln!("No data folder.");
        return 1;
    };
    let releases = match release::fetch_releases() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let Some(rel) = release::newest(&releases, channel) else {
        println!(
            "No {} release with a build for this computer.",
            channel.title()
        );
        return 2;
    };
    println!(
        "Newest {}: {} ({})",
        channel.title(),
        rel.tag_name,
        rel.archive().map_or("no build", |a| a.name.as_str())
    );
    if cmd == "--install" {
        match install::install(&paths, rel, &|p| {
            if let Progress::Downloading { done, total } = p {
                eprint!(
                    "\rDownloading {:.1} of {:.1} MB",
                    done as f64 / 1e6,
                    total as f64 / 1e6
                );
            }
        }) {
            Ok(v) => println!("\nInstalled {v} at {}", paths.version_dir(&v).display()),
            Err(e) => {
                eprintln!("\n{e}");
                return 1;
            }
        }
    }
    0
}
