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
fn github_rest_transport_is_single_authority_for_protocol_defaults() {
    let transport = read_file("scripts/github_api.py");

    for required in [
        "API_ROOT = \"https://api.github.com\"",
        "API_VERSION = \"2026-03-10\"",
        "DEFAULT_TIMEOUT_SECONDS = 30",
        "REPOSITORY_PATTERN",
        "X-GitHub-Api-Version",
        "application/vnd.github+json",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GitHubApiError",
        "GitHubJsonResponse",
        "request_json",
        "--self-test",
    ] {
        assert!(
            transport.contains(required),
            "shared GitHub REST transport missing authority: {}",
            required
        );
    }
}

#[test]
fn release_gates_reuse_shared_transport_without_private_http_stacks() {
    for path in [
        "scripts/probe_github_release.py",
        "scripts/validate_main_ci_evidence.py",
        "scripts/validate_reusable_release_run.py",
    ] {
        let source = read_file(path);
        for required in ["from github_api import", "request_json"] {
            assert!(
                source.contains(required),
                "{} must reuse shared GitHub REST transport: {}",
                path,
                required
            );
        }

        for forbidden in [
            "API_VERSION =",
            "API_ROOT =",
            "urllib.request",
            "urllib.error",
            "urlopen(",
            "Request(",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} must not retain private transport authority: {}",
                path,
                forbidden
            );
        }
    }
}

#[test]
fn authentication_semantics_remain_explicit_per_consumer() {
    let evidence = read_file("scripts/validate_main_ci_evidence.py");
    let reusable = read_file("scripts/validate_reusable_release_run.py");
    let probe = read_file("scripts/probe_github_release.py");

    assert!(
        evidence.contains("github_token(required=False)"),
        "main CI evidence must preserve optional authentication"
    );
    assert!(
        reusable.contains("github_token(required=True)"),
        "reusable-run provenance must require authentication"
    );
    assert!(
        probe.contains("accepted_statuses=(200, 404)"),
        "release existence probe must preserve explicit 200/404 semantics"
    );
}

#[test]
fn protected_and_local_gates_self_test_shared_transport() {
    let ci = read_file(".github/workflows/ci.yml");
    let local = read_file("scripts/verify_change_gate.sh");

    for source in [&ci, &local] {
        assert!(
            source.contains("python3 scripts/github_api.py --self-test"),
            "shared GitHub REST transport self-test must run in protected and local gates"
        );
    }
}
