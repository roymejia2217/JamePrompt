use std::fs;
use std::path::{Path, PathBuf};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read_file(relative: &str) -> String {
    let path = repo_path(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("Expected {} to be readable: {}", path.display(), error))
        .replace("\r\n", "\n")
}

fn workflow_paths() -> Vec<PathBuf> {
    let root = repo_path(".github/workflows");
    let mut paths = fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("Expected {} to be readable: {}", root.display(), error))
        .map(|entry| entry.expect("workflow directory entry").path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("yml" | "yaml")
            )
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn external_action_ref(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let value = trimmed
        .strip_prefix("uses: ")
        .or_else(|| trimmed.strip_prefix("- uses: "))?
        .split('#')
        .next()
        .expect("uses value")
        .trim();

    if value.starts_with("./") || value.starts_with("docker://") {
        return None;
    }

    Some(value)
}

#[test]
fn all_external_github_actions_are_pinned_to_full_commit_shas() {
    let mut checked = 0usize;

    for path in workflow_paths() {
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("Expected {} to be readable: {}", path.display(), error));

        for (index, line) in content.lines().enumerate() {
            let Some(action) = external_action_ref(line) else {
                continue;
            };

            let (_, reference) = action.rsplit_once('@').unwrap_or_else(|| {
                panic!(
                    "{}:{} external action must include an @ref: {}",
                    path.display(),
                    index + 1,
                    action
                )
            });

            assert!(
                reference.len() == 40 && reference.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "{}:{} external action must be pinned to a full 40-character commit SHA: {}",
                path.display(),
                index + 1,
                action
            );
            checked += 1;
        }
    }

    assert!(
        checked > 0,
        "expected at least one external GitHub Action reference"
    );
}

#[test]
fn appimage_release_tool_downloads_are_asset_pinned_and_checksum_verified() {
    let workflow = read_file(".github/workflows/release.yml");

    for required in [
        "LINUXDEPLOY_ASSET_ID: \"538917371\"",
        "LINUXDEPLOY_SHA256: \"36a2d7e274d12e1050d0e9ecfe11d339ed54720b2bec464c286d53f8b07f5c62\"",
        "APPIMAGETOOL_ASSET_ID: \"324406736\"",
        "APPIMAGETOOL_SHA256: \"ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0\"",
        "repos/linuxdeploy/linuxdeploy/releases/assets/$LINUXDEPLOY_ASSET_ID",
        "repos/AppImage/appimagetool/releases/assets/$APPIMAGETOOL_ASSET_ID",
        "sha256sum --check --strict",
    ] {
        assert!(
            workflow.contains(required),
            "release workflow must include pinned AppImage tool contract: {}",
            required
        );
    }

    assert!(
        !workflow.contains("gh release download --repo linuxdeploy/linuxdeploy"),
        "linuxdeploy must not be resolved from a mutable release tag"
    );
    assert!(
        !workflow.contains("gh release download --repo AppImage/appimagetool"),
        "appimagetool must not be resolved from whichever release is latest"
    );
}
