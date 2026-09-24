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
fn annotated_tag_message_is_rendered_from_release_metadata() {
    let metadata = read_file("scripts/validate_release_metadata.py");
    let tagger = read_file("scripts/create_release_tag.sh");

    for required in [
        "render_tag_message",
        "--write-tag-message",
        "metadata.tag",
        "metadata.notes",
    ] {
        assert!(
            metadata.contains(required),
            "release metadata renderer must include tag-message contract: {}",
            required
        );
    }

    for required in [
        "validate_release_metadata.py",
        "--write-tag-message",
        "git tag -a --cleanup=verbatim \"$tag\" -F \"$tag_message_file\"",
    ] {
        assert!(
            tagger.contains(required),
            "tag creation must consume the validated annotation file: {}",
            required
        );
    }

    assert!(
        !tagger.contains("git tag -a \"$tag\" -m \"$tag\""),
        "tag creation must not use the legacy title-only annotation"
    );
}

#[test]
fn release_gate_validates_actual_annotation_before_release_or_push() {
    let gate = read_file("scripts/validate_release_gate.py");
    let tagger = read_file("scripts/create_release_tag.sh");

    for required in [
        "validate_tag_annotation",
        "git for-each-ref",
        "--format=%(contents)",
        "annotated tag message does not match release metadata",
    ] {
        assert!(
            gate.contains(required),
            "release gate must validate actual tag annotation: {}",
            required
        );
    }

    let create = tagger
        .find("git tag -a --cleanup=verbatim \"$tag\" -F \"$tag_message_file\"")
        .expect("tagger must create annotated tag from rendered metadata");
    let post_tag_gate = tagger
        .rfind("python3 scripts/validate_release_gate.py")
        .expect("tagger must validate the created tag before push");
    let push = tagger
        .find("git push origin \"$tag\"")
        .expect("tagger must retain explicit tag push");

    assert!(
        create < post_tag_gate && post_tag_gate < push,
        "created tag annotation must be validated before remote push"
    );
}

#[test]
fn local_tag_is_removed_when_post_creation_validation_or_push_fails() {
    let tagger = read_file("scripts/create_release_tag.sh");

    for required in [
        "cleanup_local_tag()",
        "tag_created=false",
        "tag_pushed=false",
        "trap cleanup_local_tag EXIT",
        "git tag -d \"$tag\"",
    ] {
        assert!(
            tagger.contains(required),
            "tag creation must bound local side effects: {}",
            required
        );
    }

    for forbidden in ["git tag -f", "git push --force", "|| true"] {
        assert!(
            !tagger.contains(forbidden),
            "tag annotation path must remain fail-closed: {}",
            forbidden
        );
    }
}


#[test]
fn annotated_tag_creation_preserves_release_metadata_verbatim() {
    let tagger = read_file("scripts/create_release_tag.sh");

    assert!(
        tagger.contains(
            "git tag -a --cleanup=verbatim \"$tag\" -F \"$tag_message_file\""
        ),
        "annotated tag creation must preserve Markdown release metadata verbatim"
    );
    assert!(
        !tagger.contains("git tag -a --cleanup=verbatim \"$tag\" -F \"$tag_message_file\""),
        "annotated tag creation must not use Git's default comment-stripping cleanup"
    );
}
