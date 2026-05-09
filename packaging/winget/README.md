# Winget Packaging

This directory contains the Windows Package Manager manifests for JamePrompt.
GitHub Releases are the canonical binary source, and the MSI installer is the
only Winget installer artifact.

Before submitting a new version to `microsoft/winget-pkgs`:

1. Publish the GitHub Release and verify that the MSI and `SHA256SUMS` assets are present.
2. Copy the versioned manifest directory and update `PackageVersion`, `InstallerUrl`, `InstallerSha256`, and `ReleaseDate`.
3. Run `winget validate <manifest-directory>`.
4. Run the winget-pkgs Windows Sandbox test against the manifest directory.
5. Submit the validated files to the Windows Package Manager Community Repository.

Do not edit the checksum by hand without comparing it to the published
`SHA256SUMS` release asset.
