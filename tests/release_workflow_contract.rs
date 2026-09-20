use std::path::Path;

fn read_release_workflow() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("Expected {} to be readable: {error}", path.display()))
        .replace("\r\n", "\n")
}

#[test]
fn release_job_checks_out_repository_before_using_release_tooling() {
    let workflow = read_release_workflow();
    let release_job = workflow
        .split("\n  release:\n")
        .nth(1)
        .expect("release workflow must define a release job");

    let checkout = release_job
        .find("- name: Checkout release tooling\n        uses: actions/checkout@")
        .expect("release job must checkout repository tooling");
    let staging = release_job
        .find("python3 scripts/stage_release_assets.py")
        .expect("release job must stage release assets with the tracked script");

    assert!(
        checkout < staging,
        "release tooling must be checked out before stage_release_assets.py is executed"
    );
}

#[test]
fn rpm_release_job_uses_builder_with_dbus_daemon_for_portal_contract_tests() {
    let workflow = read_release_workflow();
    let rpm_job = workflow
        .split("\n  rpm:\n")
        .nth(1)
        .expect("release workflow must define an rpm job")
        .split("\n  appimage:\n")
        .next()
        .expect("rpm job must precede the appimage job");
    let builder_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/build_rpm_fedora.sh");
    let builder = std::fs::read_to_string(&builder_path).unwrap_or_else(|error| {
        panic!("Expected {} to be readable: {error}", builder_path.display())
    });

    assert!(
        rpm_job.contains("bash scripts/build_rpm_fedora.sh"),
        "rpm release job must execute the tracked Fedora builder"
    );
    assert!(
        builder.contains("dbus-daemon"),
        "rpm builder must install dbus-daemon because dbus-run-session is required by the portal contract tests"
    );
}

#[test]
fn appimage_release_job_provisions_and_smokes_x11_runtime() {
    let workflow = read_release_workflow();
    let appimage_job = workflow
        .split("\n  appimage:\n")
        .nth(1)
        .expect("release workflow must define an appimage job")
        .split("\n  windows:\n")
        .next()
        .expect("appimage job must precede the windows job");

    for required in [
        "libxkbcommon-x11-0",
        "xvfb",
        "xauth",
        "xdotool",
        "zenity",
        "Run AppImage X11 native hotkey auto-paste smoke",
        "APPIMAGE_EXTRACT_AND_RUN: \"1\"",
        "scripts/smoke/native-hotkey-x11.sh",
    ] {
        assert!(
            appimage_job.contains(required),
            "appimage release job must include required runtime contract: {}",
            required
        );
    }
}

#[test]
fn windows_release_job_smokes_release_binary_before_packaging() {
    let workflow = read_release_workflow();
    let windows_job = workflow
        .split("\n  windows:\n")
        .nth(1)
        .expect("release workflow must define a windows job")
        .split("\n  release:\n")
        .next()
        .expect("windows job must precede the release job");

    let build = windows_job
        .find("- name: Build Windows binaries")
        .expect("windows release job must build release binaries");
    let smoke = windows_job
        .find("- name: Run Windows release native hotkey auto-paste smoke")
        .expect("windows release job must smoke the release binary");
    let portable = windows_job
        .find("- name: Create portable archive")
        .expect("windows release job must package the portable archive");

    assert!(
        build < smoke && smoke < portable,
        "Windows release auto-paste smoke must run after build and before packaging"
    );
    assert!(
        windows_job.contains(
            "./scripts/smoke/native-hotkey-windows.ps1 -Binary ./target/$env:WINDOWS_TARGET/release/jame-prompt.exe"
        ),
        "Windows release smoke must execute the release-profile binary"
    );
}

#[test]
fn prerelease_notes_describe_current_multiplatform_validation_scope() {
    let workflow = read_release_workflow();

    assert!(
        !workflow.contains("Wayland support release candidate."),
        "Prerelease notes must not describe every future prerelease as Wayland-only"
    );
    for required in ["Linux Wayland", "Linux X11", "Windows", "prerelease"] {
        assert!(
            workflow.contains(required),
            "Prerelease notes must include current validation scope: {}",
            required
        );
    }
}
