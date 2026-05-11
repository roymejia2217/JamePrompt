# Windows Distribution

GitHub Releases are the canonical binary source for Windows distribution.
The release workflow publishes the MSI installer, portable ZIP, and
`SHA256SUMS`. Winget and Chocolatey metadata must reference those release
assets directly instead of repackaging binaries in this repository.

## Supported Artifacts

For version `1.1.0`, the supported Windows artifacts are:

- `JamePrompt-1.1.0-x64.msi`
- `JamePrompt-1.1.0-x64-portable.zip`
- `SHA256SUMS`

The MSI is the supported installer for package managers. The portable ZIP is
for manual no-install use and should not replace the MSI in Winget manifests.

## Winget

Winget metadata lives in `packaging/winget`. Submit those manifests to the
Windows Package Manager Community Repository only after the GitHub Release is
published.

Release checklist:

1. Verify that the MSI URL is reachable from the GitHub Release.
2. Verify that `InstallerSha256` matches the MSI entry in `SHA256SUMS`.
3. Copy the latest accepted manifest directory, update it to the new release version, and replace the checksum only after the MSI is published.
4. Run `winget validate` against the versioned manifest directory.
5. Run the winget-pkgs Windows Sandbox validation script.
6. Open the pull request against `microsoft/winget-pkgs`.

## Chocolatey

Chocolatey metadata lives in `packaging/chocolatey`. The package installs the
official MSI from GitHub Releases with checksum validation and silent MSI
arguments.

Release checklist:

1. Verify that the MSI URL is reachable from the GitHub Release.
2. Verify that the checksum in `chocolateyInstall.ps1` matches `SHA256SUMS`.
3. Update the package version, release notes URL, MSI URL, and checksum only after the MSI is published.
4. Run `choco pack packaging/chocolatey/jame-prompt.nuspec`.
5. Test the package locally with `choco install jame-prompt --source <local-package-directory>`.
6. Test uninstall behavior in the same clean Windows environment.
7. Push the package to the Chocolatey Community Repository after local install and uninstall are green.

Do not announce Winget or Chocolatey availability until the package has been accepted by the target repository.

## Reputation Rules

- Keep GitHub Releases as the single source of truth for binaries.
- Keep package-manager metadata synchronized with `Cargo.toml`.
- Use immutable versioned release URLs.
- Publish checksums for every release artifact.
- Document known platform limits clearly, especially global hotkey and paste behavior on restricted desktop sessions.
