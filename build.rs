fn main() {
    // Regenerate Lucide icons
    println!("cargo:rerun-if-changed=fonts/icons.toml");
    iced_lucide::build("fonts/icons.toml").expect("Build icon module");

    // Pre-decode PNG to RGBA at compile time for tray icon
    println!("cargo:rerun-if-changed=assets/icons/tray_icon.png");
    buildtime_png::Builder::new()
        .include_png("assets/icons/tray_icon.png", "APP_TRAY_ICON")
        .emit_source_file_at("image.rs")
        .expect("Failed to pre-decode tray icon");

    // Embed icon into Windows executable only
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/app_icon.ico");
        res.compile().expect("Failed to embed Windows icon");
    }
}
