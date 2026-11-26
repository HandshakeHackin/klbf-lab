# KLBF Lab launcher: the exact settings of our measured runs (RX 9070 XT, Series S mode + AMD FSR).
#   .\play.ps1 -Prebuild        once after each build: pre-builds every pipeline (rerun by itself after a driver crash)
#   .\play.ps1                  play
# Options: -GameDir <folder with the title's files>, -Exe <the title's exe, default: the largest .exe there>,
#          -FfxDir <FidelityFX SDK signedbin folder>, -SeriesX (Series X mode, no FSR needed),
#          -LowMemory (textures capped at 512 px instead of 1024: fewer freezes when the card or PC memory is full, softer textures).
param(
  [string]$GameDir = 'C:\Games\Title',
  [string]$Exe = '',
  [string]$FfxDir = 'C:\FidelityFX-SDK\Kits\FidelityFX\signedbin',
  [switch]$Prebuild,
  [switch]$SeriesX,
  [switch]$LowMemory
)
$ErrorActionPreference = 'Stop'
$victory = Join-Path $PSScriptRoot 'target\release\victory.exe'
if (-not (Test-Path $victory)) { throw "Build first: cargo build --release ($victory not found)" }
if (-not (Test-Path $GameDir)) { throw "Game folder not found: $GameDir (use -GameDir)" }
if (-not $Exe) { $Exe = (Get-ChildItem $GameDir -Filter *.exe | Sort-Object Length -Descending | Select-Object -First 1).FullName }

# The same settings for pre-build and play: the shader cache key depends on them.
# The three adapter defaults can be overridden independently for a hardware retest.
$adapterOverrides = @('VICTORY_AHEAD_FIRST', 'VICTORY_CARD_FLAGGED', 'VICTORY_PASSES_AHEAD')
Get-ChildItem Env:VICTORY_* -ErrorAction SilentlyContinue |
  Where-Object { $_.Name -notin $adapterOverrides } | Remove-Item
$set = [ordered]@{
  VICTORY_GAME_DIR = $GameDir
  VICTORY_OVERLAY = '0'
  VICTORY_SKIP_PIPELINES = (Get-Content (Join-Path $PSScriptRoot 'docs\klbf\skip-amd.txt') -Raw).Trim()
  VICTORY_CARD_WINDOWS_MB = '4096'; VICTORY_BAKE_TEXTURES_MS = '10000'; VICTORY_EARLY_TEXTURES_MS = '40'
  VICTORY_WAVE_OWN_LDS_GROUPS = '64,128'; VICTORY_WIDE_MERGE = '1'; VICTORY_RELAY_OWN_QUEUE = '1'
  VICTORY_UNDER_WRITES_WORDS = '1'; VICTORY_UNDER_WRITES_RUNS = '100000'; VICTORY_UNDER_WRITES_MASKS = '1'
  VICTORY_UNDER_WRITES_MASKS_LEAST = '1'; VICTORY_RELAY = '2'; VICTORY_WIDE_REFRESH = '1'; VICTORY_ASK_ONCE = '1'
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
if ($LowMemory) { $set['VICTORY_TEXTURE_MAX_DIM'] = '512' }
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
