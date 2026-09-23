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
fn ci_checkouts_do_not_persist_credentials() {
    let ci = read_file(".github/workflows/ci.yml");

    for (job, next_job) in [
        ("validate-commit-messages", Some("test-linux")),
        ("test-linux", Some("test-windows")),
        ("test-windows", Some("test_deb")),
        ("test_deb", Some("test_arch")),
        ("test_arch", Some("test_rpm")),
        ("test_rpm", Some("test")),
    ] {
        let block = job_block(&ci, job, next_job);
        assert!(
            block.contains("actions/checkout@"),
            "{job} must retain repository checkout"
        );
        assert!(
            block.contains("persist-credentials: false"),
            "{job} must not persist checkout credentials"
        );
    }

    let commitlint = job_block(&ci, "validate-commit-messages", Some("test-linux"));
    assert!(
        commitlint.contains("fetch-depth: 0"),
        "commit validation must retain complete pull request history"
    );
}

#[test]
fn pull_request_target_checkout_is_base_bound_and_tokenless() {
    let governance = read_file(".github/workflows/pr-governance.yml");
    let job = job_block(&governance, "validate-pull-request-governance", None);

    for required in [
        "Checkout trusted base revision",
        "ref: ${{ github.event.pull_request.base.sha }}",
        "persist-credentials: false",
    ] {
        assert!(
            job.contains(required),
            "governance checkout missing trusted-base credential contract: {required}"
        );
    }
}


#[test]
fn nonrelease_workflows_reject_explicit_token_reintroduction() {
    let ci = read_file(".github/workflows/ci.yml");
    let governance = read_file(".github/workflows/pr-governance.yml");

    for (source, workflow) in [("CI", ci.as_str()), ("Governance", governance.as_str())] {
        for forbidden in [
            "GH_TOKEN:",
            "GITHUB_TOKEN:",
            "token: ${{ github.token }}",
            "token: ${{ secrets.GITHUB_TOKEN }}",
        ] {
            assert!(
                !workflow.contains(forbidden),
                "{source} must not reintroduce explicit GitHub token exposure: {forbidden}"
            );
        }
    }

    for forbidden in [
        "ref: ${{ github.event.pull_request.head.sha }}",
        "refs/pull/",
    ] {
        assert!(
            !governance.contains(forbidden),
            "pull_request_target governance must never execute pull request head content: {forbidden}"
        );
    }
}
