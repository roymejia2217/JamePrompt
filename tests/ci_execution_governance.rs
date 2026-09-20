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
            section
                .split(&end)
                .next()
                .expect("job section terminator")
        }
        None => section,
    }
}

#[test]
fn ci_cancels_only_superseded_pull_request_runs() {
    let ci = read_file(".github/workflows/ci.yml");

    for required in [
        "concurrency:",
        "group: ${{ github.workflow }}-${{ github.event.pull_request.number || github.run_id }}",
        "cancel-in-progress: ${{ github.event_name == 'pull_request' }}",
    ] {
        assert!(
            ci.contains(required),
            "CI must include PR-only concurrency contract: {}",
            required
        );
    }

    assert!(
        !ci.contains("cancel-in-progress: true"),
        "CI must not indiscriminately cancel main-branch push validation"
    );
}

#[test]
fn protected_ci_jobs_have_explicit_fail_closed_timeouts() {
    let ci = read_file(".github/workflows/ci.yml");

    let expectations = [
        ("validate-commit-messages", Some("test-linux"), "timeout-minutes: 10"),
        ("test-linux", Some("test-windows"), "timeout-minutes: 45"),
        ("test-windows", Some("test_rpm"), "timeout-minutes: 45"),
        ("test_rpm", Some("test"), "timeout-minutes: 45"),
        ("test", None, "timeout-minutes: 5"),
    ];

    for (job, next, timeout) in expectations {
        let section = job_section(&ci, job, next);
        assert!(
            section.contains(timeout),
            "protected CI job {job} must include {timeout}"
        );
    }
}
