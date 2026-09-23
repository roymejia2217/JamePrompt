$ErrorActionPreference = "Stop"

$Scripts = @(
    "./scripts/build_windows_msi.ps1",
    "./scripts/validate_windows_msi.ps1",
    "./scripts/validate_windows_runtime.ps1",
    "./scripts/smoke/native-hotkey-windows.ps1"
)

foreach ($Script in $Scripts) {
    $Tokens = $null
    $SyntaxErrors = $null
    [System.Management.Automation.Language.Parser]::ParseFile(
        (Resolve-Path $Script).Path,
        [ref]$Tokens,
        [ref]$SyntaxErrors
    ) | Out-Null

    if ($SyntaxErrors.Count -ne 0) {
        foreach ($SyntaxError in $SyntaxErrors) {
            Write-Error "${Script}: $($SyntaxError.Message)"
        }
        throw "PowerShell syntax validation failed: $Script"
    }
}

Write-Host "Windows packaging PowerShell syntax validation passed"
