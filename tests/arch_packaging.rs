use std::path::Path;

#[test]
fn arch_package_disables_split_debug_artifacts() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/arch/PKGBUILD");
    let pkgbuild = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("Expected {} to be readable: {error}", path.display()));

    assert!(
        pkgbuild
            .lines()
            .any(|line| line.trim() == "options=(!debug)"),
        "Arch release packages must disable makepkg's split debug package"
    );
}
