//! Native macOS picker. The OS runs the sampling interaction and supplies
//! just the selected colour.

use block2::RcBlock;
use eframe::egui;
use objc2_app_kit::{NSColor, NSColorSampler, NSColorSpace};

type Pick = (String, [u8; 3]);

fn id() -> egui::Id {
    egui::Id::unique("native-screen-colour")
}

pub fn pick(ctx: &egui::Context, scene: String) {
    let ctx = ctx.clone();
    let handler = RcBlock::new(move |colour: *mut NSColor| {
        // The native callback owns the lifetime of the borrowed colour.
        let Some(colour) = (unsafe { colour.as_ref() }) else {
            return;
        };
        let Some(srgb) = colour.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) else {
            return;
        };
        let byte = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        let rgb = [
            srgb.redComponent(),
            srgb.greenComponent(),
            srgb.blueComponent(),
        ]
        .map(byte);
        ctx.data_mut(|data| data.insert_temp(id(), (scene.clone(), rgb)));
        ctx.request_repaint();
    });
    // Called from the AppKit UI thread. The sampler keeps the copied
    // callback and itself alive until the user picks a colour or cancels.
    unsafe { NSColorSampler::new().showSamplerWithSelectionHandler(&handler) };
}

pub fn take(ctx: &egui::Context, scene: &str) -> Option<[u8; 3]> {
    ctx.data_mut(|data| data.remove_temp::<Pick>(id()))
        .filter(|(source, _)| source == scene)
        .map(|(_, rgb)| rgb)
}
