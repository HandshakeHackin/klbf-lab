# KLBF Lab launcher: the exact settings of our measured runs (RX 9070 XT, Series S mode + AMD FSR).
#   .\play.ps1 -Prebuild        once after each build: pre-builds every pipeline (rerun by itself after a driver crash)
#   .\play.ps1                  play
# Options: -GameDir <folder with the title's files>, -Exe <the title's exe, default: the largest .exe there>,
#          -FfxDir <FidelityFX SDK signedbin folder>, -SeriesX (Series X mode, no FSR needed),
#          -RenderS (with -SeriesX, report Series S ConsoleMode while retaining the Series X profile),
#          -LowMemory (textures capped at 512 px instead of 1024: fewer freezes when the card or PC memory is full, softer textures),
#          -SkipMovies open|end|fast (skips the opening movies),
#          -NoAgility (skip the D3D12 Agility SDK check), -Yes (fetch it without asking).
# Bench: $env:KLBF_BENCH = 'bm-v1' before .\play.ps1 (5:00 measured from the first world frame, input locked).
param(
  [string]$GameDir = 'C:\Games\Title',
  [string]$Exe = '',
  [string]$FfxDir = 'C:\FidelityFX-SDK\Kits\FidelityFX\signedbin',
  [switch]$Prebuild,
  [switch]$SeriesX,
  [switch]$LowMemory,
  [string]$SkipMovies = '',
  [switch]$NoAgility,
  [switch]$Yes,
  [switch]$RenderS
)
$ErrorActionPreference = 'Stop'
$victory = Join-Path $PSScriptRoot 'target\release\victory.exe'
if (-not (Test-Path $victory)) { throw "Build first: cargo build --release ($victory not found)" }
if (-not (Test-Path $GameDir)) { throw "Game folder not found: $GameDir (use -GameDir)" }

# victory.exe loads Microsoft's D3D12 Agility SDK from D3D12\ beside it (Windows 10's in-box D3D12
# refuses our root signatures, #11 #85); without it the game does not start. Fetched once, with consent.
$agility = Join-Path (Split-Path $victory) 'D3D12'
if (-not $NoAgility -and -not (Test-Path (Join-Path $agility 'D3D12Core.dll'))) {
  $ver = '1.620.2'
  $url = "https://api.nuget.org/v3-flatcontainer/microsoft.direct3d.d3d12/$ver/microsoft.direct3d.d3d12.$ver.nupkg"
  if (-not $Yes) {
    $a = Read-Host "Microsoft's D3D12 Agility SDK $ver is missing next to victory.exe (needed to start). Download it from nuget.org? (Y/n)"
    if ($a -and $a -notmatch '^[yY]') { throw "Agility SDK needed: put D3D12Core.dll from Microsoft.Direct3D.D3D12 1.620.x into $agility (or use -NoAgility)" }
  }
  $tmp = Join-Path ([IO.Path]::GetTempPath()) "klbf-agility-$PID"
  New-Item -ItemType Directory -Force $tmp | Out-Null
  try {
    $pkg = Join-Path $tmp 'agility.zip'
    Invoke-WebRequest -UseBasicParsing $url -OutFile $pkg
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($pkg)
    try {
      foreach ($n in 'D3D12Core.dll', 'd3d12SDKLayers.dll') {
        $e = $zip.GetEntry("build/native/bin/x64/$n")
        if (-not $e) { throw "$n not in the package" }
        $out = Join-Path $tmp $n
        [IO.Compression.ZipFileExtensions]::ExtractToFile($e, $out, $true)
        $sig = Get-AuthenticodeSignature $out
        if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notmatch 'Microsoft') {
          throw "$n is not validly signed by Microsoft ($($sig.Status)): refused"
        }
      }
    } finally { $zip.Dispose() }
    New-Item -ItemType Directory -Force $agility | Out-Null
    foreach ($n in 'D3D12Core.dll', 'd3d12SDKLayers.dll') { Move-Item -Force (Join-Path $tmp $n) $agility }
    "Agility SDK $ver installed in $agility"
  } finally { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
}
if (-not $Exe) { $Exe = (Get-ChildItem $GameDir -Filter *.exe | Sort-Object Length -Descending | Select-Object -First 1).FullName }

# The same settings for pre-build and play: the shader cache key depends on them.
# Preserve explicit adapter overrides for hardware retests. With none set, the
# executable chooses its defaults using the D3D12 adapter it actually opened.
$adapterOverrides = @('VICTORY_AHEAD_FIRST', 'VICTORY_CARD_FLAGGED', 'VICTORY_PASSES_AHEAD', 'VICTORY_RELAY')
Get-ChildItem Env:VICTORY_* -ErrorAction SilentlyContinue |
  Where-Object { $_.Name -notin $adapterOverrides } | Remove-Item
$set = [ordered]@{
  VICTORY_GAME_DIR = $GameDir
  VICTORY_OVERLAY = '0'
  VICTORY_SKIP_PIPELINES = (Get-Content (Join-Path $PSScriptRoot 'docs\klbf\skip-amd.txt') -Raw).Trim()
  VICTORY_CARD_WINDOWS_MB = '4096'; VICTORY_BAKE_TEXTURES_MS = '10000'; VICTORY_EARLY_TEXTURES_MS = '40'
  VICTORY_WAVE_OWN_LDS_GROUPS = '64,128'; VICTORY_WIDE_MERGE = '1'; VICTORY_RELAY_OWN_QUEUE = '1'
  VICTORY_UNDER_WRITES_WORDS = '1'; VICTORY_UNDER_WRITES_RUNS = '100000'; VICTORY_UNDER_WRITES_MASKS = '1'
  VICTORY_UNDER_WRITES_MASKS_LEAST = '1'; VICTORY_WIDE_REFRESH = '1'; VICTORY_ASK_ONCE = '1'
  VICTORY_TEXTURE_NO_WAIT = '1'; VICTORY_CARD_REHASH_RUNS = '1'; VICTORY_JOIN_COPIES = '1'; VICTORY_D3D12_BACKGROUND = '0'
  VICTORY_LATE_WAIT_LISTS = '1024'; VICTORY_WINDOW_TAILS = '1'; VICTORY_TEXTURE_STAGE_MB = '512'
  VICTORY_CARD_BUFFERS_MIN_KB = '64'; VICTORY_ASK_QUIET = '8'; VICTORY_COPY_CHANGED = '1'; VICTORY_QUIET_BACK = '1'
  VICTORY_LIST_POOL_MOST = '2048'; VICTORY_LIST_POOL_MB = '2048'; VICTORY_WIDE_THREADS = '12'
  VICTORY_INDIRECT_RAW = '0'; VICTORY_STAGE_POOL_MB = '2048'; VICTORY_TEXTURE_HASH_EVERY = '32'
  VICTORY_LEVELS_ON_DEMAND = '1'; VICTORY_WINDOW_KB = '1024'; VICTORY_CARD_BUFFERS_MB = '1536'
  VICTORY_CARD_WINDOW_UPLOAD = '1'; VICTORY_DROP_DXIL = '1'; VICTORY_DESC_DATA = '1'
  VICTORY_VRAM_IDLE_MS = '15000,2500,1000'; VICTORY_PIPELINE_WAIT_MS = '2500'; VICTORY_OCEAN_READBACK = '1'
  VICTORY_WATCH_4K = '1'; VICTORY_THIN = '1'
  VICTORY_CLAMP_AFTER_WRITER = '1'
  VICTORY_PORT_CONSTANTS = '1'; VICTORY_PORT_DESCRIPTORS = '1'
  VICTORY_DYNAMIC_SLOTS = '786432'
  VICTORY_TEXTURE_MAX_DIM = '1024'; VICTORY_LIBRARY_LOCK = '0'
}
if ($RenderS) { $set['VICTORY_RENDER_S'] = '1' }
if ($LowMemory) { $set['VICTORY_TEXTURE_MAX_DIM'] = '512' }
if ($SkipMovies) { $set['VICTORY_SKIP_MOVIES'] = $SkipMovies }
$console = @()
$fsr = Join-Path $FfxDir 'amd_fidelityfx_upscaler_dx12.dll'
if (-not $SeriesX) {
  if (-not (Test-Path $fsr)) { throw "AMD FSR not found: $fsr (use -FfxDir, or -SeriesX)" }
  $set['VICTORY_FSR_DLL'] = $fsr
  $console = @('--console', 'series-s')
}
foreach ($k in $set.Keys) { Set-Item -Path "Env:$k" -Value $set[$k] }

if ($Prebuild) {
  $shaders = Join-Path $GameDir 'shaders.rpf'
  for ($i = 1; $i -le 120; $i++) {
    & $victory translate --cache --pipelines --jobs 8 $shaders
    if ($LASTEXITCODE -eq 0) { "Pre-build done ($i pass(es)). Now run .\play.ps1"; exit 0 }
    "Pass $i ended with code $LASTEXITCODE (driver crash?): resuming..."
    Start-Sleep 5
  }
  throw 'Pre-build did not finish after 120 passes'
}
& $victory run $Exe --execute @console
