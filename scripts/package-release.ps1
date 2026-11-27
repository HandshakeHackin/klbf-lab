# KLBF Lab: the players' zip (release.yml, and a local dry run). Only our own files: never any file of the
# title, and no third-party binary (the launcher fetches Microsoft's Agility SDK and AMD's FSR DLLs from
# their official sources on the first start, Install-KlbfComponent in launcher-menu.ps1).
#
#   .\scripts\package-release.ps1 -Tag v1.0.0 -Victory target\diag\victory.exe -Out dist
#
# The zip: victory.exe, play.ps1, launcher-menu.ps1, "KLBF Lab.vbs", klbf-lab.ico, README.txt, LICENSE, VERSION and
# SHA256SUMS.txt, at its root (Windows' "Extract All" makes the folder).
param(
    [Parameter(Mandatory)][string]$Tag,
    [Parameter(Mandatory)][string]$Victory,
    [Parameter(Mandatory)][string]$Out,
    # Where play.ps1, launcher-menu.ps1, LICENSE and the icon come from (default: this repository)
    [string]$Source = (Split-Path -Parent $PSScriptRoot)
)
$ErrorActionPreference = 'Stop'

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
Set-Content -LiteralPath (Join-Path $stage 'KLBF Lab.vbs') -Value (Get-KlbfVbsText) -Encoding ascii

Set-Content -LiteralPath (Join-Path $stage 'README.txt') -Encoding ascii -Value @"
KLBF Lab $Tag (Windows, 64-bit)

1. Extract anywhere.
2. Double-click "KLBF Lab.vbs" (Windows may hide the ".vbs" part: it is the file with the script icon). Later, use the desktop shortcut it can create.
3. Choose your game folder (your own copy).
4. Play.

Windows SmartScreen: KLBF Lab is not signed. If Windows says it protected your PC, click
"More info" then "Run anyway". If it asks whether to open "KLBF Lab", click Open.

The first start downloads two official components once, into this folder, each file checked:
Microsoft's DirectX 12 Agility SDK (from Microsoft's NuGet) and AMD FSR (AMD FidelityFX SDK v2.4.0,
from AMD's GitHub), with their licenses beside them. Offline, AMD FSR is skipped and the built-in
upscaler is used; start again online to get it.

Nothing of the game is included: KLBF Lab only runs your own copy. KLBF Lab itself: see LICENSE.
Source and help: https://github.com/PastiiJay/klbf-lab
"@

# The version the launcher shows and compares with GitHub's latest release (Get-KlbfVersion)
Set-Content -LiteralPath (Join-Path $stage 'VERSION') -Value $Tag -Encoding ascii

# Every file's hash, sha256sum style (sha256sum -c SHA256SUMS.txt checks them)
$files = Get-ChildItem -LiteralPath $stage -File | Sort-Object Name
$sums = $files | ForEach-Object { '{0} *{1}' -f (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLower(), $_.Name }
[IO.File]::WriteAllText((Join-Path $stage 'SHA256SUMS.txt'), ($sums -join "`n") + "`n")

Add-Type -AssemblyName System.IO.Compression.FileSystem
[IO.Compression.ZipFile]::CreateFromDirectory($stage, $zip, [IO.Compression.CompressionLevel]::Optimal, $false)
$zip
