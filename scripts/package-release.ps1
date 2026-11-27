# KLBF Lab: the players' zip (release.yml, and a local dry run). Only our own files: never any file of the
# title, and no third-party binary (the launcher fetches Microsoft's Agility SDK and AMD's FSR DLLs from
# their official sources on the first start, Install-KlbfComponent in launcher-menu.ps1).
#
#   .\scripts\package-release.ps1 -Tag v1.0.0 -Victory target\diag\victory.exe -Out dist
#
# The zip: victory.exe, play.ps1, launcher-menu.ps1, "KLBF Lab.cmd", klbf-lab.ico, README.txt, LICENSE, VERSION and
# SHA256SUMS.txt, at its root (the updater's zip). KLBF-Lab.zip beside it: the same files in a "KLBF Lab" folder.
param(
    [Parameter(Mandatory)][string]$Tag,
    [Parameter(Mandatory)][string]$Victory,
    [Parameter(Mandatory)][string]$Out,
    # Where play.ps1, launcher-menu.ps1, LICENSE and the icon come from (default: this repository)
    [string]$Source
)
$ErrorActionPreference = 'Stop'
# Windows PowerShell 5.1 leaves $PSScriptRoot empty in param defaults (BUILD.md runs it with powershell -File)
if (-not $Source) { $Source = Split-Path -Parent $PSScriptRoot }

# (27/11) The same name for every release: the extracted folder keeps it across updates (the version is
# in VERSION, the release tag and the window)
$name = 'KLBF-Lab-win64'
$stage = Join-Path $Out $name
$zip = Join-Path $Out "$name.zip"
if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

Copy-Item -LiteralPath $Victory -Destination (Join-Path $stage 'victory.exe')
foreach ($f in 'play.ps1', 'launcher-menu.ps1', 'LICENSE') { Copy-Item -LiteralPath (Join-Path $Source $f) -Destination $stage }
Copy-Item -LiteralPath (Join-Path $Source 'apps\victory\klbf-lab.ico') -Destination $stage

# The starter, the same text the launcher writes beside a desktop shortcut
. (Join-Path $Source 'launcher-menu.ps1')
Set-Content -LiteralPath (Join-Path $stage 'KLBF Lab.cmd') -Value (Get-KlbfStarterText) -Encoding ascii

Set-Content -LiteralPath (Join-Path $stage 'README.txt') -Encoding ascii -Value @"
KLBF Lab $Tag (Windows, 64-bit)

1. Extract anywhere.
2. Open the "KLBF Lab" folder and double-click "KLBF Lab.cmd" (Windows may hide the ".cmd" part). Later, use the desktop shortcut it can create.
3. Choose your game folder (your own copy).
4. Play.

Windows SmartScreen: KLBF Lab is not signed. If Windows says it protected your PC, click
"More info" then "Run anyway". If it asks whether to open "KLBF Lab", click Open.

If it doesn't start (nothing happens after double-clicking "KLBF Lab.cmd"):
1. Right-click the zip, Properties, tick "Unblock", OK, then extract it again.
2. Or right-click play.ps1, "Run with PowerShell".
When the launcher itself fails, a message says why, and the details are in
Saved Games\KLBF Lab\logs\launcher-error.txt (attach it to a bug report).

The first start downloads two official components once, into this folder, each file checked:
Microsoft's DirectX 12 Agility SDK (from Microsoft's NuGet) and AMD FSR (AMD FidelityFX SDK v2.4.0,
from AMD's GitHub), with their licenses beside them. Offline, AMD FSR is skipped and the built-in
upscaler is used; start again online to get it.

Nothing of the game is included: KLBF Lab only runs your own copy. KLBF Lab itself: see LICENSE.
Source and help: https://github.com/PastiiJay/klbf-lab
Prefer to build it yourself? See BUILD.md in the repository.
"@

# The version the launcher shows and compares with GitHub's latest release (Get-KlbfVersion)
Set-Content -LiteralPath (Join-Path $stage 'VERSION') -Value $Tag -Encoding ascii

# Every file's hash, sha256sum style (sha256sum -c SHA256SUMS.txt checks them)
$files = Get-ChildItem -LiteralPath $stage -File | Sort-Object Name
$sums = $files | ForEach-Object { '{0} *{1}' -f (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLower(), $_.Name }
[IO.File]::WriteAllText((Join-Path $stage 'SHA256SUMS.txt'), ($sums -join "`n") + "`n")

Add-Type -AssemblyName System.IO.Compression.FileSystem
[IO.Compression.ZipFile]::CreateFromDirectory($stage, $zip, [IO.Compression.CompressionLevel]::Optimal, $false)

# (v26.11.4) KLBF-Lab.zip, for people: the same files inside a top folder "KLBF Lab\" (extracting the flat zip
# spills them loose). KLBF-Lab-win64.zip stays flat: the updater (v26.11.2 on) downloads that one.
$folderRoot = Join-Path $Out 'folder'
$folder = Join-Path $folderRoot 'KLBF Lab'
$zip2 = Join-Path $Out 'KLBF-Lab.zip'
foreach ($p in $folderRoot, $zip2) { if (Test-Path -LiteralPath $p) { Remove-Item -LiteralPath $p -Recurse -Force } }
New-Item -ItemType Directory -Force -Path $folderRoot | Out-Null
Copy-Item -LiteralPath $stage -Destination $folder -Recurse
[IO.Compression.ZipFile]::CreateFromDirectory($folder, $zip2, [IO.Compression.CompressionLevel]::Optimal, $true)
# Only the flat zip's path is returned (release.yml and tests use it); KLBF-Lab.zip is beside it.
$zip
