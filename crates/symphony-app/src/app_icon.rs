const ICON_PNG: &[u8] = include_bytes!("../assets/Symphony.png");

pub fn window_icon() -> eframe::egui::IconData {
    let icon = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png)
        .expect("embedded Symphony icon is a valid PNG")
        .into_rgba8();
    eframe::egui::IconData {
        width: icon.width(),
        height: icon.height(),
        rgba: icon.into_raw(),
    }
}

pub fn texture(ctx: &eframe::egui::Context) -> eframe::egui::TextureHandle {
    let icon = window_icon();
    ctx.load_texture(
        "Symphony brush mark",
        eframe::egui::ColorImage::from_rgba_unmultiplied(
            [icon.width as usize, icon.height as usize],
            &icon.rgba,
        ),
        eframe::egui::TextureOptions::LINEAR,
    )
}

#[cfg(target_os = "macos")]
pub fn set_dock_icon() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(ICON_PNG);
    if let Some(icon) = NSImage::initWithData(main_thread.alloc(), &data) {
        let app = NSApplication::sharedApplication(main_thread);
        // AppKit is on the main thread and retains the image for the Dock.
        unsafe { app.setApplicationIconImage(Some(&icon)) };
    }
}
