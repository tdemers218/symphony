//! Screenshot mode for design reviews (ENGINEERING E11.5). With
//! `SYMPHONY_SCREENSHOT=out.png`, the app renders a few frames, saves a PNG
//! of its own window and quits. It captures only Symphony and needs no OS
//! screen-recording permission. `SYMPHONY_WORKSPACE` picks the workspace.
//! See scripts/screenshot.sh.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eframe::egui::{self, ColorImage};

pub struct Shot {
    path: String,
    frames: u32,
    requested: bool,
    done: Arc<AtomicBool>,
}

impl Shot {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var("SYMPHONY_SCREENSHOT").ok()?;
        Some(Shot {
            path,
            frames: 0,
            requested: false,
            done: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Call once per frame.
    pub fn tick(&mut self, ctx: &egui::Context) {
        self.frames += 1;
        if self.done.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        ctx.request_repaint();
        // SYMPHONY_SHOT_SCALE=2: render at twice the size, for sharp images
        // on high-density screens (pair it with a doubled SHOT_SIZE).
        // Always set, so one run's scale never carries into the next
        // through the throwaway state folder.
        if self.frames == 1 {
            let z = std::env::var("SYMPHONY_SHOT_SCALE")
                .ok()
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(1.0);
            ctx.set_zoom_factor(z.clamp(0.5, 4.0));
        }
        // Review sizes (SYMPHONY_SHOT_SIZE=1280x800): resize once the window
        // exists, so a maximised or remembered window can't override it.
        if self.frames == 2 {
            if let Some((w, h)) = std::env::var("SYMPHONY_SHOT_SIZE").ok().and_then(|v| {
                let (w, h) = v.split_once('x')?;
                Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
            }) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
            }
        }
        // A few frames and half a second first, so fonts and layout have
        // settled and opening fades (windows, panels) have finished.
        let settled = ctx.input(|i| i.time) >= 0.5;
        if self.frames >= 20 && settled && !self.requested {
            self.requested = true;
            let (path, done, ctx2) = (self.path.clone(), self.done.clone(), ctx.clone());
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                egui::ScreenshotCallback::new(move |img: Arc<ColorImage>| {
                    let [w, h] = img.size;
                    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                    match image::RgbaImage::from_raw(w as u32, h as u32, bytes)
                        .map(|i| i.save(&path))
                    {
                        Some(Ok(())) => println!("Saved {path} ({w}x{h})"),
                        Some(Err(e)) => eprintln!("Couldn't save {path}: {e}"),
                        None => eprintln!("Screenshot had the wrong size"),
                    }
                    done.store(true, Ordering::SeqCst);
                    ctx2.request_repaint();
                }),
            ));
        }
    }
}
