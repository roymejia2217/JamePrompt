use std::path::{Path, PathBuf};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read_file(relative: &str) -> String {
    let path = repo_path(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("Expected {} to be readable: {}", path.display(), error))
        .replace("\r\n", "\n")
}

fn step_block<'a>(workflow: &'a str, step_name: &str) -> &'a str {
    let marker = format!("      - name: {step_name}\n");
    let start = workflow
        .find(&marker)
        .unwrap_or_else(|| panic!("missing workflow step: {step_name}"));
    let rest = &workflow[start + marker.len()..];
    let end = rest
        .find("\n      - name: ")
        .map(|offset| start + marker.len() + offset)
        .unwrap_or(workflow.len());
    &workflow[start..end]
}

#[test]
fn appimage_tool_download_is_public_and_tokenless() {
    let installer = read_file("scripts/install_appimage_tools.sh");

    for required in [
        "curl",
        "Accept: application/octet-stream",
        "X-GitHub-Api-Version:",
        "https://api.github.com/repos/${repo}/releases/assets/${asset_id}",
        "sha256sum --check --strict",
    ] {
        assert!(
            installer.contains(required),
            "AppImage tool installer missing tokenless download contract: {required}"
        );
    }

    for forbidden in ["gh api", "GH_TOKEN", "GITHUB_TOKEN"] {
        assert!(
            !installer.contains(forbidden),
            "AppImage tool installer must not depend on GitHub authentication: {forbidden}"
        );
    }
}

#[test]
fn appimage_tool_steps_do_not_receive_github_tokens() {
    for workflow_path in [".github/workflows/ci.yml", ".github/workflows/release.yml"] {
        let workflow = read_file(workflow_path);
        let step = step_block(&workflow, "Install verified AppImage tools");

        for forbidden in ["GH_TOKEN", "GITHUB_TOKEN", "github.token"] {
            assert!(
                !step.contains(forbidden),
                "{workflow_path} AppImage tool step exposes token: {forbidden}"
            );
        }
    }
}

#[test]
fn parity_validator_enforces_tokenless_appimage_tool_installation() {
    let parity = read_file("scripts/validate_ci_release_parity.py");

    assert!(
        parity.contains("AppImage tool installer must not receive GitHub token"),
        "parity validator must guard AppImage tool token isolation"
    );
}
