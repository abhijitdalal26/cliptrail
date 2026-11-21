# ClipTrail installer for Windows. In PowerShell:
#
#   irm https://abhijitdalal26.github.io/cliptrail/install.ps1 | iex
#
# Finds the newest release on GitHub, downloads the installer, checks it
# against the SHA-256 GitHub publishes for it, installs it for this user (no
# admin) and starts ClipTrail. Nothing else is downloaded or changed.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$repo = 'abhijitdalal26/cliptrail'

function Step($msg) { Write-Host "-> $msg" }

Write-Host 'ClipTrail installer' -ForegroundColor Cyan
if (-not [Environment]::Is64BitOperatingSystem) { throw 'ClipTrail needs 64-bit Windows.' }

$release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -Headers @{ 'User-Agent' = 'cliptrail-installer' }
$asset = $release.assets | Where-Object { $_.name -like '*_x64-setup.exe' } | Select-Object -First 1
if (-not $asset) { throw "No Windows installer in $($release.tag_name). See https://github.com/$repo/releases" }
Step "Latest version: $($release.tag_name.TrimStart('v'))"

$file = Join-Path ([IO.Path]::GetTempPath()) $asset.name
Step ("Downloading {0} ({1:N1} MB)" -f $asset.name, ($asset.size / 1MB))
Invoke-WebRequest $asset.browser_download_url -OutFile $file -UseBasicParsing

if ($asset.digest -notmatch '^sha256:([0-9a-f]{64})$') { Remove-Item $file; throw 'GitHub gave no checksum for the installer; not installing.' }
$actual = (Get-FileHash $file -Algorithm SHA256).Hash.ToLower()
if ($actual -ne $Matches[1]) { Remove-Item $file; throw 'Checksum mismatch: the download is not the published installer. Not installing.' }
Step 'Checksum OK'

Get-Process cliptrail -ErrorAction SilentlyContinue | Stop-Process -Force
Step 'Installing...'
$p = Start-Process $file -ArgumentList '/S' -PassThru -Wait
Remove-Item $file -ErrorAction SilentlyContinue
if ($p.ExitCode -ne 0) { throw "The installer failed (exit code $($p.ExitCode))." }

$exe = Join-Path $env:LOCALAPPDATA 'ClipTrail\cliptrail.exe'
if (Test-Path $exe) { Start-Process $exe }
Write-Host "Done: ClipTrail $($release.tag_name.TrimStart('v')) is installed and running." -ForegroundColor Green
