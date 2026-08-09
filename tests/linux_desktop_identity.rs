#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};

const DESKTOP_APP_ID: &str = "io.github.roymejia2217.JamePrompt";

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read_file(relative: &str) -> String {
    let path = repo_path(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("Expected {} to be readable: {}", path.display(), error))
}

#[test]
fn linux_packagers_install_a_portal_compatible_desktop_id() {
    let deb = read_file("packaging/linux/build-deb.sh");
    let arch = read_file("packaging/arch/PKGBUILD");
    let rpm = read_file("packaging/rpm/jame-prompt.spec");
    let appimage = read_file("packaging/appimage/build-appimage.sh");

    for (name, content) in [
        ("deb", deb.as_str()),
        ("arch", arch.as_str()),
        ("rpm", rpm.as_str()),
        ("appimage", appimage.as_str()),
    ] {
        assert!(
            content.contains(DESKTOP_APP_ID),
            "{name} packaging must install the reverse-DNS desktop identity"
        );
    }
}

#[test]
fn executable_and_icon_names_remain_stable() {
    let desktop = read_file("packaging/linux/jame-prompt.desktop");
    assert!(desktop.contains("Exec=jame-prompt"));
    assert!(desktop.contains("Icon=jame-prompt"));
    assert!(desktop.contains("StartupWMClass=jame-prompt"));
}
