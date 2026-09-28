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

fn job_section<'a>(workflow: &'a str, job: &str, next_job: Option<&str>) -> &'a str {
    let start = format!("\n  {job}:\n");
    let section = workflow
        .split(&start)
        .nth(1)
        .unwrap_or_else(|| panic!("CI must define job {job}"));

    match next_job {
        Some(next) => {
            let end = format!("\n  {next}:\n");
            section.split(&end).next().expect("job section terminator")
        }
        None => section,
    }
}

#[test]
fn readme_policy_uses_the_adopted_locked_upstream_tooling() {
    let package = read_file("ci/readme-policy/package.json");
    let remark = read_file("ci/readme-policy/.remarkrc.json");

    for required in [
        r#""remark-cli": "12.0.1""#,
        r#""remark-preset-lint-consistent": "6.0.1""#,
        r#""remark-preset-lint-recommended": "7.0.1""#,
        r#""standard-readme-preset": "1.0.13""#,
    ] {
        assert!(
            package.contains(required),
            "README policy must pin upstream dependency: {required}"
        );
    }

    for required in [
        r#""standard-readme-preset""#,
        r#""standard-readme-preset/rules/require-sections.js""#,
        r#""installable": true"#,
        r#""remark-preset-lint-recommended""#,
        r#""remark-preset-lint-consistent""#,
    ] {
        assert!(
            remark.contains(required),
            "README policy must configure upstream contract: {required}"
        );
    }
}

#[test]
fn linux_ci_enforces_readme_policy_fail_closed_with_pinned_containers() {
    let ci = read_file(".github/workflows/ci.yml");
    let linux = job_section(&ci, "test-linux", Some("test-windows"));

    for required in [
        "Enforce Standard Readme and remark-lint",
        "docker.io/library/node:24.14.1-bookworm-slim@sha256:b506e7321f176aae77317f99d67a24b272c1f09f1d10f1761f2773447d8da26c",
        r#"--volume "$PWD:/JamePrompt""#,
        "--workdir /JamePrompt",
        "NPM_CONFIG_IGNORE_SCRIPTS=true npm ci",
        "ci/readme-policy/node_modules/.bin/remark README.md --frail --rc-path ci/readme-policy/.remarkrc.json",
        "Check README links",
        "lycheeverse/lychee:0.24.2@sha256:e2d19e57cf6ab037026f20b8e449a1f30d9d7f81eef4194763aab2eab20bd28d",
        r#"--volume "$PWD:/JamePrompt:ro""#,
        "--no-progress --root-dir . README.md",
    ] {
        assert!(
            linux.contains(required),
            "Linux CI must enforce README policy contract: {required}"
        );
    }

    assert!(!linux.contains("continue-on-error: true"));
}

#[test]
fn readme_preserves_visual_identity_with_standard_readme_structure() {
    let readme = read_file("README.md");

    assert!(
        readme.starts_with(
            "# JamePrompt\n\n<p align=\"center\">\n  <img src=\"docs/banner.webp\" alt=\"JamePrompt Banner\""
        ),
        "README must start with the canonical title followed immediately by the local banner"
    );

    for screenshot in [
        "docs/screenshots/main_window.webp",
        "docs/screenshots/main_window_min.webp",
        "docs/screenshots/prompt_editor.webp",
        "docs/screenshots/settings_window.webp",
        "docs/screenshots/favorites_filter.webp",
        "docs/screenshots/system_tray.webp",
        "docs/screenshots/about_window.webp",
    ] {
        assert!(
            readme.contains(screenshot),
            "README must preserve product screenshot: {screenshot}"
        );
    }

    let required = [
        "## Table of Contents",
        "## Install",
        "## Usage",
        "## Screenshots",
        "## Contributing",
        "## License",
    ];
    let positions: Vec<_> = required
        .iter()
        .map(|heading| {
            readme
                .find(heading)
                .unwrap_or_else(|| panic!("README must contain {heading}"))
        })
        .collect();

    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "Standard Readme sections must remain in canonical order"
    );

    assert!(!readme.contains("\n## Quick Start\n"));
    assert!(!readme.contains("\n## Installation\n"));
}

#[test]
fn existing_ci_aggregate_remains_the_single_merge_authority() {
    let ci = read_file(".github/workflows/ci.yml");
    let aggregate = job_section(&ci, "test", None);

    for required in [
        "test-linux",
        "test-windows",
        "test_deb",
        "test_arch",
        "test_rpm",
    ] {
        assert!(
            aggregate.contains(required),
            "existing aggregate must retain required dependency: {required}"
        );
    }

    assert!(!ci.contains("\n  documentation:\n"));
}
