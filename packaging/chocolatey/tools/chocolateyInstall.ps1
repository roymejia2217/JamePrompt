$ErrorActionPreference = 'Stop'

$packageName = 'jame-prompt'
$softwareName = 'JamePrompt*'
$installerType = 'msi'
$silentArgs = '/qn /norestart'
$url64 = 'https://github.com/roymejia2217/JamePrompt/releases/download/v1.0.0/JamePrompt-1.0.0-x64.msi'
$checksum64 = '740798A2A471DA3F689C42F0BC160685D6B5742CDBDF2D612C5B1049DA67F9B8'
$checksumType64 = 'sha256'
$validExitCodes = @(0, 3010, 1641)

Install-ChocolateyPackage `
  -PackageName $packageName `
  -FileType $installerType `
  -SilentArgs $silentArgs `
  -Url64bit $url64 `
  -Checksum64 $checksum64 `
  -ChecksumType64 $checksumType64 `
  -ValidExitCodes $validExitCodes
