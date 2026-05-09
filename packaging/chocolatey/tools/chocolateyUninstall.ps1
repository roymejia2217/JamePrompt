$ErrorActionPreference = 'Stop'

$packageName = 'jame-prompt'
$softwareName = 'JamePrompt*'
$installerType = 'msi'
$silentArgs = '/qn /norestart'
$validExitCodes = @(0, 3010, 1605, 1614, 1641)

$uninstallEntry = Get-UninstallRegistryKey -SoftwareName $softwareName | Select-Object -First 1

if ($null -eq $uninstallEntry) {
  Write-Warning "JamePrompt is not registered as an installed application."
  return
}

Uninstall-ChocolateyPackage `
  -PackageName $packageName `
  -FileType $installerType `
  -SilentArgs "$($uninstallEntry.PSChildName) $silentArgs" `
  -ValidExitCodes $validExitCodes
