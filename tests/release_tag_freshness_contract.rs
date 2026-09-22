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
fn release_tag_creation_reconciles_remote_state_again_after_long_running_gates() {
    let tagger = read_file("scripts/create_release_tag.sh");

    assert!(
        tagger.contains("assert_release_state_current()"),
        "release tagger must centralize the exact-main and tag-absence invariant"
    );
    assert!(
        tagger.matches("git fetch origin main --tags").count() >= 2,
        "release tagger must fetch main and tags both before and after long-running gates"
    );
    assert!(
        tagger.matches("assert_release_state_current").count() >= 3,
        "release state invariant must be defined and enforced at least twice"
    );

    let change_gate = tagger
        .find("scripts/verify_change_gate.sh")
        .expect("release tagger must retain local change gate");
    let final_fetch = tagger
        .rfind("git fetch origin main --tags")
        .expect("release tagger must re-fetch remote state after local gates");
    let preflight = tagger
        .find("python3 scripts/validate_release_gate.py")
        .expect("release tagger must retain release preflight");
    let tag = tagger
        .find("git tag -a")
        .expect("release tagger must create annotated tag");

    assert!(
        change_gate < final_fetch && final_fetch < preflight && preflight < tag,
        "remote state must be re-fetched after local gates and before preflight/tag creation"
    );
}

#[test]
fn release_state_invariant_fails_closed_on_advanced_main_or_existing_tag() {
    let tagger = read_file("scripts/create_release_tag.sh");

    for required in [
        "head_commit=\"$(git rev-parse HEAD)\"",
        "main_commit=\"$(git rev-parse origin/main)\"",
        "[ \"$head_commit\" = \"$main_commit\" ]",
        "release tags must be created from the current origin/main commit",
        "git rev-parse --verify --quiet \"refs/tags/$tag\"",
        "release tag already exists: $tag",
    ] {
        assert!(
            tagger.contains(required),
            "release freshness gate must preserve invariant: {}",
            required
        );
    }

    for forbidden in ["git push --force", "git tag -f", "|| true"] {
        assert!(
            !tagger.contains(forbidden),
            "release tag freshness must remain fail-closed: {}",
            forbidden
        );
    }
}


#[test]
fn release_tag_push_reconciles_remote_state_after_local_tag_validation() {
    let tagger = read_file("scripts/create_release_tag.sh");

    for required in [
        "assert_release_push_state_current()",
        "assert_main_current",
        "assert_remote_tag_absent",
        "git ls-remote --exit-code origin \"refs/tags/$tag\"",
        "release tag already exists on origin: $tag",
        "unable to verify remote release tag absence: $tag",
    ] {
        assert!(
            tagger.contains(required),
            "pre-push release reconciliation must preserve invariant: {}",
            required
        );
    }

    assert!(
        tagger.matches("git fetch origin main --tags").count() >= 3,
        "release tagger must refresh remote state again immediately before push"
    );

    let local_tag = tagger
        .find("git tag -a")
        .expect("release tagger must create annotated tag");
    let post_tag_validation = tagger
        .rfind("python3 scripts/validate_release_gate.py")
        .expect("release tagger must validate the created tag");
    let final_fetch = tagger
        .rfind("git fetch origin main --tags")
        .expect("release tagger must refresh remote state after local tag validation");
    let final_assert = tagger
        .rfind("assert_release_push_state_current")
        .expect("release tagger must reconcile exact remote state before push");
    let push = tagger
        .find("git push origin \"$tag\"")
        .expect("release tagger must push the validated tag");

    assert!(
        local_tag < post_tag_validation
            && post_tag_validation < final_fetch
            && final_fetch < final_assert
            && final_assert < push,
        "remote release state must be reconciled after local tag validation and immediately before push"
    );
}


#[test]
fn remote_tag_absence_probe_fails_closed_on_query_errors() {
    let tagger = read_file("scripts/create_release_tag.sh");

    for required in [
        "remote_status=$?",
        "[ \"$remote_status\" -ne 2 ]",
        "unable to verify remote release tag absence: $tag",
        "exit \"$remote_status\"",
    ] {
        assert!(
            tagger.contains(required),
            "remote tag absence probe must distinguish absence from query failure: {}",
            required
        );
    }

    assert!(
        !tagger.contains("git ls-remote --exit-code origin \"refs/tags/$tag\" >/dev/null || true"),
        "remote tag lookup errors must never be suppressed"
    );
}
