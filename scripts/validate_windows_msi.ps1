param(
    [Parameter(Mandatory = $true)]
    [string]$MsiPath
)

$ErrorActionPreference = "Stop"
$ResolvedMsi = (Resolve-Path $MsiPath).Path
$InstallDirectory = Join-Path $env:ProgramFiles "JamePrompt"
$InstalledBinary = Join-Path $InstallDirectory "jame-prompt.exe"
$MachineMarker = "HKLM:\SOFTWARE\JamePrompt"
$CommonShortcut = Join-Path $env:ProgramData "Microsoft\Windows\Start Menu\Programs\JamePrompt\JamePrompt.lnk"
$UserShortcut = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\JamePrompt\JamePrompt.lnk"
$TempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("jameprompt-msi-" + [Guid]::NewGuid())
$InstallLog = Join-Path $TempRoot "install.log"
$UninstallLog = Join-Path $TempRoot "uninstall.log"
$Installed = $false

New-Item -ItemType Directory -Path $TempRoot | Out-Null

function Quote-Argument {
    param([Parameter(Mandatory = $true)][string]$Value)
    return '"' + $Value + '"'
}

function Invoke-MsiExec {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$Arguments,
        [Parameter(Mandatory = $true)]
        [string]$Phase
    )

    $process = Start-Process -FilePath "msiexec.exe" -ArgumentList $Arguments -Wait -PassThru
    if (@(0, 3010, 1641) -notcontains $process.ExitCode) {
        throw "$Phase failed with msiexec exit code $($process.ExitCode)"
    }
}

function Get-JamePromptUninstallEntries {
    $paths = @(
        "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*",
        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*"
    )

    @(
        foreach ($path in $paths) {
            Get-ItemProperty -Path $path -ErrorAction SilentlyContinue |
                Where-Object { $_.DisplayName -eq "JamePrompt" }
        }
    )
}

try {
    Invoke-MsiExec -Phase "MSI install" -Arguments @(
        "/i",
        (Quote-Argument $ResolvedMsi),
        "/qn",
        "/norestart",
        "/l*v",
        (Quote-Argument $InstallLog)
    )
    $Installed = $true

    if (-not (Test-Path $InstalledBinary)) {
        throw "MSI install did not create expected binary: $InstalledBinary"
    }

    $uninstallEntries = @(Get-JamePromptUninstallEntries)
    if ($uninstallEntries.Count -lt 1) {
        throw "MSI install did not register JamePrompt in Windows Apps & Features"
    }

    if (-not (Test-Path $MachineMarker)) {
        throw "MSI install did not create machine registry marker: $MachineMarker"
    }

    if (-not ((Test-Path $CommonShortcut) -or (Test-Path $UserShortcut))) {
        throw "MSI install did not create the expected Start Menu shortcut"
    }

    ./scripts/validate_windows_runtime.ps1 -BinaryDirectory $InstallDirectory
    ./scripts/smoke/native-hotkey-windows.ps1 -Binary $InstalledBinary
}
finally {
    if ($Installed -or (Test-Path $InstalledBinary) -or @(Get-JamePromptUninstallEntries).Count -gt 0) {
        Invoke-MsiExec -Phase "MSI uninstall" -Arguments @(
            "/x",
            (Quote-Argument $ResolvedMsi),
            "/qn",
            "/norestart",
            "/l*v",
            (Quote-Argument $UninstallLog)
        )
    }

    if (Test-Path $InstalledBinary) {
        throw "MSI uninstall left installed binary behind: $InstalledBinary"
    }
    if (@(Get-JamePromptUninstallEntries).Count -gt 0) {
        throw "MSI uninstall left Apps & Features registration behind"
    }
    if (Test-Path $MachineMarker) {
        throw "MSI uninstall left machine registry marker behind: $MachineMarker"
    }
    if ((Test-Path $CommonShortcut) -or (Test-Path $UserShortcut)) {
        throw "MSI uninstall left Start Menu shortcut behind"
    }

    Remove-Item -Recurse -Force $TempRoot -ErrorAction SilentlyContinue
}

Write-Host "MSI install/runtime/uninstall validation passed"
