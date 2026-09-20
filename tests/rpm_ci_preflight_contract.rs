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
fn protected_ci_builds_real_rpm_in_pinned_fedora_container() {
    let ci = read_file(".github/workflows/ci.yml");

    let rpm_job = ci
        .split("\n  test_rpm:\n")
        .nth(1)
        .expect("protected CI must define a test_rpm job")
        .split("\n  test:\n")
        .next()
        .expect("test_rpm job must precede the aggregate test job");

    for required in [
        "runs-on: ubuntu-latest",
        "actions/checkout@",
        "fedora:44@sha256:43b29f65a41eb9c35e1cd5323e3bdf3b655c2357a9f4f1ff2f9c2798e5045d80",
        "bash scripts/build_rpm_fedora.sh",
        "Verify RPM package artifact",
        "target/rpmbuild/RPMS",
        "-name '*.rpm'",
        "test -s",
    ] {
        assert!(
            rpm_job.contains(required),
            "protected RPM preflight must include contract: {}",
            required
        );
    }

    for forbidden in ["upload-artifact", "gh release", "contents: write"] {
        assert!(
            !rpm_job.contains(forbidden),
            "RPM preflight must not publish artifacts: {}",
            forbidden
        );
    }
}

#[test]
fn aggregate_test_requires_rpm_preflight() {
    let ci = read_file(".github/workflows/ci.yml");
    let aggregate = ci
        .split("\n  test:\n")
        .nth(1)
        .expect("CI must define the protected aggregate test job");

    for required in [
        "needs:",
        "- test-linux",
        "- test-windows",
        "- test_rpm",
        "RPM_RESULT: ${{ needs.test_rpm.result }}",
        "test \"$RPM_RESULT\" = \"success\"",
    ] {
        assert!(
            aggregate.contains(required),
            "protected aggregate test must require RPM preflight: {}",
            required
        );
    }
}
