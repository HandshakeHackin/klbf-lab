# KLBF Lab: starts the title in Profile X with the settings of Op-lite/victory-scarlett's
# scripts\play-scarlett.ps1 (same set; only the cache and save folders are KLBF Lab's own).
#
#   .\play.ps1 -Game 'D:\Games\MyDump'
#
# Started with no settings switch, a setup window asks how to play (profile, upscaler and preset, output,
# frame generation; saved profiles) on the first start, and the choice is reused on later starts without it.
# -Setup (or holding Shift while it starts) opens it again. -NoMenu skips it (the saved choice, else the
# default: Profile X, FSR 4 Performance, 3840x2160, frame generation, hooks and texture trim). Any switch
# below but -Game, -Build, -Victory, -Cache, -Data and -FfxDir skips it too and runs exactly what is asked.
# -PrintEnv prints what would be run (settings and environment) and stops, without starting anything.
#
# -Game        your own dump: the folder with game_scarlett_master.exe (or that file itself).
#              Remembered in game-folder.txt next to the repository's README, so later starts need no -Game.
# -Build       diag (default: the build all measurements are of) or release
# -Victory     another victory.exe than target\<Build>\victory.exe
# -Cache       shader and pipeline caches (default: Saved Games\KLBF Lab\cache). A first run fills them
#              and is slow; faces can be missing in that first run and are right from the second.
# -Data        saves (default: Saved Games\KLBF Lab\data)
# -RayTracing  the title's own ray tracing (VICTORY_BVH_BUILDER=1): better light, much slower. Off by default.
# -Fsr        Profile S with AMD FSR 4 (the title's own upscaler, sent to the PC's FSR DLL in -FfxDir).
# -FsrX       Profile X with AMD FSR 4 (KLBF_X_FSR=1): the title is made to use its FidelityFX path, sent to
#              the PC's FSR DLL in -FfxDir. -FsrQuality quality (default, 1/1.5 of the output) | balanced (1/1.7)
#              | performance (1/2) | ultraperformance (1/3), AMD's presets. -Output 2560x1440 sets the title's output (one of its modes; 3840x2160 without).
# -FrameGen    AMD FSR frame generation on top (KLBF_FRAMEGEN=1), with -FsrX or -Fsr only: two frames shown per
#              real one. AMD recommends a base of 60 fps; at ~16 real fps it adds latency and artifacts.
# -FfxDir      the folder with AMD's signed FidelityFX DLLs (default D:\KLBF-Lab\ffx-4.1.2: SDK 2.4.0, FSR 4.1.2).
# -Bench      KLBF Lab Bench BM-v1 (5 min, hands-off; your morning save in slot 1): Continue pressed for you.
#              -BenchPlayer <GitHub login> names you on the card (else anonymous).
#              -BenchLength Short (5 min, the board's test) | Medium (15) | Long (30) | Marathon (60), or the minutes: -BenchLength 60. Same place, longer.
# -AutoStart   presses through the start screens by itself, for a fresh start with no save
# -SkipMovies  skips the opening movies (off by default): all or open (none opens), end (each jumps to its
#              last frame), fast (decoded without waiting). VICTORY_SKIP_MOVIES, by zocram4cc.
# -NoProbeRays the base's picture with ray tracing off: bushes, trees and palms glow violet. By default the
#              title's light probes are activated and every ray misses, as with an empty ray-tracing
#              structure (no cost measured); the first start after a change of this re-translates shaders.
# -Hooks       with the memory hooks: faster, but they can show a few small textures with old contents for a
#              moment. Off by default until that is fixed.
# -TexTrim     KLBF_TEX_TRIM=1: pool pages of textures whose card copy is alive are given back while RAM is low
#              (needs -Hooks).
# -Tout        everything at once, for a test: -FsrX (quality unless -FsrQuality), -FrameGen, -Hooks and -TexTrim,
#              with the DLLs of -FfxDir. Each switch stays usable alone for A/B.
# -NoPrecompile  skip the step below
# -OnlyPrecompile  do the step below and stop, without starting the game
# -More        other settings, last word: -More @{ VICTORY_LOG = 'warn' }
#
# Before the game starts, the game's shaders are translated ahead (victory.exe shaders <shaders.rpf>) whenever
# that is needed: on the first start, after a new build of victory.exe, and after a change of a setting that
# changes the translation (-RayTracing, -NoProbeRays). It takes a few minutes and is remembered in the cache
# folder, so later starts go straight to the game. Without it the first session translates while playing:
# slow, and faces can be drawn wrong for that session.
#
# Run it from a normal window, not as administrator.
param(
    [string]$Game = '',
    [ValidateSet('diag', 'release')][string]$Build = 'diag',
    [string]$Victory = '',
    [string]$Cache = '',
    [string]$Data = '',
    [switch]$RayTracing,
    [switch]$Fsr,
    [switch]$FsrX,
    [switch]$FrameGen,
    [ValidateSet('quality', 'balanced', 'performance', 'ultraperformance')][string]$FsrQuality = 'quality',
    [ValidateSet('', '1280x720', '1664x936', '1920x1080', '2560x1440', '3840x2160')][string]$Output = '',
    [string]$FfxDir = 'D:\KLBF-Lab\ffx-4.1.2',
    [switch]$AutoStart,
    [switch]$Bench,
    [string]$BenchPlayer = 'anonymous',
    [ValidateSet('Short', 'Medium', 'Long', 'Marathon', '5', '15', '30', '60')]
    [string]$BenchLength = 'Short',
    [ValidateSet('open', 'end', 'fast', 'all')][string]$SkipMovies = '',
    [switch]$NoProbeRays,
    [switch]$ProbeRays,
    [switch]$Hooks,
    [switch]$NoHooks,
    [switch]$TexTrim,
    [switch]$Tout,
    [switch]$NoPrecompile,
    [switch]$OnlyPrecompile,
    [hashtable]$More = @{},
    [switch]$Setup,
    [switch]$NoMenu,
    [switch]$PrintEnv
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'launcher-menu.ps1')
# (27/11) First start with the Saved Games home: the older AppData folder's saves, settings, backups and
# cache are COPIED into it once (the older folder is left as it was).
if (-not $PrintEnv -and -not (Test-Path -LiteralPath (Get-KlbfHome))) {
    try { $moved = Move-KlbfOldData; if ($moved) { Write-Host $moved } } catch { Write-Warning "Older KLBF Lab folder not copied: $_" }
}
# The setup window's choice (launcher-menu.ps1), only when no settings switch is given.
$menuAction = ''
$passOn = @{}
foreach ($k in 'Game', 'Build', 'Victory', 'Cache', 'Data', 'FfxDir') { if ($PSBoundParameters.ContainsKey($k)) { $passOn[$k] = $PSBoundParameters[$k] } }
$notSettings = @($passOn.Keys) + @('Setup', 'NoMenu', 'PrintEnv')
$explicit = @($PSBoundParameters.Keys | Where-Object { $_ -notin $notSettings })
if (-not $explicit) {
    $settingsFile = Join-Path (Get-KlbfLauncherDir) 'settings.json'
    $choice = Read-KlbfSettings $settingsFile
    $ask = -not $NoMenu -and -not $PrintEnv -and ($Setup -or -not $choice -or (Test-KlbfShift))
    # (27/11) No settings yet: the window starts from the setting that works for everyone (built-in upscaler,
    # output fitting the screen); -NoMenu keeps the owner's setting, so scripts and benches are unchanged
    if (-not $choice) { $choice = if ($ask) { try { Get-KlbfUniversal (Get-KlbfHardware) } catch { Get-KlbfDefaults } } else { Get-KlbfDefaults } }
    if ($ask) {
        # One line on the shaders: prepared for this build or not (the window's choices do not change them).
        $status = ''
        try {
            $v = if ($Victory) { $Victory } else { Join-Path $PSScriptRoot "target\$Build\victory.exe" }
            $m = Join-Path (Get-KlbfCacheDir $Cache) 'scarlett-shaders-ahead.txt'
            $ready = (Test-Path -LiteralPath $m) -and ((Get-Content -LiteralPath $m -TotalCount 1) -eq (Get-KlbfShaderStamp $v $Build '1' ''))
            $status = if ($ready) { 'Shaders: ready for this build.' } else { 'Shaders: not prepared yet (Play offers it).' }
        } catch {}
        $known = if ($Game) { $Game } elseif (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'game-folder.txt')) { (Get-Content -LiteralPath (Join-Path $PSScriptRoot 'game-folder.txt') -TotalCount 1).Trim() } else { '' }
        $picked = Show-KlbfMenu $choice $status $v $(if ($Data) { $Data } else { Join-Path (Get-KlbfHome) 'data' }) $known $Cache
        if (-not $picked) { exit 0 }
        $choice = $picked.choice; $menuAction = $picked.action
        Save-KlbfSettings $choice $settingsFile
        # (27/11) Once, after the first setup: a desktop shortcut for this build
        if ($menuAction -eq 'play') {
            try { $null = Invoke-KlbfShortcutOffer (Get-KlbfShortcutSpec $PSScriptRoot $Victory) { param($t) [System.Windows.Forms.MessageBox]::Show($t, 'KLBF Lab', 'YesNo', 'Question') -eq 'Yes' } } catch {}
        }
    }
    # "Run benchmark": the player's settings and -Bench (-BenchPlayer from the bench window)
    $switches = if ($menuAction -eq 'bench') { Get-KlbfBenchSwitches $choice $picked.benchPlayer } else { Get-KlbfSwitches $choice }
    foreach ($kv in $switches.GetEnumerator()) { Set-Variable -Name $kv.Key -Value $kv.Value }
    # The game folder chosen in the window, unless -Game says another
    if (-not $PSBoundParameters.ContainsKey('Game') -and $choice.game) { $Game = $choice.game }
}
if ($Tout) { $FsrX = [switch]$true; $FrameGen = [switch]$true; $Hooks = [switch]$true; $TexTrim = [switch]$true }
$root = $PSScriptRoot
$remembered = Join-Path $root 'game-folder.txt'

if (-not $Game -and (Test-Path -LiteralPath $remembered)) { $Game = (Get-Content -LiteralPath $remembered -TotalCount 1) }
if (-not $Game) { throw "Say where your dump is: .\play.ps1 -Game 'D:\Games\MyDump'" }
$Game = $Game.Trim().Trim('"')
if ((Test-Path -LiteralPath $Game -PathType Leaf) -and $Game.EndsWith('.exe')) { $Game = Split-Path -Parent $Game }
$exe = Join-Path $Game 'game_scarlett_master.exe'
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "No game_scarlett_master.exe in $Game. -Game is the folder of your dump." }
Set-Content -LiteralPath $remembered -Value $Game -Encoding utf8

if (-not $Victory) { $Victory = Join-Path $root "target\$Build\victory.exe" }
if (-not (Test-Path -LiteralPath $Victory -PathType Leaf)) {
    throw "No $Victory. Build it first: cargo build --profile diag -p victory --features diagnostics --locked"
}
# The exe asks for the DirectX 12 Agility SDK 1.620.2 in D3D12\ beside it (Windows' own runtime is older).
# A source build has no such folder: fetched once from Microsoft's NuGet package (27/11, #82).
$agility = Join-Path (Split-Path -Parent $Victory) 'D3D12'
if (-not (Test-Path -LiteralPath (Join-Path $agility 'D3D12Core.dll'))) {
    Write-Host 'Fetching the DirectX 12 Agility SDK 1.620.2 (Microsoft NuGet, once)...'
    $pkg = Join-Path $env:TEMP 'klbf-agility-1.620.2.zip'
    $tmp = Join-Path $env:TEMP 'klbf-agility-1.620.2'
    Invoke-WebRequest -UseBasicParsing 'https://www.nuget.org/api/v2/package/Microsoft.Direct3D.D3D12/1.620.2' -OutFile $pkg
    Expand-Archive -LiteralPath $pkg -DestinationPath $tmp -Force
    New-Item -ItemType Directory -Force -Path $agility | Out-Null
    Copy-Item (Join-Path $tmp 'build\native\bin\x64\D3D12Core.dll'), (Join-Path $tmp 'build\native\bin\x64\d3d12SDKLayers.dll') $agility
    Remove-Item -Recurse -Force $pkg, $tmp
}
if (-not $Cache) { $Cache = Get-KlbfCacheDir '' }
if (-not $Data) { $Data = Join-Path (Get-KlbfHome) 'data' }
# (27/11) Never a new game over saves left in an older folder: said, and nothing starts
if (-not $PrintEnv -and -not $Bench) { $lost = Test-KlbfLostSaves $Data; if ($lost) { throw $lost } }
New-Item -ItemType Directory -Force -Path $Cache, $Data | Out-Null
# (27/11) A bench from the window loads the save picked there: the run gets a copy of the data folder in which
# that save is the only one (Continue can load nothing else), and the player's own saves are not opened.
$benchLabel = ''
if ($menuAction -eq 'bench' -and $picked.benchSave) {
    try {
        $zip = Backup-KlbfSaves (Get-KlbfSavesRoot $Data)
        if ($zip) { Write-Host "Saves backed up: $zip" }
    } catch { Write-Warning "Saves not backed up: $_" }
    $benchLabel = Get-KlbfSaveLabel $picked.benchSave
    $Data = New-KlbfBenchData $Data $picked.benchSave
    Write-Host "KLBF Lab Bench: save $benchLabel, alone in a copy of your data ($Data)"
}

# Settings left in this window by an earlier start go: only what is below counts.
Get-ChildItem Env: | Where-Object { $_.Name -like 'VICTORY_*' -and $_.Name -ne 'VICTORY_HALF_VIEWS' -and $_.Name -ne 'VICTORY_AUTO_INPUT' } | ForEach-Object { Remove-Item "Env:$($_.Name)" }
Get-ChildItem Env: | Where-Object { $_.Name -in 'KLBF_X_FSR', 'KLBF_X_FSR_QUALITY', 'KLBF_X_OUTPUT', 'KLBF_FRAMEGEN', 'KLBF_TEX_TRIM', 'KLBF_BENCH_SAVE' } | ForEach-Object { Remove-Item "Env:$($_.Name)" }

$set = [ordered]@{
    VICTORY_GAME_DIR  = $Game
    VICTORY_CACHE_DIR = $Cache
    VICTORY_DATA_DIR  = $Data

    # The standing set of this fork's runs (some names come from its older tree and are ignored here)
    VICTORY_JOIN_COPIES = '1'; VICTORY_REFRESH_ONCE = '1'; VICTORY_CARD_REHASH_RUNS = '1'; VICTORY_BACK_OWN_VIEW = '1'
    VICTORY_WIDE_MERGE = '1'; VICTORY_MIRROR_FAST = '1'; VICTORY_FRAME_LOG = '1'; VICTORY_CARD_METER = '1'
    VICTORY_PIPELINE_WAIT_MS = '500'; VICTORY_ASYNC_SUBMIT = '1'; VICTORY_QUEUE2 = '1'; VICTORY_INDIRECT_WINDOW = '1'
    VICTORY_TEXTURE_NO_WAIT = '1'; VICTORY_COPY_PLAIN = '1'; VICTORY_WIDE_TEXTURES = '1'; VICTORY_INDIRECT_GATHER = '1'
    VICTORY_LATE_WAIT_LISTS = '1024'; VICTORY_CARD_FILLS_BATCH = '0'; VICTORY_GATHER_DISPATCH = '0'

    # cool_fox's launcher set, without Series S, VICTORY_THIN and the stand-in ambient light; the keep-busy loop off
    VICTORY_RELAY = '2'; VICTORY_WIDE_BACK = '1'; VICTORY_RELAY_COPY = '1'; VICTORY_CARD_STEPS = '2'
    VICTORY_INDIRECT_BATCH = '2'; VICTORY_MAP_THREADS = '1'; VICTORY_CARD_AWAKE = '0'; VICTORY_SHOW_AFTER = '1'
    VICTORY_JOIN_PARTS = '8'; VICTORY_UPLOAD_AHEAD = '1'; VICTORY_CARD_BUFFERS_MIN_KB = '64'

    # Textures keep only the levels their views show (the base's default; cool_fox's launcher turned it off)
    VICTORY_LEVELS_ON_DEMAND = '0'

    # This fork's memory and upload switches
    VICTORY_POOL_DISCARD = '1'; VICTORY_FILL_TITLE = '1'; VICTORY_PARTIAL_DELTA = '1'; VICTORY_BAKE_DELTA = '1'
    VICTORY_MAP_LOCK_FINE = '1'; VICTORY_POOL_CAP = '1'

    # Draw set-up: table entries done earlier in a list are not done again; an aliased texture's copy follows its
    # source's writes within a list (a stale read in the base)
    VICTORY_TABLE_MEMO = '1'; VICTORY_LIST_ALIAS_EXACT = '1'

    # (KLBF Lab v12, 27/11) Every descriptor slot the title writes is passed on in the same frame (Yaekai's
    # 8 GB caps left textures black and roads dark on a 16 GB card)
    VICTORY_HEAP_CHANGED_MAX = '0'; VICTORY_HEAP_PASS_MAX = '0'

    # (KLBF Lab, 27/11) Every pipeline the title creates is written down, so the launcher makes them ahead next time
    VICTORY_PIPELINE_BOOK = '1'

    # (KLBF Lab v12, 27/11) Left off after a real play session showed characters in a T-pose, black textures
    # and missing buildings with them on, and a clean picture with them off (one of them is the cause; being
    # narrowed down): VICTORY_LIST_HOLD, VICTORY_WINDOWS_BAR_MB, VICTORY_PORT_CONSTANTS, VICTORY_SOFT_VIEWS,
    # VICTORY_SAMPLERS_MADE, VICTORY_TABLE_NAMED_ONLY, VICTORY_QUEUE_MARKS

    # Picture: the title's character bake waits up to 10 s for its textures (1.5 s by default), and a draw
    # waits up to 500 ms for a pipeline still being made (above) instead of being dropped after 20 ms
    VICTORY_BAKE_TEXTURES_MS = '10000'

    # Picture: the material-class depth plane is current before the title's tiling pass reads it (the base
    # gave it last frame's)
    VICTORY_PLANE_NOW = '1'
}
if ($Hooks -and -not $NoHooks) {
    # The memory hooks
    $set.VICTORY_GPU_ONLY = '1'; $set.VICTORY_KNOWN_WRITES = '1'
}
if ($TexTrim) {
    if (-not $Hooks -or $NoHooks) { throw '-TexTrim needs -Hooks (KLBF_TEX_TRIM refuses without VICTORY_GPU_ONLY=1).' }
    $set.KLBF_TEX_TRIM = '1'
}
if ($Output) { $set.KLBF_X_OUTPUT = $Output }
if ($FsrX) {
    # (KLBF Lab, 27/11) The title's FidelityFX path in Series X, to the PC FSR DLL
    $fsrDll = Join-Path $FfxDir 'amd_fidelityfx_upscaler_dx12.dll'
    if (-not (Test-Path -LiteralPath $fsrDll)) { throw "AMD FSR not found: $fsrDll (use -FfxDir)" }
    $set.VICTORY_FSR_DLL = $fsrDll; $set.KLBF_X_FSR = '1'; $set.KLBF_X_FSR_QUALITY = $FsrQuality
}
if ($FrameGen) {
    # (KLBF Lab, 27/11) AMD's frame generation, fed by the FSR upscale: its DLLs beside the upscaler's
    if (-not $FsrX -and -not $Fsr) { throw '-FrameGen needs -FsrX (Profile X) or -Fsr (Profile S): it is fed by the FSR upscale.' }
    foreach ($dll in 'amd_fidelityfx_loader_dx12.dll', 'amd_fidelityfx_framegeneration_dx12.dll') {
        if (-not (Test-Path -LiteralPath (Join-Path $FfxDir $dll))) { throw "AMD frame generation needs $dll in $FfxDir" }
    }
    $set.KLBF_FRAMEGEN = '1'
}
if (-not $NoProbeRays -and -not $RayTracing) { $set.VICTORY_PROBE_RAYS = '1' }
if ($RayTracing) { $set.VICTORY_BVH_BUILDER = '1' }
if ($AutoStart) {
    $set.VICTORY_AUTO_INPUT = 'd>=15,rep(A,1,d<10),w3,d>=20,w1,rep(A,1,d>=35),w1.5,Down,w7,Down,w0.6,A,w1.5,A,w0.7,Down,w0.5,A,w0.7,Down,w0.5,A'
}
if ($SkipMovies) { $set.VICTORY_SKIP_MOVIES = $(if ($SkipMovies -eq 'all') { 'open' } else { $SkipMovies }) }
# -Bench (KLBF Lab Bench BM-v1, docs/klbf/BENCH-BM-V1.md): the bench on, the movies skipped, and the start
# screens and Continue pressed by themselves (your morning save in slot 1 must be the last one); touch nothing.
if ($Bench) {
    $env:KLBF_BENCH = 'bm-v1'; $env:KLBF_BENCH_PLAYER = $BenchPlayer; $env:KLBF_BENCH_LENGTH = @{ '5' = 'short'; '15' = 'medium'; '30' = 'long'; '60' = 'marathon' }[$BenchLength]; if (-not $env:KLBF_BENCH_LENGTH) { $env:KLBF_BENCH_LENGTH = $BenchLength.ToLower() }
    if (-not $SkipMovies) { $set.VICTORY_SKIP_MOVIES = 'end' }
    $set.VICTORY_AUTO_INPUT = 'rep(A,2,d>=15),rep(A,1,d<10),w3,d>=20,w1,rep(A,1,d>=35),w1.5,Down,w7,Down,w0.6,A,w2,A,w1.5,A,d>=1000'
    Write-Host 'KLBF Lab Bench BM-v1: Continue is pressed for you, then touch nothing until the card shows. Your card is saved in bench-results beside victory.exe;'
    Write-Host 'post it with the form: https://github.com/PastiiJay/klbf-lab/issues/new?template=bench-bm-v1.yml'
}
if ($benchLabel) { $set.KLBF_BENCH_SAVE = $benchLabel }
# (27/11) F8's screenshots: the folder chosen in Settings, else Pictures\KLBF Lab
$saved = Read-KlbfSettings (Join-Path (Get-KlbfLauncherDir) 'settings.json')
$set.KLBF_SHOTS_DIR = Get-KlbfShotsDir $saved
# (27/11) The Display chosen in Settings: the game window opens centred on that monitor (else the main one)
if ($saved -and $saved.display -ge 0) { $set.KLBF_WINDOW_MONITOR = [string]$saved.display }
if ($saved -and $saved.borderless) { $set.KLBF_BORDERLESS = '1' }
foreach ($k in $More.Keys) { $set[$k] = [string]$More[$k] }
foreach ($k in $set.Keys) { Set-Item -Path "Env:$k" -Value $set[$k] }
if ($PrintEnv) {
    $mode = if ($Fsr) { 'Profile S (run --console series-s), VICTORY_FSR_DLL=' + (Join-Path $FfxDir 'amd_fidelityfx_upscaler_dx12.dll') } else { 'Profile X' }
    Write-Output "mode: $mode"
    $on = @(@('Fsr', 'FsrX', 'FrameGen', 'Hooks', 'TexTrim') | Where-Object { (Get-Variable $_).Value })
    if ($FsrX) { $on += "FsrQuality=$FsrQuality" }
    if ($Output) { $on += "Output=$Output" }
    Write-Output ('switches: ' + ($on -join ' '))
    foreach ($k in $set.Keys | Sort-Object) { Write-Output "$k=$($set[$k])" }
    exit 0
}

# Shaders translated ahead, when needed: the mark holds what the translations depend on that this script knows of
# (the build and the settings that change the translation).
$rpf = Join-Path $Game 'shaders.rpf'
$mark = Join-Path $Cache 'scarlett-shaders-ahead.txt'
$stamp = Get-KlbfShaderStamp $Victory $Build $set['VICTORY_PROBE_RAYS'] $set['VICTORY_BVH_BUILDER']
$have = if (Test-Path -LiteralPath $mark) { Get-Content -LiteralPath $mark -TotalCount 1 } else { '' }
# From the setup window: "Prepare shaders", or Play with shaders to prepare (asked: Prepare now / Later).
if ($menuAction -and -not $NoPrecompile -and ($menuAction -eq 'prepare' -or $have -ne $stamp)) {
    if (-not (Test-Path -LiteralPath $rpf -PathType Leaf)) {
        Write-Warning "No shaders.rpf in ${Game}: the shaders will be translated while playing."
    } elseif ($menuAction -eq 'prepare' -or (Confirm-KlbfPrepare (Get-KlbfPrepareReason $have $stamp))) {
        if (Show-KlbfPrepare $Victory $rpf) { Set-Content -LiteralPath $mark -Value $stamp -Encoding utf8 }
    }
    if ($menuAction -eq 'prepare') { & $PSCommandPath -Setup @passOn; exit $LASTEXITCODE }
    $have = $stamp
}
if (-not $NoPrecompile -and $have -ne $stamp) {
    if (Test-Path -LiteralPath $rpf -PathType Leaf) {
        Write-Host 'Translating the game''s shaders ahead (needed once per build and setting; a few minutes)...'
        & $Victory shaders $rpf
        if ($LASTEXITCODE -eq 0) {
            Set-Content -LiteralPath $mark -Value $stamp -Encoding utf8
        } else {
            Write-Warning 'Translating ahead failed; the shaders will be translated while playing instead.'
        }
    } else {
        Write-Warning "No shaders.rpf in ${Game}: the shaders will be translated while playing."
    }
}
# (27/11) Effects made on the loading side, never while playing: every pipeline the title created in earlier runs
# (written down by VICTORY_PIPELINE_BOOK) is made ahead by `victory warm` into the driver's cache, before the game
# starts. Only new ones are made; it goes on where it stopped. -NoPrecompile skips it.
$book = Join-Path $Cache 'pipelines\known-pipelines.bin'
if (-not $NoPrecompile -and (Test-Path -LiteralPath $book)) {
    Write-Host 'Making the effects of earlier runs ahead, so they do not compile while playing (only new ones; resumes where it stopped)...'
    & $Victory warm
    if ($LASTEXITCODE -ne 0) { Write-Warning 'Making effects ahead stopped early; the rest are made while playing.' }
}
if ($OnlyPrecompile) { exit 0 }
# (27/11) The bench starts only on a ready cache: the shaders translated for this build, and the effects made by
# about ten minutes of play (otherwise they compile during the run and the figures are not comparable).
if ($Bench) {
    $have = if (Test-Path -LiteralPath $mark) { Get-Content -LiteralPath $mark -TotalCount 1 } else { '' }
    if ($have -ne $stamp) {
        throw 'KLBF Lab Bench: the game''s shaders are not translated for this build yet. Start once without -Bench (the launcher translates them, a few minutes), then run the bench.'
    }
    $early = Join-Path $Cache 'pipelines\early-pipelines.txt'
    $made = if (Test-Path -LiteralPath $early) { @(Get-Content -LiteralPath $early).Count } else { 0 }
    if ($made -lt 1000) {
        throw "KLBF Lab Bench: only $made effects are built so far. Play about 10 minutes first (same build, without -Bench) so they are made before the bench; it does not start until then."
    }
    Write-Host "KLBF Lab Bench: cache ready ($made effects built, shaders translated)."
}

# (27/11) Every save backed up before the game starts (KLBF-Lab\save-backups, the last 10 kept)
if (-not $benchLabel) { try {
    $savedZip = Backup-KlbfSaves (Get-KlbfSavesRoot $Data)
    if ($savedZip) { Write-Host "Saves backed up: $savedZip" }
} catch { Write-Warning "Saves not backed up: $_" } }
Write-Host ("KLBF Lab, Profile {0}, {1} build, ray tracing {2}, memory hooks {3}" -f $(if ($Fsr) { 'S' } else { 'X' }), $Build, $(if ($RayTracing) { 'on' } else { 'off' }), $(if ($Hooks -and -not $NoHooks) { 'on' } else { 'off' }))
if ($FsrX) { Write-Host "AMD FSR (Profile X): $FsrQuality, output $(if ($Output) { $Output } else { 'the title''s' })" }
if ($FrameGen) { Write-Host 'AMD frame generation: on (two frames shown per real one; more latency at a low base frame rate)' }
Write-Host "Cache: $Cache"
Write-Host "Saves: $Data"
# -Fsr (KLBF Lab): Profile S, whose title upscales with FSR 4, the calls going to the PC's FSR DLL
# (VICTORY_FSR_DLL, in -FfxDir).
if ($Fsr) {
    $fsrDll = Join-Path $FfxDir 'amd_fidelityfx_upscaler_dx12.dll'
    if (-not (Test-Path -LiteralPath $fsrDll)) { throw "AMD FSR not found: $fsrDll (use -FfxDir)" }
    $env:VICTORY_FSR_DLL = $fsrDll
    & $Victory run $exe --execute --console series-s
} else {
    & $Victory run $exe --execute
}
# (27/11) In the game F10 (Settings) and F11 (Restart) end it with exit code 10 or 11: the settings window
# (its Play starts the game again), or the same start again; the saves are backed up again at that start.
# ponytail: a new play.ps1 per relaunch (nested), fine for a session's few dozen restarts.
$code = $LASTEXITCODE
switch (Get-KlbfNextStart $code) {
    'settings' { $again = $passOn; & $PSCommandPath -Setup @again; exit $LASTEXITCODE }
    'restart' { $again = Get-KlbfRestartArgs $PSBoundParameters; & $PSCommandPath @again; exit $LASTEXITCODE }
}
exit $code
