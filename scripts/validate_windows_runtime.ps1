param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryDirectory
)

$ErrorActionPreference = "Stop"
$ForbiddenRuntimeDlls = @(
    "VCRUNTIME140.dll",
    "VCRUNTIME140_1.dll",
    "MSVCP140.dll",
    "MSVCP140_1.dll"
)

$dumpbinCommand = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
$dumpbinPath = if ($dumpbinCommand) { $dumpbinCommand.Source } else { $null }

if (-not $dumpbinPath) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $installationPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($installationPath) {
            $dumpbinPath = Get-ChildItem -Path (Join-Path $installationPath "VC\Tools\MSVC") -Recurse -Filter dumpbin.exe -ErrorAction SilentlyContinue |
                Where-Object { $_.FullName -match '\\Hostx64\\x64\\dumpbin.exe$' } |
                Sort-Object FullName -Descending |
                Select-Object -First 1 -ExpandProperty FullName
        }
    }
}

if (-not $dumpbinPath) {
    $searchRoots = @(
        "${env:ProgramFiles}\Microsoft Visual Studio\2022",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022"
    ) | Where-Object { Test-Path $_ }

    if ($searchRoots) {
        $dumpbinPath = Get-ChildItem -Path $searchRoots -Recurse -Filter dumpbin.exe -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match '\\Hostx64\\x64\\dumpbin.exe$' } |
            Sort-Object FullName -Descending |
            Select-Object -First 1 -ExpandProperty FullName
    }
}

if (-not $dumpbinPath) {
    throw "Unable to locate dumpbin.exe for Windows runtime dependency validation."
}

$resolvedDirectory = (Resolve-Path $BinaryDirectory).Path
$executables = Get-ChildItem -Path $resolvedDirectory -Filter "*.exe"
if (-not $executables) {
    throw "No Windows executables were found for runtime dependency validation."
}

foreach ($executable in $executables) {
    $imports = & $dumpbinPath /DEPENDENTS $executable.FullName
    foreach ($dll in $ForbiddenRuntimeDlls) {
        if ($imports -match [regex]::Escape($dll)) {
            throw "$($executable.Name) imports $dll. Windows release artifacts must run on a clean Windows 10 LTSC x64 VM without requiring the Visual C++ Redistributable."
        }
    }
}

Write-Host "Windows runtime dependency validation passed for $($executables.Count) executable(s)"
