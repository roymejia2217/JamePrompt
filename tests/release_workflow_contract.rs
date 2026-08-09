use std::path::Path;

fn read_release_workflow() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("Expected {} to be readable: {error}", path.display()))
}

#[test]
fn release_job_checks_out_repository_before_using_release_tooling() {
    let workflow = read_release_workflow();
    let release_job = workflow
        .split("\n  release:\n")
        .nth(1)
        .expect("release workflow must define a release job");

    let checkout = release_job
        .find("- name: Checkout release tooling\n        uses: actions/checkout@v6")
        .expect("release job must checkout repository tooling");
    let staging = release_job
        .find("python3 scripts/stage_release_assets.py")
        .expect("release job must stage release assets with the tracked script");

    assert!(
        checkout < staging,
        "release tooling must be checked out before stage_release_assets.py is executed"
    );
}
