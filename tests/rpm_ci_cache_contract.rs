use std::path::{Path, PathBuf};

const CACHE_ACTION_SHA: &str = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
const FEDORA_DIGEST: &str = "43b29f65a41eb9c35e1cd5323e3bdf3b655c2357a9f4f1ff2f9c2798e5045d80";

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read_file(relative: &str) -> String {
    let path = repo_path(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("Expected {} to be readable: {}", path.display(), error))
        .replace("\r\n", "\n")
}

fn job_block<'a>(workflow: &'a str, job: &str, next_job: Option<&str>) -> &'a str {
    let start = workflow
        .find(&format!("  {job}:\n"))
        .unwrap_or_else(|| panic!("missing workflow job: {job}"));
    let end = next_job
        .and_then(|next| {
            workflow[start + 1..]
                .find(&format!("\n  {next}:\n"))
                .map(|offset| start + 1 + offset)
        })
        .unwrap_or(workflow.len());
    &workflow[start..end]
}

#[test]
fn rpm_ci_uses_exact_restore_only_cache_contract() {
    let ci = read_file(".github/workflows/ci.yml");
    let rpm = job_block(&ci, "test_rpm", Some("test"));

    let restore = format!("uses: actions/cache/restore@{CACHE_ACTION_SHA} # v6.1.0");
    assert!(
        rpm.contains(&restore),
        "RPM CI must use the pinned first-party restore action"
    );
    assert!(
        rpm.contains("id: rpm-cargo-cache"),
        "RPM cache restore must expose the primary key for trusted save"
    );
    assert!(
        rpm.contains("path: target/rpm-cargo"),
        "RPM CI must cache only the dedicated Cargo target directory"
    );
    assert!(
        rpm.contains(&format!("rpm-cargo-fedora44-{FEDORA_DIGEST}")),
        "RPM cache identity must bind the exact Fedora image digest"
    );
    assert!(
        rpm.contains("hashFiles('Cargo.lock', 'Cargo.toml', 'packaging/rpm/jame-prompt.spec', 'scripts/build_rpm_fedora.sh')"),
        "RPM cache key must bind dependency and builder contracts"
    );
    assert!(
        !rpm.contains("restore-keys:"),
        "RPM cache must not accept prefix-matched stale caches"
    );
}

#[test]
fn rpm_builder_uses_persistent_dedicated_cargo_target() {
    let builder = read_file("scripts/build_rpm_fedora.sh");
    let spec = read_file("packaging/rpm/jame-prompt.spec");

    assert!(
        builder.contains(r#"if [[ -n "${JAME_PROMPT_RPM_CARGO_TARGET_DIR:-}" ]]; then"#),
        "RPM builder must make persistent Cargo output an explicit opt-in"
    );
    assert!(
        builder.contains("export CARGO_TARGET_DIR=\"$JAME_PROMPT_RPM_CARGO_TARGET_DIR\""),
        "RPM builder must export the opt-in Cargo target directory to rpmbuild"
    );
    assert!(
        spec.contains(r#"${CARGO_TARGET_DIR:-target}/release/%{name}"#),
        "RPM install must consume the configured Cargo target directory"
    );
}

#[test]
fn rpm_cache_is_ci_only_and_release_remains_uncached() {
    let ci = read_file(".github/workflows/ci.yml");
    let release = read_file(".github/workflows/release.yml");
    let rpm = job_block(&ci, "test_rpm", Some("test"));

    assert!(
        rpm.contains("-e JAME_PROMPT_RPM_CARGO_TARGET_DIR=/workspace/target/rpm-cargo"),
        "CI RPM container must explicitly opt into the persistent Cargo target"
    );
    assert!(
        !release.contains("actions/cache/"),
        "Release workflow must not gain cache actions from the CI optimization"
    );
    assert!(
        !release.contains("JAME_PROMPT_RPM_CARGO_TARGET_DIR"),
        "Release workflow must retain its existing uncached RPM builder behavior"
    );
}

#[test]
fn rpm_cache_write_boundary_is_trusted_and_post_validation() {
    let ci = read_file(".github/workflows/ci.yml");
    let rpm = job_block(&ci, "test_rpm", Some("test"));

    let restore = rpm
        .find("- name: Restore RPM Cargo build cache")
        .expect("RPM cache restore step");
    let build = rpm
        .find("- name: Build RPM package in pinned Fedora container")
        .expect("RPM build step");
    let verify = rpm
        .find("- name: Verify RPM package artifact")
        .expect("RPM verification step");
    let save = rpm
        .find("- name: Save RPM Cargo build cache")
        .expect("RPM cache save step");

    assert!(
        restore < build && build < verify && verify < save,
        "RPM cache must restore before build and save only after artifact validation"
    );

    for required in [
        "if: github.event_name == 'push' && github.ref == 'refs/heads/main' && steps.rpm-cargo-cache.outputs.cache-hit != 'true'",
        "uses: actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0",
        "key: ${{ steps.rpm-cargo-cache.outputs.cache-primary-key }}",
    ] {
        assert!(
            rpm.contains(required),
            "RPM cache save boundary missing trusted-main contract: {required}"
        );
    }

    for forbidden in [
        "restore-keys:",
        "path: target/rpmbuild",
        "path: ~/.cargo",
        "path: /root/.cargo",
    ] {
        assert!(
            !rpm.contains(forbidden),
            "RPM cache must not broaden its trust or artifact surface: {forbidden}"
        );
    }
}

#[test]
fn rpm_cache_actions_remain_supply_chain_pinned() {
    let ci = read_file(".github/workflows/ci.yml");
    let rpm = job_block(&ci, "test_rpm", Some("test"));

    let pinned = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
    assert_eq!(
        rpm.matches(&format!("actions/cache/restore@{pinned}"))
            .count(),
        1,
        "RPM CI must contain exactly one pinned cache restore action"
    );
    assert_eq!(
        rpm.matches(&format!("actions/cache/save@{pinned}"))
            .count(),
        1,
        "RPM CI must contain exactly one pinned cache save action"
    );
}
