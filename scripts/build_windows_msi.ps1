param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$BinaryDirectory = "./target/x86_64-pc-windows-msvc/release",
    [string]$OutputDirectory = "./target/wix"
)

$ErrorActionPreference = "Stop"

$Version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"').Matches[0].Groups[1].Value
if (-not $Version) {
    throw "Unable to read package version from Cargo.toml"
}

$SupportedVersionPattern = '^(?<base>(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*))(?:-(?:alpha|beta)\.(?:0|[1-9]\d*))?$'
if ($Version -match $SupportedVersionPattern) {
    $InstallerVersion = $Matches['base']
}
else {
    throw "MSI build requires a supported stable/alpha/beta SemVer package version, found: $Version"
}

$ResolvedBinaryDirectory = (Resolve-Path $BinaryDirectory).Path
$Binary = Join-Path $ResolvedBinaryDirectory "jame-prompt.exe"
if (-not (Test-Path $Binary)) {
    throw "Expected release binary not found: $Binary"
}

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$OutputPath = Join-Path $OutputDirectory "JamePrompt-$Version-x64.msi"

$cargoArgs = @(
    "wix",
    "--no-build",
    "--target", $Target,
    "--target-bin-dir", $ResolvedBinaryDirectory,
    "--install-version", $InstallerVersion,
    "--output", $OutputPath,
    "--nocapture"
)
& cargo @cargoArgs
if ($LASTEXITCODE -ne 0) {
    throw "cargo wix failed with exit code $LASTEXITCODE"
}

if (-not (Test-Path $OutputPath)) {
    throw "MSI build did not produce expected artifact: $OutputPath"
}

$Msi = Get-Item $OutputPath
if ($Msi.Length -le 0) {
    throw "MSI build did not produce a non-empty artifact: $OutputPath"
}

Write-Host "Built MSI: $($Msi.FullName)"
