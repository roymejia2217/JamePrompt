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

fn job_block<'a>(workflow: &'a str, job: &str, next_job: &str) -> &'a str {
    let start = workflow
        .find(&format!("  {job}:\n"))
        .unwrap_or_else(|| panic!("missing release job: {job}"));
    let end = workflow[start + 1..]
        .find(&format!("\n  {next_job}:\n"))
        .map(|offset| start + 1 + offset)
        .unwrap_or(workflow.len());
    &workflow[start..end]
}

#[test]
fn release_builders_do_not_persist_checkout_credentials() {
    let release = read_file(".github/workflows/release.yml");

    for (job, next_job) in [
        ("deb", "arch"),
        ("arch", "rpm"),
        ("rpm", "appimage"),
        ("appimage", "windows"),
        ("windows", "release"),
    ] {
        let block = job_block(&release, job, next_job);
        assert!(
            block.contains("ref: ${{ env.RELEASE_REF }}"),
            "{job} must continue building the validated release tag"
        );
        assert!(
            block.contains("persist-credentials: false"),
            "{job} must not expose persisted checkout credentials to tagged build code"
        );
    }
}

#[test]
fn parity_validator_enforces_builder_credential_isolation() {
    let parity = read_file("scripts/validate_ci_release_parity.py");

    for required in [
        "Release/{platform}: builder checkout must disable persisted credentials",
        "persist-credentials",
        "Checkout",
    ] {
        assert!(
            parity.contains(required),
            "parity validator missing builder credential contract: {required}"
        );
    }
}
