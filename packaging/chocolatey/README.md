# Chocolatey Packaging

This directory contains the Chocolatey package source for JamePrompt.
The package downloads the official MSI from GitHub Releases and verifies it
with the published SHA256 checksum.

Before pushing a new version to the Chocolatey Community Repository:

1. Publish the GitHub Release and verify that the MSI and `SHA256SUMS` assets are present.
2. Update `jame-prompt.nuspec`, `tools/chocolateyInstall.ps1`, and release notes to the new version.
3. Confirm that the MSI URL points to the official JamePrompt GitHub Release.
4. Confirm that the checksum matches the release `SHA256SUMS` entry for the MSI.
5. Run `choco pack packaging/chocolatey/jame-prompt.nuspec`.
6. Test in a clean Windows environment with `choco install jame-prompt --source <local-package-directory>`.

Do not push the package until silent install and uninstall have been verified.
