use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::tempdir;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap_or_else(|error| {
        panic!("Failed to write {}: {}", path.display(), error);
    });
    let mut permissions = fs::metadata(path)
        .unwrap_or_else(|error| panic!("Failed to read {}: {}", path.display(), error))
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .unwrap_or_else(|error| panic!("Failed to mark {} executable: {}", path.display(), error));
}

fn copy_path(source: &Path, destination: &Path) {
    if source.is_dir() {
        fs::create_dir_all(destination).unwrap_or_else(|error| {
            panic!("Failed to create {}: {}", destination.display(), error)
        });

        for entry in fs::read_dir(source)
            .unwrap_or_else(|error| panic!("Failed to read {}: {}", source.display(), error))
        {
            let entry = entry.unwrap_or_else(|error| {
                panic!("Failed to read entry in {}: {}", source.display(), error)
            });
            copy_path(&entry.path(), &destination.join(entry.file_name()));
        }
    } else {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("Failed to create {}: {}", parent.display(), error));
        }

        fs::copy(source, destination).unwrap_or_else(|error| {
            panic!(
                "Failed to copy {} to {}: {}",
                source.display(),
                destination.display(),
                error
            )
        });
    }
}

fn mock_command(log_path: &Path, body: &str) -> String {
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
echo "$(basename "$0") $*" >> "{log}"
{body}
"#,
        log = log_path.display(),
        body = body
    )
}

fn prepare_mock_path(log_path: &Path, include_rpmbuild: bool, fail_makepkg: bool) -> PathBuf {
    let mock_dir = tempdir().expect("mock dir").keep();
    let bin_dir = mock_dir.join("bin");
    fs::create_dir_all(&bin_dir).expect("mock bin dir");

    write_executable(
        &bin_dir.join("cargo"),
        &mock_command(
            log_path,
            r#"
if [[ "$1" == "build" ]]; then
  mkdir -p target/release
  cat > target/release/jame-prompt <<'EOF'
#!/usr/bin/env bash
echo mock binary
EOF
  chmod 755 target/release/jame-prompt
fi
"#,
        ),
    );

    write_executable(
        &bin_dir.join("python3"),
        &mock_command(
            log_path,
            r#"
sizes=(16 22 24 32 48 64 128 256 512)
for size in "${sizes[@]}"; do
  mkdir -p "assets/icons/hicolor/${size}x${size}/apps"
  cp assets/icons/app_icon.png "assets/icons/hicolor/${size}x${size}/apps/jame-prompt.png"
done
"#,
        ),
    );

    write_executable(
        &bin_dir.join("dpkg"),
        &mock_command(log_path, r#"echo amd64"#),
    );

    write_executable(
        &bin_dir.join("dpkg-deb"),
        &mock_command(
            log_path,
            r#"
output="${@: -1}"
mkdir -p "$(dirname "$output")"
touch "$output"
"#,
        ),
    );

    write_executable(
        &bin_dir.join("makepkg"),
        &mock_command(
            log_path,
            if fail_makepkg {
                r#"exit 7"#
            } else {
                r#"mkdir -p target/makepkg && touch target/makepkg/jame-prompt.pkg.tar.zst"#
            },
        ),
    );

    if include_rpmbuild {
        write_executable(
            &bin_dir.join("rpmbuild"),
            &mock_command(
                log_path,
                r#"mkdir -p target/rpmbuild/RPMS/x86_64 && touch target/rpmbuild/RPMS/x86_64/jame-prompt.rpm"#,
            ),
        );
    }

    write_executable(
        &bin_dir.join("linuxdeploy"),
        &mock_command(log_path, r#"true"#),
    );

    write_executable(
        &bin_dir.join("appimagetool"),
        &mock_command(
            log_path,
            r#"
output="${@: -1}"
mkdir -p "$(dirname "$output")"
touch "$output"
"#,
        ),
    );

    bin_dir
}

fn prepare_workspace() -> PathBuf {
    let workspace = tempdir().expect("workspace dir").keep();
    let source_root = repo_root();

    for entry in [
        "build.sh",
        "Cargo.toml",
        "LICENSE",
        "README.md",
        "assets",
        "packaging",
        "src",
        "fonts",
    ] {
        copy_path(&source_root.join(entry), &workspace.join(entry));
    }

    let build_script = workspace.join("build.sh");
    let mut permissions = fs::metadata(&build_script)
        .unwrap_or_else(|error| panic!("Failed to read {}: {}", build_script.display(), error))
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&build_script, permissions).unwrap_or_else(|error| {
        panic!(
            "Failed to mark {} executable: {}",
            build_script.display(),
            error
        )
    });

    workspace
}

fn run_build_script(workspace: &Path, mock_path: &Path) -> (bool, String) {
    let output = Command::new("bash")
        .arg("build.sh")
        .current_dir(workspace)
        .env(
            "PATH",
            format!("{}:{}", mock_path.display(), env::var("PATH").unwrap()),
        )
        .output()
        .expect("build.sh should run");

    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr),
    )
}

#[test]
fn build_orchestrator_reports_skipped_targets_and_continues() {
    let log_dir = tempdir().expect("log dir").keep();
    let log = log_dir.join("build.log");
    let mock_path = prepare_mock_path(&log, false, false);
    let workspace = prepare_workspace();

    let (success, output) = run_build_script(&workspace, &mock_path);

    assert!(
        success,
        "build.sh should succeed when missing targets are skipped"
    );
    assert!(
        output.contains("SKIP rpm"),
        "RPM should be skipped without rpmbuild"
    );
    assert!(output.contains("BUILD debian"));
    assert!(output.contains("BUILD arch"));
    assert!(output.contains("BUILD appimage"));
}

#[test]
fn build_orchestrator_reports_failures_without_stopping() {
    let log_dir = tempdir().expect("log dir").keep();
    let log = log_dir.join("build.log");
    let mock_path = prepare_mock_path(&log, true, true);
    let workspace = prepare_workspace();

    let (success, output) = run_build_script(&workspace, &mock_path);

    assert!(!success, "build.sh should fail when any target fails");
    assert!(output.contains("FAIL arch"));
    assert!(output.contains("BUILD rpm"));
    assert!(output.contains("BUILD appimage"));
}
