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

#[test]
fn commit_messages_use_conventional_commits_with_required_body() {
    let config = read_file("commitlint.config.cjs");
    let title_config = read_file("commitlint.title.config.cjs");
    let tests = read_file("scripts/test-commitlint.sh");

    for required in [
        "'body-empty': [2, 'never']",
        "'body-min-length': [2, 'always', 20]",
        "'body-leading-blank': [2, 'always']",
        "'footer-leading-blank': [2, 'always']",
    ] {
        assert!(
            config.contains(required),
            "commit contract must include rule: {}",
            required
        );
    }

    assert!(
        title_config.contains("'body-empty': [0]"),
        "PR titles must validate the Conventional Commit header without requiring a body"
    );
    assert!(
        title_config.contains("'body-min-length': [0]"),
        "PR title validation must disable commit-body length requirements"
    );
    assert!(
        tests.contains("assert_rejected 'missing body'"),
        "commitlint self-test must prove header-only commits are rejected"
    );
}

#[test]
fn pull_request_template_and_validator_enforce_exact_ordered_schema() {
    let template = read_file(".github/pull_request_template.md");
    let validator = read_file("scripts/validate_pr_description.py");
    let governance = read_file(".github/workflows/pr-governance.yml");

    let headings = [
        "## Summary",
        "## Motivation",
        "## Changes",
        "## Verification",
        "## Risk and rollback",
        "## Release impact",
    ];
    let mut previous = 0usize;
    for heading in headings {
        let position = template
            .find(heading)
            .unwrap_or_else(|| panic!("PR template missing heading: {}", heading));
        assert!(position >= previous, "PR template headings must be ordered");
        previous = position;
    }

    for required in [
        "REQUIRED_HEADINGS",
        "unexpected level-2 section",
        "required sections are out of order",
        "required section contains only a placeholder",
    ] {
        assert!(
            validator.contains(required),
            "PR validator must include strict schema behavior: {}",
            required
        );
    }

    assert!(
        governance.contains("commitlint.title.config.cjs"),
        "PR title governance must use the header-only Conventional Commit profile"
    );
}

#[test]
fn release_metadata_uses_semver_and_keep_a_changelog_as_executable_contracts() {
    let changelog = read_file("CHANGELOG.md");
    let metadata = read_file("scripts/validate_release_metadata.py");
    let release_gate = read_file("scripts/validate_release_gate.py");
    let tagger = read_file("scripts/create_release_tag.sh");
    let release = read_file(".github/workflows/release.yml");

    for required in [
        "Keep a Changelog",
        "Semantic Versioning",
        "## [Unreleased]",
        "### Added",
        "### Changed",
        "### Deprecated",
        "### Removed",
        "### Fixed",
        "### Security",
    ] {
        assert!(
            changelog.contains(required),
            "tracked changelog contract missing: {}",
            required
        );
    }

    for required in [
        "CATEGORIES",
        "alpha",
        "beta",
        "stable",
        "validate_changelog_for_tag",
        "--write-notes",
        "--existing-release-json",
        "release metadata contract: ok",
    ] {
        assert!(
            metadata.contains(required),
            "release metadata validator missing contract: {}",
            required
        );
    }

    for required in [
        "alpha.",
        "beta.",
        "stable",
        "--changelog",
        "validate_changelog_for_tag",
    ] {
        assert!(
            release_gate.contains(required),
            "release gate must enforce release metadata/type contract: {}",
            required
        );
    }

    assert!(
        tagger.contains("vMAJOR.MINOR.PATCH[-alpha.N|-beta.N]"),
        "tag creation usage must advertise the accepted SemVer prerelease profile"
    );
    assert!(
        tagger.contains("validate_release_metadata.py"),
        "tag creation must fail before tagging when release metadata is invalid"
    );

    for required in [
        "validate_release_metadata.py",
        "--write-notes",
        "--notes-file",
        "python3 scripts/probe_github_release.py",
        "--write-json \"$EXISTING_RELEASE_JSON\"",
        "--existing-release-json",
        "--title \"$TAG_NAME\"",
    ] {
        assert!(
            release.contains(required),
            "Release workflow must enforce metadata before publication: {}",
            required
        );
    }

    assert!(
        !release.contains("NOTES=\"Automated release for $TAG_NAME\""),
        "Release body must not be synthesized from an inline ad-hoc note"
    );
}

#[test]
fn release_version_self_test_covers_alpha_packaging_metadata() {
    let versioner = read_file("scripts/prepare_release_version.py");

    for required in [
        "alpha = parse_tag(\"v1.2.0-alpha.1\")",
        "assert alpha.canonical == \"1.2.0-alpha.1\"",
        "assert alpha.debian == \"1.2.0~alpha.1\"",
        "assert alpha.arch == \"1.2.0alpha.1\"",
        "assert alpha.rpm_release == \"0.1.alpha.1\"",
        "apply_release_version(root, alpha)",
    ] {
        assert!(
            versioner.contains(required),
            "release version contract must prove alpha packaging behavior: {}",
            required
        );
    }
}

#[test]
fn protected_and_local_gates_self_test_repository_metadata_contracts() {
    let ci = read_file(".github/workflows/ci.yml");
    let local_gate = read_file("scripts/verify_change_gate.sh");
    let package = read_file("package.json");

    for required in [
        "python3 scripts/validate_pr_description.py --self-test",
        "python3 scripts/validate_release_metadata.py --self-test",
        "bash scripts/test-commitlint.sh",
    ] {
        assert!(
            ci.contains(required) || local_gate.contains(required),
            "metadata contract must be executable from protected/local gates: {}",
            required
        );
    }

    for required in [
        "test:commitlint",
        "test:pr-governance",
        "test:release-metadata",
    ] {
        assert!(
            package.contains(required),
            "package scripts must expose metadata harness: {}",
            required
        );
    }
}

#[test]
fn readme_documents_published_backup_and_current_architecture() {
    let readme = read_file("README.md");

    for required in [
        "Prompt backup",
        "Export prompts",
        "Import prompts",
        "JSON",
        "src/application/",
        "src/domain/",
        "platform.rs",
        "prompt_backup.rs",
        "window_lifecycle.rs",
    ] {
        assert!(
            readme.contains(required),
            "README must document published product/architecture contract: {}",
            required
        );
    }
}

#[test]
fn readme_backup_scope_matches_serialized_product_contract() {
    let readme = read_file("README.md");
    let backup = read_file("src/prompt_backup.rs");

    for required in [
        "pub app_name: String",
        "pub app_version: String",
        "pub schema_version: u32",
        "pub prompts: Vec<Prompt>",
    ] {
        assert!(
            backup.contains(required),
            "backup schema contract missing serialized field: {}",
            required
        );
    }

    assert!(
        readme.contains(
            "Prompt backups contain prompt records only; settings remain in `settings.json` and are not included in the JSON backup."
        ),
        "README must not imply that prompt backups include application settings"
    );
}
