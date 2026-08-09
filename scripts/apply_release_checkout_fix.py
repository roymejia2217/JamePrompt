from pathlib import Path


def replace_exact(path: str, old: str, new: str, expected: int = 1) -> None:
    target = Path(path)
    text = target.read_text()
    count = text.count(old)
    if count != expected:
        raise SystemExit(
            f"{path}: expected {expected} occurrence(s) of replacement block, found {count}"
        )
    target.write_text(text.replace(old, new, expected))


replace_exact(
    ".github/workflows/release.yml",
    """    steps:\n      - name: Download artifacts from current run\n""",
    """    steps:\n      - name: Checkout release source\n        uses: actions/checkout@v6\n\n      - name: Download artifacts from current run\n""",
)

replace_exact(
    "tests/packaging_metadata.rs",
    """#[test]\nfn release_workflow_publishes_checksums_with_artifacts() {\n""",
    """#[test]\nfn release_job_checks_out_source_before_staging_assets() {\n    let workflow = read_file(\".github/workflows/release.yml\");\n    let release_job = workflow\n        .split_once(\"\\n  release:\\n\")\n        .map(|(_, release)| release)\n        .expect(\"Release workflow should define a release job\");\n    let checkout = release_job\n        .find(\"- name: Checkout release source\")\n        .expect(\"Release job should check out the repository\");\n    let staging = release_job\n        .find(\"python3 scripts/stage_release_assets.py\")\n        .expect(\"Release job should stage normalized release assets\");\n\n    assert!(\n        checkout < staging,\n        \"Release job must check out scripts before staging release assets\"\n    );\n}\n\n#[test]\nfn release_workflow_publishes_checksums_with_artifacts() {\n""",
)

print("Release checkout patch applied successfully")
