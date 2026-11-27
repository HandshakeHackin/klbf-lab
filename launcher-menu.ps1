# KLBF Lab: the launcher's setup window and shader preparation, dot-sourced by play.ps1.
# Settings: %LOCALAPPDATA%\KLBF-Lab\launcher\settings.json (the current choice) and presets\<name>.json
# (KLBF_LAUNCHER_DIR moves that folder, for tests).

$script:KlbfModes = '1280x720', '1664x936', '1920x1080', '2560x1440', '3840x2160'
# AMD's presets: play.ps1's -FsrQuality values and their names (no Native AA: at 1.0 the Profile X memory
# plan overflows, victory-loader resolution.rs).
$script:KlbfPresets = [ordered]@{ quality = 'Quality'; balanced = 'Balanced'; performance = 'Performance'; ultraperformance = 'Ultra Performance' }

function Get-KlbfLauncherDir {
    if ($env:KLBF_LAUNCHER_DIR) { return $env:KLBF_LAUNCHER_DIR }
    Join-Path $env:LOCALAPPDATA 'KLBF-Lab\launcher'
}

# The owner's setting: Profile X, FSR 4 Performance, 3840x2160, frame generation, hooks and texture trim
# (= -Tout -FsrQuality performance -Output 3840x2160).
function Get-KlbfDefaults {
    [ordered]@{ profile = 'X'; upscaler = 'fsr'; preset = 'performance'; output = '3840x2160'; frameGen = $true; advanced = $true; game = '' }
}

# Any object (a parsed JSON) over the defaults: unknown or invalid values keep the default.
function ConvertTo-KlbfChoice($o) {
    $c = Get-KlbfDefaults
    if (-not $o) { return $c }
    if ($o.profile -in 'X', 'S') { $c.profile = [string]$o.profile }
    if ($o.upscaler -in 'fsr', 'title') { $c.upscaler = [string]$o.upscaler }
    if ($o.preset -in 'quality', 'balanced', 'performance', 'ultraperformance') { $c.preset = [string]$o.preset }
    if ($o.output -in $script:KlbfModes) { $c.output = [string]$o.output }
    if ($o.frameGen -is [bool]) { $c.frameGen = $o.frameGen }
    if ($o.advanced -is [bool]) { $c.advanced = $o.advanced }
    if ($o.game -is [string]) { $c.game = $o.game }
    $c
}

# Whether $Path is a game folder play.ps1 can start: it has game_scarlett_master.exe.
function Test-KlbfGameFolder([string]$Path) {
    [bool]($Path -and (Test-Path -LiteralPath (Join-Path $Path 'game_scarlett_master.exe') -PathType Leaf))
}

# Room on the drive of the cache. 10 GB measured 27/11: a full shader and pipeline cache 7.2 GB, saves and
# logs 11 MB, 10 save backups under 0.2 GB, plus margin for the cache to grow.
$script:KlbfSpaceGB = 10
function Get-KlbfSpace([string]$Cache) {
    if (-not $Cache) { $Cache = Join-Path $env:LOCALAPPDATA 'klbf-lab\cache' }
    $root = [IO.Path]::GetPathRoot($Cache)
    $free = try { [math]::Floor((New-Object IO.DriveInfo($root)).AvailableFreeSpace / 1GB) } catch { -1 }
    @{ drive = $root.TrimEnd('\'); needGB = $script:KlbfSpaceGB; freeGB = $free; ok = $free -ge $script:KlbfSpaceGB }
}

# The saved choice, or $null without a file (first run). A damaged file gives the defaults.
function Read-KlbfSettings([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    try { ConvertTo-KlbfChoice (Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json) } catch { Get-KlbfDefaults }
}

function Save-KlbfSettings($Choice, [string]$Path) {
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Path) | Out-Null
    ([pscustomobject]$Choice | ConvertTo-Json) | Set-Content -LiteralPath $Path -Encoding utf8
}

function Get-KlbfPresetPath([string]$Name) {
    $safe = ($Name -replace '[^\w \-\.]', '').Trim().Trim('.')
    if (-not $safe) { return $null }
    Join-Path (Join-Path (Get-KlbfLauncherDir) 'presets') "$safe.json"
}

function Get-KlbfPresetNames {
    $dir = Join-Path (Get-KlbfLauncherDir) 'presets'
    if (-not (Test-Path -LiteralPath $dir)) { return @() }
    @(Get-ChildItem -LiteralPath $dir -Filter *.json | Sort-Object Name | ForEach-Object { $_.BaseName })
}

# The play.ps1 parameters a choice stands for. Profile S is the title's FidelityFX path sent to the PC FSR
# (-Fsr): its render size and output are the title's. Frame generation is fed by the FSR upscale.
function Get-KlbfSwitches($Choice) {
    $on = [switch]$true
    $s = [ordered]@{}
    $fsr = $Choice.profile -eq 'S' -or $Choice.upscaler -eq 'fsr'
    if ($Choice.profile -eq 'S') {
        $s.Fsr = $on
    } else {
        if ($fsr) { $s.FsrX = $on; $s.FsrQuality = $Choice.preset }
        $s.Output = $Choice.output
    }
    if ($fsr -and $Choice.frameGen) { $s.FrameGen = $on }
    if ($Choice.advanced) { $s.Hooks = $on; $s.TexTrim = $on }
    $s
}

# The monitor's real mode (DPI-independent), else the primary screen's bounds.
function Get-KlbfScreenSize {
    try {
        $v = Get-CimInstance Win32_VideoController -ErrorAction Stop | Where-Object CurrentHorizontalResolution | Select-Object -First 1
        if ($v) { return '{0}x{1}' -f $v.CurrentHorizontalResolution, $v.CurrentVerticalResolution }
    } catch {}
    Add-Type -AssemblyName System.Windows.Forms
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    '{0}x{1}' -f $b.Width, $b.Height
}

function Test-KlbfShift {
    try {
        Add-Type -AssemblyName System.Windows.Forms
        ([System.Windows.Forms.Control]::ModifierKeys -band [System.Windows.Forms.Keys]::Shift) -ne 0
    } catch { $false }
}

# What the translated shaders depend on that the launcher knows of: the build and the settings that change
# the translation. Of the window's choices none does (profile, upscaler, preset, output, frame generation,
# hooks and texture trim are not read by the translator); ray tracing and -NoProbeRays do.
function Get-KlbfShaderStamp([string]$Victory, [string]$Build, $Probe, $Rays) {
    $built = Get-Item -LiteralPath $Victory
    '{0}|{1}|{2}|probe={3}|rays={4}' -f $built.Length, $built.LastWriteTimeUtc.Ticks, $Build, $Probe, $Rays
}

# Why shaders must be prepared again, from the stamp they were prepared for ($Have, '' if never).
function Get-KlbfPrepareReason([string]$Have, [string]$Want) {
    $build = { param($s) ($s -split '\|')[0..2] -join '|' }
    if ($Have -and (& $build $Have) -eq (& $build $Want)) {
        'You changed how the game is rendered: shaders must be prepared again. This takes a few minutes.'
    } else {
        'This build of KLBF Lab has not prepared its shaders yet. This takes a few minutes.'
    }
}

# This PC: @{ gpu; vramGB; ramGB; screen }. The card's memory from the driver's registry entry
# (HardwareInformation.qwMemorySize; Win32_VideoController's AdapterRAM stops at 4 GB), the largest card.
function Get-KlbfHardware {
    $gpu = ''; $vram = 0
    $class = 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}'
    foreach ($k in (Get-ChildItem -LiteralPath $class -ErrorAction SilentlyContinue | Where-Object PSChildName -match '^\d{4}$')) {
        $p = Get-ItemProperty -LiteralPath $k.PSPath -ErrorAction SilentlyContinue
        $bytes = $p.'HardwareInformation.qwMemorySize'
        if ($bytes -is [byte[]]) { $bytes = [BitConverter]::ToUInt64($bytes, 0) }
        if ($p.DriverDesc -and $bytes -and [double]$bytes -gt $vram) { $gpu = [string]$p.DriverDesc; $vram = [double]$bytes }
    }
    if (-not $gpu) {
        $v = Get-CimInstance Win32_VideoController -ErrorAction SilentlyContinue | Sort-Object AdapterRAM -Descending | Select-Object -First 1
        if ($v) { $gpu = [string]$v.Name; $vram = [double]$v.AdapterRAM }
    }
    # Installed memory (Windows' total leaves out what is reserved: 31.2 GB on a 32 GB PC)
    $ram = [double](Get-CimInstance Win32_PhysicalMemory -ErrorAction SilentlyContinue | Measure-Object Capacity -Sum).Sum
    if (-not $ram) { $ram = [double](Get-CimInstance Win32_ComputerSystem -ErrorAction SilentlyContinue).TotalPhysicalMemory }
    @{ gpu = $gpu; vramGB = [math]::Round($vram / 1GB); ramGB = [math]::Round($ram / 1GB); screen = Get-KlbfScreenSize }
}

# The card's generation from its name (the title needs mesh shaders: RDNA 2 and later on AMD).
function Get-KlbfGpuKind([string]$Name) {
    if ($Name -match 'Radeon.*RX\s*(\d)\d{3}') {
        switch ($Matches[1]) {
            '9' { return @{ gen = 'RDNA 4'; level = 'ok'; text = 'supported' } }
            '7' { return @{ gen = 'RDNA 3'; level = 'ok'; text = 'supported, less tested' } }
            '6' { return @{ gen = 'RDNA 2'; level = 'warn'; text = 'experimental (a fix for the card stopping is in progress)' } }
            '5' { return @{ gen = 'RDNA 1'; level = 'bad'; text = 'not supported (no mesh shaders, which the title needs)' } }
        }
    }
    # (27/11, nuit/etude-nvidia.md) Other cards by what they can run: the title draws with mesh shaders
    $nomesh = 'not supported (no mesh shaders, which the title needs)'
    if ($Name -match 'RTX\s*(20|30|40|50)\d\d') { return @{ gen = 'NVIDIA RTX'; level = 'warn'; text = 'supported, Profile S recommended' } }
    if ($Name -match 'GTX\s*16\d\d') { return @{ gen = 'NVIDIA GTX 16'; level = 'warn'; text = 'not tested' } }
    if ($Name -match 'GTX|GeForce\s*\d{3}') { return @{ gen = ''; level = 'bad'; text = $nomesh } }
    if ($Name -match '\bArc\b') { return @{ gen = 'Intel Arc'; level = 'warn'; text = 'not tested' } }
    if ($Name -match 'Intel.*(UHD|Iris|HD Graphics)') { return @{ gen = ''; level = 'bad'; text = $nomesh } }
    if ($Name -match 'Radeon.*(RX\s*[45]\d0\b|Vega|R[579]\s)') { return @{ gen = ''; level = 'bad'; text = $nomesh } }
    @{ gen = ''; level = 'warn'; text = 'not known' }
}

# One verdict per line: @{ what; level = ok|warn|bad; text }.
function Get-KlbfVerdicts($Hw) {
    $k = Get-KlbfGpuKind $Hw.gpu
    $name = if ($Hw.gpu) { $Hw.gpu + $(if ($k.gen) { " ($($k.gen))" } else { '' }) } else { 'not found' }
    @(
        @{ what = 'Graphics card'; level = $k.level; text = "$name - $($k.text)" }
        $(if ($Hw.vramGB -ge 16) { @{ what = 'Video memory'; level = 'ok'; text = "$($Hw.vramGB) GB" } }
          elseif ($Hw.vramGB -ge 12) { @{ what = 'Video memory'; level = 'warn'; text = "$($Hw.vramGB) GB - 16 GB recommended" } }
          else { @{ what = 'Video memory'; level = 'bad'; text = "$($Hw.vramGB) GB - under 12 GB: Profile S recommended" } })
        $(if ($Hw.ramGB -ge 32) { @{ what = 'Memory'; level = 'ok'; text = "$($Hw.ramGB) GB" } }
          elseif ($Hw.ramGB -ge 24) { @{ what = 'Memory'; level = 'warn'; text = "$($Hw.ramGB) GB - 32 GB recommended" } }
          else { @{ what = 'Memory'; level = 'bad'; text = "$($Hw.ramGB) GB - may run out of memory (the game uses about 21 GB)" } })
        @{ what = 'Screen'; level = 'ok'; text = $Hw.screen }
    )
}

# The upscaler's name on this card: what ran last time if the game said (klbf-upscaler.txt in the cache,
# written from the DLL's provider version), else AMD FSR 4 on RDNA 4 and AMD FSR 3.1 elsewhere (the DLL
# falls back to FSR 3.1 without RDNA 4).
function Get-KlbfUpscalerName($Hw, [string]$Cache) {
    if (-not $Cache) { $Cache = Join-Path $env:LOCALAPPDATA 'klbf-lab\cache' }
    $ran = Join-Path $Cache 'klbf-upscaler.txt'
    if (Test-Path -LiteralPath $ran) { $t = (Get-Content -LiteralPath $ran -TotalCount 1).Trim(); if ($t -match '^AMD FSR [\d.]+$') { return $t } }
    if ((Get-KlbfGpuKind $Hw.gpu).gen -eq 'RDNA 4') { 'AMD FSR 4' } else { 'AMD FSR 3.1' }
}

# The settings for this PC (27/11): Profile X only for RDNA 3 and 4 with 16 GB or more (RDNA 4: the owner's
# default; RDNA 3: output = the screen if it is one of the title's modes, else 2560x1440). Everything else,
# RDNA 2 and any card under 16 GB included, Profile S (a 12 GB RX 6750 XT ran 25 min stable on it).
function Get-KlbfRecommended($Hw) {
    $c = Get-KlbfDefaults
    $gen = (Get-KlbfGpuKind $Hw.gpu).gen
    # Frame generation only on RDNA 4 (elsewhere FSR 3.1 frame generation on a base far under AMD's 60 fps)
    if ($gen -notin 'RDNA 3', 'RDNA 4' -or $Hw.vramGB -lt 16) { $c.profile = 'S'; $c.frameGen = $gen -eq 'RDNA 4'; return $c }
    if ($gen -eq 'RDNA 4') { return $c }
    $c.frameGen = $false
    $c.output = if ($Hw.screen -in $script:KlbfModes) { $Hw.screen } else { '2560x1440' }
    $c
}

function New-KlbfFont([float]$Size = 9, [string]$Style = 'Regular') {
    New-Object System.Drawing.Font('Segoe UI', $Size, [System.Drawing.FontStyle]::$Style)
}

# KLBF Lab's colours, as on the game window's frame (victory-gpu shell.rs set_colors): violet border,
# near-black caption, white text.
# Violet only for accents (the Play button, the header line); the rest in neutral greys.
$script:KlbfTheme = @{ back = 0x15101F; field = 0x2A2A30; line = 0x55555E; violet = 0x7C3AED; text = 0xFFFFFF; soft = 0xB8B8C0 }

function Get-KlbfColor([int]$Rgb) {
    [System.Drawing.Color]::FromArgb(($Rgb -shr 16) -band 255, ($Rgb -shr 8) -band 255, $Rgb -band 255)
}

# Our icon: beside this script (a version folder) or in the repository (apps\victory\klbf-lab.ico).
function Get-KlbfIconPath {
    foreach ($p in (Join-Path $PSScriptRoot 'klbf-lab.ico'), (Join-Path $PSScriptRoot 'apps\victory\klbf-lab.ico')) {
        if (Test-Path -LiteralPath $p) { return $p }
    }
}

# The icon's largest picture: its PNG frame when it has one (Icon.ToBitmap garbles those), else 48 x 48.
function Get-KlbfLogo([string]$Ico) {
    $b = [IO.File]::ReadAllBytes($Ico)
    $best = $null; $size = 0
    for ($i = 0; $i -lt [BitConverter]::ToUInt16($b, 4); $i++) {
        $e = 6 + 16 * $i
        $w = if ($b[$e] -eq 0) { 256 } else { $b[$e] }
        $len = [BitConverter]::ToInt32($b, $e + 8); $at = [BitConverter]::ToInt32($b, $e + 12)
        if ($w -gt $size -and $b[$at] -eq 0x89 -and $b[$at + 1] -eq 0x50) { $size = $w; $best = @($at, $len) }
    }
    if ($best) { return [System.Drawing.Image]::FromStream((New-Object IO.MemoryStream($b, $best[0], $best[1]))) }
    (New-Object System.Drawing.Icon($Ico, 48, 48)).ToBitmap()
}

# A form in KLBF Lab's colours and icon; on Windows 11 its frame too (violet border, dark caption).
function Set-KlbfTheme($Form) {
    $Form.BackColor = Get-KlbfColor $script:KlbfTheme.back
    $Form.ForeColor = Get-KlbfColor $script:KlbfTheme.text
    $Form.Font = New-KlbfFont
    $ico = Get-KlbfIconPath
    if ($ico) { $Form.Icon = New-Object System.Drawing.Icon($ico) }
    $Form.add_HandleCreated({
        param($sender)
        try {
            if (-not ('KlbfDwm' -as [type])) {
                Add-Type -Namespace '' -Name KlbfDwm -MemberDefinition '[DllImport("dwmapi.dll")] public static extern int DwmSetWindowAttribute(IntPtr h, int a, ref int v, int n);'
            }
            # The game window's exact values (victory-gpu shell.rs set_colors): DWMWA_BORDER_COLOR 34 = 0x7C3AED,
            # CAPTION_COLOR 35 = 0x15101F, TEXT_COLOR 36 = 0xFFFFFF, as COLORREF (0x00BBGGRR)
            foreach ($a in @(@(34, 0xED3A7C), @(35, 0x1F1015), @(36, 0xFFFFFF))) { $v = $a[1]; [void][KlbfDwm]::DwmSetWindowAttribute($sender.Handle, $a[0], [ref]$v, 4) }
        } catch {}
    })
}

function Set-KlbfButton($Button, [bool]$Main = $false) {
    $Button.FlatStyle = 'Flat'
    $Button.FlatAppearance.BorderColor = Get-KlbfColor $(if ($Main) { $script:KlbfTheme.violet } else { $script:KlbfTheme.line })
    $Button.BackColor = Get-KlbfColor $(if ($Main) { $script:KlbfTheme.violet } else { $script:KlbfTheme.field })
    $Button.ForeColor = Get-KlbfColor $script:KlbfTheme.text
    $Button.UseVisualStyleBackColor = $false
}

$script:KlbfRepo = 'https://github.com/PastiiJay/klbf-lab'

# ---- Saves: the title's containers in <data>\<title id>\saves\<provider>\<user>\<name>~<hash>\v<N>\ ----
# (victory-hle xgamesave.rs). A version holds `manifest` (u32-prefixed strings: name, display name, blob
# names) and one file per blob: 0 the save, 1 its header, whose title (UTF-16 at 0x24) the menu shows.

function Get-KlbfSavesRoot([string]$Data) {
    if (-not $Data) { $Data = Join-Path $env:LOCALAPPDATA 'klbf-lab\data' }
    Get-ChildItem -LiteralPath $Data -Directory -ErrorAction SilentlyContinue |
        ForEach-Object { Join-Path $_.FullName 'saves' } | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}

function Get-KlbfBackupDir { Join-Path (Split-Path -Parent (Get-KlbfLauncherDir)) 'save-backups' }

function Read-KlbfManifest([string]$Path) {
    $b = [IO.File]::ReadAllBytes($Path); $at = 0; $out = @()
    while ($at + 4 -le $b.Length) {
        $n = [BitConverter]::ToUInt32($b, $at); $at += 4
        if ($at + $n -gt $b.Length) { return $null }
        $out += [Text.Encoding]::UTF8.GetString($b, $at, $n); $at += $n
    }
    , $out
}

function Write-KlbfManifest([string]$Path, [string[]]$Names) {
    $ms = New-Object IO.MemoryStream
    foreach ($s in $Names) { $u = [Text.Encoding]::UTF8.GetBytes($s); $ms.Write([BitConverter]::GetBytes([uint32]$u.Length), 0, 4); $ms.Write($u, 0, $u.Length) }
    [IO.File]::WriteAllBytes($Path, $ms.ToArray())
}

# The game's saves (not the profile's settings containers), as its Load menu lists them: the autosave first,
# then the newest first. Seen in the game (25/11), not read from the title: the menu sorts by date.
function Get-KlbfSaves([string]$SavesRoot) {
    if (-not $SavesRoot -or -not (Test-Path -LiteralPath $SavesRoot)) { return @() }
    $list = foreach ($dir in (Get-ChildItem -Path (Join-Path $SavesRoot '*\*\*') -Directory -ErrorAction SilentlyContinue | Where-Object Name -match '^SARC.*~[0-9a-f]{16}$')) {
        $v = Get-ChildItem -LiteralPath $dir.FullName -Directory | Where-Object { $_.Name -match '^v(\d+)$' -and (Test-Path -LiteralPath (Join-Path $_.FullName 'manifest')) } |
            Sort-Object { [int]($_.Name.Substring(1)) } -Descending | Select-Object -First 1
        if (-not $v) { continue }
        $names = Read-KlbfManifest (Join-Path $v.FullName 'manifest')
        $title = ''
        $head = Join-Path $v.FullName '1'
        if (Test-Path -LiteralPath $head) {
            $h = [IO.File]::ReadAllBytes($head)
            if ($h.Length -gt 0x26) { $title = ([Text.Encoding]::Unicode.GetString($h, 0x24, [math]::Min(128, $h.Length - 0x24)) -split [char]0)[0] }
        }
        $data = Get-Item -LiteralPath (Join-Path $v.FullName '0') -ErrorAction SilentlyContinue
        [pscustomobject]@{
            name = $names[0]; display = $names[1]; blobs = @($names | Select-Object -Skip 2); title = $title
            auto = $title -like '(Auto)*'; date = $data.LastWriteTime; size = $data.Length; dir = $dir.FullName; version = $v.FullName; slot = ''
        }
    }
    $sorted = @(@($list | Where-Object auto | Sort-Object date -Descending) + @($list | Where-Object { -not $_.auto } | Sort-Object date -Descending))
    $n = 0
    foreach ($s in $sorted) { $s.slot = if ($s.auto) { 'Autosave' } else { $n++; "Slot $n" } }
    $sorted
}

# A zip of the whole saves folder in save-backups\<date>.zip; the last $Keep are kept. Its path, or $null.
function Backup-KlbfSaves([string]$SavesRoot, [string]$Dir = (Get-KlbfBackupDir), [int]$Keep = 10) {
    if (-not $SavesRoot -or -not (Test-Path -LiteralPath $SavesRoot)) { return $null }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    New-Item -ItemType Directory -Force -Path $Dir | Out-Null
    $zip = Join-Path $Dir ((Get-Date -Format 'yyyy-MM-dd_HH-mm-ss-fff') + '.zip')
    [IO.Compression.ZipFile]::CreateFromDirectory($SavesRoot, $zip)
    Get-ChildItem -LiteralPath $Dir -Filter *.zip | Sort-Object Name -Descending | Select-Object -Skip $Keep | Remove-Item
    $zip
}

# Puts a backup back: the current saves are backed up first, then replaced by the zip's.
function Restore-KlbfSaves([string]$Zip, [string]$SavesRoot, [string]$Dir = (Get-KlbfBackupDir)) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $before = Backup-KlbfSaves $SavesRoot $Dir
    if (Test-Path -LiteralPath $SavesRoot) {
        if (-not $before) { throw 'No backup of the current saves: nothing restored.' }
        Remove-Item -LiteralPath $SavesRoot -Recurse -Force
    }
    [IO.Compression.ZipFile]::ExtractToDirectory($Zip, $SavesRoot)
    $before
}

# Slot $From's save into slot $To (both existing saves): every save backed up first, then a new version of
# $To with $From's files and $To's own names, committed as the game commits one (v<N>.new renamed to v<N>).
function Copy-KlbfSave($From, $To, [string]$SavesRoot, [string]$Dir = (Get-KlbfBackupDir)) {
    if (-not $From -or -not $To -or $From.dir -eq $To.dir) { throw 'Pick two different slots.' }
    $backup = Backup-KlbfSaves $SavesRoot $Dir
    if (-not $backup) { throw 'No backup: nothing copied.' }
    $n = 1 + (Get-ChildItem -LiteralPath $To.dir -Directory | Where-Object Name -match '^v(\d+)$' | ForEach-Object { [int]$_.Name.Substring(1) } | Measure-Object -Maximum).Maximum
    $new = Join-Path $To.dir "v$n.new"
    New-Item -ItemType Directory -Force -Path $new | Out-Null
    Get-ChildItem -LiteralPath $From.version -File | Where-Object Name -ne 'manifest' | Copy-Item -Destination $new
    Write-KlbfManifest (Join-Path $new 'manifest') (@($To.name, $To.display) + $From.blobs)
    Rename-Item -LiteralPath $new -NewName "v$n"
    $backup
}

# ---- Report a bug and the bench ----

# The newest log of the title (the crash log beside its data), or ''.
function Get-KlbfLatestLog([string]$Data) {
    if (-not $Data) { $Data = Join-Path $env:LOCALAPPDATA 'klbf-lab\data' }
    $l = Get-ChildItem -Path (Join-Path $Data '*\crashes.log') -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($l) { $l.FullName } else { '' }
}

# The block "Report a bug" copies: build, settings, this PC, Windows, the log to attach. Nothing goes in a URL.
function Get-KlbfBugReport($Choice, $Hw, [string]$Victory, [string]$Log, [string]$Windows) {
    $k = Get-KlbfGpuKind $Hw.gpu
    $build = if ($Victory -and (Test-Path -LiteralPath $Victory)) {
        $e = Get-Item -LiteralPath $Victory
        '{0} (victory.exe of {1:yyyy-MM-dd HH:mm})' -f $e.Directory.Name, $e.LastWriteTime
    } else { 'unknown' }
    $git = Join-Path $PSScriptRoot '.git'
    if (Test-Path -LiteralPath $git) { try { $build += ', commit ' + (git -C $PSScriptRoot rev-parse --short HEAD 2>$null) } catch {} }
    $up = if ($Choice.profile -eq 'S') { "the title's FSR (Profile S)" } elseif ($Choice.upscaler -eq 'fsr') { (Get-KlbfUpscalerName $Hw) + ' ' + $script:KlbfPresets[$Choice.preset] } else { "the title's own" }
    @(
        '### System (KLBF Lab Settings)'
        "- KLBF Lab: $build"
        "- Profile: $($Choice.profile)"
        "- Upscaler: $up"
        "- Output: $(if ($Choice.profile -eq 'S') { "the title's" } else { $Choice.output })"
        "- Frame generation: $(if ($Choice.frameGen -and ($Choice.profile -eq 'S' -or $Choice.upscaler -eq 'fsr')) { 'on (FG x2)' } else { 'off' })"
        "- Memory hooks and texture trim: $(if ($Choice.advanced) { 'on' } else { 'off' })"
        "- Graphics card: $($Hw.gpu)$(if ($k.gen) { " ($($k.gen))" }), $($Hw.vramGB) GB"
        "- Memory: $($Hw.ramGB) GB"
        "- Screen: $($Hw.screen)"
        "- Windows: $Windows"
        "- Log to attach: $(if ($Log) { $Log } else { 'none found: copy the text of the console window' })"
    ) -join "`r`n"
}

# What the bench window says (KLBF Lab Bench BM-v1, docs/klbf/BENCH-BM-V1.md). No save is shipped: the
# player makes his own, at the point below.
function Get-KlbfBenchGuide {
    @(
        'KLBF Lab Bench BM-v1: 5 minutes, always the same scene, measured by the build itself.'
        ''
        'Once, before the first bench:'
        '1. Play about 10 minutes with this build, so its shaders and effects are prepared (the bench does not start before).'
        "2. Make the bench save: sleep until the morning in the main character's bed at his house (about 3 % into the story), go down to the laundry room, pick up the fishing book on the workbench, keep it in hand, and save."
        '   It must be your most recent save: the bench presses Continue, which loads the most recent one.'
        ''
        'Each bench:'
        '3. Close other heavy programs. Press Start bench, then touch nothing: after 5 seconds in the world the controller and keyboard are ignored until 5:00.'
        '4. The result card covers the screen at the end; a button closes it and the game.'
        ''
        'Results: the bench-results folder beside victory.exe (card PNG, JSON, text).'
        "Post them: $script:KlbfRepo/issues/new?template=bench-bm-v1.yml"
        'All your saves are backed up before the run (Saves tab).'
    ) -join "`r`n"
}

# The play.ps1 switches of a bench run: the player's settings, then -Bench (and -BenchPlayer).
function Get-KlbfBenchSwitches($Choice, [string]$Player) {
    $s = Get-KlbfSwitches $Choice
    $s.Bench = [switch]$true
    $s.BenchPlayer = if ($Player.Trim()) { $Player.Trim() } else { 'anonymous' }
    $s
}

# The data folder of a bench run (27/11): a copy of $Data in which the chosen save is the only save, so
# Continue can only load that one; the player's own saves are never opened by the run. Rebuilt each run.
function New-KlbfBenchData([string]$Data, $Save, [string]$Dest = (Join-Path (Split-Path -Parent (Get-KlbfLauncherDir)) 'bench-data')) {
    if (Test-Path -LiteralPath $Dest) { Remove-Item -LiteralPath $Dest -Recurse -Force }
    Copy-Item -LiteralPath $Data -Destination $Dest -Recurse
    $root = Get-KlbfSavesRoot $Dest
    foreach ($s in (Get-KlbfSaves $root)) {
        if ((Split-Path -Leaf $s.dir) -ne (Split-Path -Leaf $Save.dir)) { Remove-Item -LiteralPath $s.dir -Recurse -Force }
    }
    $Dest
}

# How the card names a save: "#2 <in-game name>" (the Saves tab's number), "Autosave <name>".
function Get-KlbfSaveLabel($Save) {
    if ($Save.auto) { "Autosave $($Save.title)" } else { '#' + ($Save.slot -replace '^Slot ', '') + " $($Save.title)" }
}

# The bench window: the guide, the save to load (the most recent by default), a GitHub login box,
# Start bench / Cancel. @{ player; save } or $null.
function Show-KlbfBenchGuide($Saves) {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $f = New-Object System.Windows.Forms.Form
    $f.Text = 'KLBF Lab Bench'; Set-KlbfTheme $f; $f.FormBorderStyle = 'FixedDialog'
    $f.MaximizeBox = $false; $f.MinimizeBox = $false; $f.StartPosition = 'CenterParent'
    $f.ClientSize = New-Object System.Drawing.Size(520, 456)
    $t = New-Object System.Windows.Forms.Label; $t.Text = Get-KlbfBenchGuide; $t.SetBounds(16, 12, 488, 300)
    $sl = New-Object System.Windows.Forms.Label; $sl.Text = 'Save the bench loads:'; $sl.SetBounds(16, 358, 200, 20)
    $pick = New-Object System.Windows.Forms.ComboBox; $pick.DropDownStyle = 'DropDownList'; $pick.ForeColor = [System.Drawing.Color]::Black; $pick.SetBounds(216, 355, 288, 24)
    foreach ($s in $Saves) { [void]$pick.Items.Add("$($s.slot): $($s.title)") }
    if ($Saves) { $pick.SelectedIndex = [array]::IndexOf(@($Saves), (@($Saves) | Sort-Object date -Descending | Select-Object -First 1)) }
    $l = New-Object System.Windows.Forms.Label; $l.Text = 'Your GitHub login (on the card; empty: anonymous):'; $l.SetBounds(16, 322, 300, 20)
    $box = New-Object System.Windows.Forms.TextBox; $box.SetBounds(316, 319, 188, 24)
    $box.BackColor = Get-KlbfColor $script:KlbfTheme.field; $box.ForeColor = Get-KlbfColor $script:KlbfTheme.text
    $go = New-Object System.Windows.Forms.Button; $go.Text = 'Start bench'; $go.DialogResult = 'OK'; $go.SetBounds(296, 404, 100, 34)
    $no = New-Object System.Windows.Forms.Button; $no.Text = 'Cancel'; $no.DialogResult = 'Cancel'; $no.SetBounds(404, 404, 100, 34)
    $go.Enabled = [bool]$Saves
    Set-KlbfButton $go $true; Set-KlbfButton $no
    $f.Controls.AddRange(@($t, $l, $box, $sl, $pick, $go, $no)); $f.AcceptButton = $go; $f.CancelButton = $no
    if ($f.ShowDialog() -eq 'OK') { @{ player = $box.Text; save = @($Saves)[$pick.SelectedIndex] } } else { $null }
}

# The setup window, not shown: @{ form; c (its controls) }. $Hardware: Get-KlbfHardware's; $Info: @{ victory;
# data; windows } (all injected by tests). $ShaderStatus: one line under the tabs.
function New-KlbfMenuForm($Choice, $Hardware, [string]$ShaderStatus = '', $Info = @{}) {
    $Screen = $Hardware.screen
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    [System.Windows.Forms.Application]::EnableVisualStyles()
    $f = New-Object System.Windows.Forms.Form
    $f.Text = 'KLBF Lab Settings'
    Set-KlbfTheme $f
    $f.FormBorderStyle = 'FixedDialog'; $f.MaximizeBox = $false; $f.MinimizeBox = $false
    $f.StartPosition = 'CenterScreen'
    $f.ClientSize = New-Object System.Drawing.Size(520, 884)
    $soft = Get-KlbfColor $script:KlbfTheme.soft
    $back = Get-KlbfColor $script:KlbfTheme.back
    $c = @{ recommended = (Get-KlbfRecommended $Hardware); info = $Info; hw = $Hardware; openUrl = { param($u) Start-Process $u }; clipboard = { param($t) [System.Windows.Forms.Clipboard]::SetText($t) } }
    $add = { param($parent, $ctl, $x, $y, $w, $h) $ctl.SetBounds($x, $y, $w, $h); $parent.Controls.Add($ctl); $ctl }
    $label = { param($parent, $text, $x, $y, $w, $h = 20) $l = New-Object System.Windows.Forms.Label; $l.Text = $text; & $add $parent $l $x $y $w $h }
    $radio = { param($parent, $text, $x, $y, $w) $r = New-Object System.Windows.Forms.RadioButton; $r.Text = $text; & $add $parent $r $x $y $w 22 }
    $group = { param($parent, $text, $y, $h) $g = New-Object System.Windows.Forms.GroupBox; $g.Text = $text; $g.ForeColor = $soft; & $add $parent $g 6 $y 480 $h }
    # Controls inside a group: white text (the group's caption is soft grey)
    $white = { param($ctl) $ctl.ForeColor = Get-KlbfColor $script:KlbfTheme.text; $ctl }
    $combo = { param($parent, $x, $y, $w)
        # Windows draws a drop-down list in its own light colours: black text on it
        $cb = New-Object System.Windows.Forms.ComboBox; $cb.DropDownStyle = 'DropDownList'; $cb.ForeColor = [System.Drawing.Color]::Black
        & $add $parent $cb $x $y $w 24 }
    $btn = { param($parent, $text, $x, $y, $w, $h = 28) $b = New-Object System.Windows.Forms.Button; $b.Text = $text; Set-KlbfButton $b; & $add $parent $b $x $y $w $h }

    # Header: our logo, the name, the question, the GitHub link
    $ico = Get-KlbfIconPath
    if ($ico) {
        $logo = New-Object System.Windows.Forms.PictureBox
        $logo.Image = Get-KlbfLogo $ico
        $logo.SizeMode = 'Zoom'
        [void](& $add $f $logo 16 12 48 48)
    }
    $title = & $label $f 'KLBF Lab' 74 10 200 30
    $title.Font = New-KlbfFont 16 'Bold'
    $sub = & $label $f 'How do you want to play?' 76 40 220 20
    $sub.ForeColor = $soft
    $c.link = New-Object System.Windows.Forms.LinkLabel
    $c.link.Text = 'github.com/PastiiJay/klbf-lab'; $c.link.TextAlign = 'MiddleRight'
    $c.link.LinkColor = $soft; $c.link.ActiveLinkColor = Get-KlbfColor $script:KlbfTheme.text
    [void](& $add $f $c.link 280 40 224 20)
    $rule = New-Object System.Windows.Forms.Panel; $rule.BackColor = Get-KlbfColor $script:KlbfTheme.violet
    [void](& $add $f $rule 16 70 488 2)

    $tabs = New-Object System.Windows.Forms.TabControl
    [void](& $add $f $tabs 12 80 496 672)
    $c.tabs = $tabs
    $page = New-Object System.Windows.Forms.TabPage; $page.Text = 'Play'; $page.BackColor = $back; $page.ForeColor = Get-KlbfColor $script:KlbfTheme.text
    $savesPage = New-Object System.Windows.Forms.TabPage; $savesPage.Text = 'Saves'; $savesPage.BackColor = $back; $savesPage.ForeColor = Get-KlbfColor $script:KlbfTheme.text
    $tabs.TabPages.AddRange(@($page, $savesPage))

    # Your PC: what was found, a verdict per line (green: fine, amber: works with less, red: not supported)
    # Game folder: where the player's copy is; checked, and the room the cache needs on its drive
    $g = & $group $page 'Game folder' 4 96
    $c.game = New-Object System.Windows.Forms.TextBox; $c.game.ReadOnly = $true
    $c.game.BackColor = Get-KlbfColor $script:KlbfTheme.field; $c.game.ForeColor = Get-KlbfColor $script:KlbfTheme.text
    [void](& $add $g $c.game 12 22 340 24)
    $c.browse = & $btn $g "Browse$([char]0x2026)" 360 20 108
    $c.gameDot = & $label $g ([string][char]0x25CF) 12 50 18
    $c.gameText = & $white (& $label $g '' 30 50 444)
    $c.spaceDot = & $label $g ([string][char]0x25CF) 12 70 18
    $c.space = Get-KlbfSpace $Info.cache
    $c.spaceText = & $white (& $label $g ('About {0} GB needed on {1} (cache, logs, backups); {2} GB free' -f $c.space.needGB, $c.space.drive, $c.space.freeGB) 30 70 450)
    $c.spaceDot.ForeColor = Get-KlbfColor $(if ($c.space.ok) { 0x22C55E } else { 0xF59E0B })

    $g = & $group $page 'Your PC' 104 140
    $c.pc = @{}
    $y = 20
    foreach ($v in (Get-KlbfVerdicts $Hardware)) {
        $dot = & $label $g ([string][char]0x25CF) 12 $y 18
        $dot.ForeColor = Get-KlbfColor @{ ok = 0x22C55E; warn = 0xF59E0B; bad = 0xEF4444 }[$v.level]
        $c.pc[$v.what] = & $white (& $label $g ('{0}: {1}' -f $v.what, $v.text) 30 $y 444)
        $y += 20
    }
    $c.recommend = & $btn $g 'Recommended settings for this PC' 12 ($y + 4) 260

    $g = & $group $page 'Profile' 248 52
    # "(recommended for your PC)" on what Get-KlbfRecommended picks for this hardware
    $mark = { param($text, $on) if ($on) { "$text (recommended for your PC)" } else { $text } }
    $rec = $c.recommended
    $c.profX = & $white (& $radio $g (& $mark 'Profile X' ($rec.profile -eq 'X')) 12 20 270)
    $c.profS = & $white (& $radio $g (& $mark 'Profile S' ($rec.profile -eq 'S')) 286 20 190)

    $g = & $group $page 'Upscaler' 304 96
    $c.upFsr = & $white (& $radio $g (& $mark (Get-KlbfUpscalerName $Hardware $Info.cache) ($rec.upscaler -eq 'fsr')) 12 20 270)
    $c.upTitle = & $white (& $radio $g "The title's own upscaler" 286 20 190)
    [void](& $white (& $label $g 'Preset:' 12 46 120))
    $p = New-Object System.Windows.Forms.Panel
    [void](& $add $g $p 8 64 466 26)
    $c.presets = @{}
    $at = @{ quality = 4, 92; balanced = 100, 96; performance = 200, 118; ultraperformance = 322, 140 }
    foreach ($k in $script:KlbfPresets.Keys) {
        $c.presets[$k] = & $white (& $radio $p $script:KlbfPresets[$k] $at[$k][0] 2 $at[$k][1])
    }

    $g = & $group $page 'Output resolution' 404 78
    $c.output = & $combo $g 12 22 140
    foreach ($m in $script:KlbfModes) { [void]$c.output.Items.Add($m) }
    $c.screen = & $white (& $label $g "Your screen: $Screen" 170 25 300)
    $c.outputNote = & $white (& $label $g '' 12 52 460)

    $g = & $group $page 'Options' 486 74
    $c.fg = New-Object System.Windows.Forms.CheckBox
    $c.fg.Text = 'Frame generation (FG x2)'
    [void](& $white (& $add $g $c.fg 12 20 460 22))
    $c.adv = New-Object System.Windows.Forms.CheckBox
    $c.adv.Text = 'Advanced: memory hooks and texture trim (faster, uses less RAM)'
    [void](& $white (& $add $g $c.adv 12 44 462 22))

    $g = & $group $page 'Saved profiles' 564 60
    $c.saved = & $combo $g 12 24 196
    foreach ($n in (Get-KlbfPresetNames)) { [void]$c.saved.Items.Add($n) }
    if ($c.saved.Items.Count) { $c.saved.SelectedIndex = 0 }
    $c.load = & $btn $g 'Load' 214 22 78
    $c.saveAs = & $btn $g "Save as$([char]0x2026)" 298 22 84
    $c.delete = & $btn $g 'Delete' 388 22 78

    # Saves tab
    $c.savesRoot = Get-KlbfSavesRoot $Info.data
    $c.savesList = New-Object System.Windows.Forms.ListView
    $c.savesList.View = 'Details'; $c.savesList.FullRowSelect = $true; $c.savesList.HeaderStyle = 'Nonclickable'
    $c.savesList.BackColor = Get-KlbfColor $script:KlbfTheme.field; $c.savesList.ForeColor = Get-KlbfColor $script:KlbfTheme.text
    foreach ($h in @(@('In the menu', 90), @('Name in the game', 190), @('Date', 120), @('Size', 70))) { [void]$c.savesList.Columns.Add($h[0], $h[1]) }
    [void](& $add $savesPage $c.savesList 6 8 480 250)
    $note = & $label $savesPage ("Numbered as the game's Load menu lists them: the autosave first, then the newest first. " +
        'All saves are backed up before every launch and every bench (the last 10 are kept).') 6 264 480 54
    $note.ForeColor = $soft
    $c.backup = & $btn $savesPage 'Back up all' 6 322 150
    $c.restore = & $btn $savesPage "Restore a backup$([char]0x2026)" 166 322 170
    $g = & $group $savesPage 'Copy a save over another (all saves are backed up first)' 362 64
    $c.copyFrom = & $combo $g 12 26 170
    [void](& $white (& $label $g 'over' 190 30 36))
    $c.copyTo = & $combo $g 230 26 140
    $c.copy = & $btn $g 'Copy' 380 24 88
    $c.savesNote = & $white (& $label $savesPage '' 6 436 480 60)

    # Under the tabs: the shaders' line, then a 3-column grid (156 px columns, 10 px apart): grey buttons on
    # the first row (room for more), Play alone in the right column of the second, violet.
    $c.status = & $label $f $ShaderStatus 16 758 488 22
    $c.status.ForeColor = $soft
    $col = { param($i) 16 + 166 * $i }
    $c.prepare = & $btn $f 'Prepare shaders' (& $col 0) 784 156 36
    $c.bench = & $btn $f 'Run benchmark' (& $col 1) 784 156 36
    $c.bug = & $btn $f 'Report a bug' (& $col 2) 784 156 36
    $c.bugNote = & $label $f '' (& $col 0) 830 322 44
    $c.bugNote.ForeColor = $soft
    $c.play = & $btn $f 'Play' (& $col 2) 828 156 44
    Set-KlbfButton $c.play $true
    $c.play.Font = New-KlbfFont 12 'Bold'

    $f.AcceptButton = $c.play

    $c.form = $f
    $c.game.Text = $(if ($Choice.game) { $Choice.game } else { [string]$Info.game })
    Set-KlbfFormChoice $c $Choice
    Update-KlbfSavesTab $c
    # A closure sees variables, not this file's functions: they go in as variables.
    $fUpdate = ${function:Update-KlbfForm}; $fSet = ${function:Set-KlbfFormChoice}; $fGet = ${function:Get-KlbfFormChoice}
    $fPath = ${function:Get-KlbfPresetPath}; $fRead = ${function:Read-KlbfSettings}; $fSave = ${function:Save-KlbfSettings}
    $fSaves = ${function:Update-KlbfSavesTab}; $fBackup = ${function:Backup-KlbfSaves}; $fRestore = ${function:Restore-KlbfSaves}
    $fCopy = ${function:Copy-KlbfSave}; $fReport = ${function:Get-KlbfBugReport}; $fLog = ${function:Get-KlbfLatestLog}
    $fGuide = ${function:Show-KlbfBenchGuide}; $repo = $script:KlbfRepo; $backupDir = Get-KlbfBackupDir
    $update = { & $fUpdate $c }.GetNewClosure()
    foreach ($r in @($c.profX, $c.profS, $c.upFsr, $c.upTitle)) { $r.add_CheckedChanged($update) }
    $c.load.add_Click({
        if (-not $c.saved.SelectedItem) { return }
        $path = & $fPath $c.saved.SelectedItem
        & $fSet $c (& $fRead $path)
    }.GetNewClosure())
    $c.saveAs.add_Click({
        Add-Type -AssemblyName Microsoft.VisualBasic
        $name = [Microsoft.VisualBasic.Interaction]::InputBox('Name of this profile:', 'KLBF Lab', '')
        $path = & $fPath $name
        if (-not $path) { return }
        & $fSave (& $fGet $c) $path
        $base = [IO.Path]::GetFileNameWithoutExtension($path)
        if (-not $c.saved.Items.Contains($base)) { [void]$c.saved.Items.Add($base) }
        $c.saved.SelectedItem = $base
    }.GetNewClosure())
    $c.delete.add_Click({
        if (-not $c.saved.SelectedItem) { return }
        $path = & $fPath $c.saved.SelectedItem
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path }
        $c.saved.Items.Remove($c.saved.SelectedItem)
        if ($c.saved.Items.Count) { $c.saved.SelectedIndex = 0 }
    }.GetNewClosure())
    $c.recommend.add_Click({ & $fSet $c $c.recommended }.GetNewClosure())
    $c.link.add_LinkClicked({ & $c.openUrl $repo }.GetNewClosure())
    $c.bug.add_Click({
        $text = & $fReport (& $fGet $c) $c.hw $c.info.victory (& $fLog $c.info.data) $c.info.windows
        $c.lastReport = $text
        try { & $c.clipboard $text } catch {}
        $c.bugNote.Text = 'System info copied: paste it in the issue and attach the log.'
        & $c.openUrl "$repo/issues/new"
    }.GetNewClosure())
    $c.bench.add_Click({
        $r = if ($c.benchGuide) { & $c.benchGuide $c.saves } else { & $fGuide $c.saves }
        if ($null -eq $r) { return }
        $c.benchPlayer = $r.player; $c.benchSave = $r.save; $c.form.Tag = 'bench'; $c.form.Close()
    }.GetNewClosure())
    $c.browse.add_Click({
        $d = New-Object System.Windows.Forms.FolderBrowserDialog
        $d.Description = 'The folder of your copy of the game (the one with game_scarlett_master.exe)'
        if ($c.game.Text) { $d.SelectedPath = $c.game.Text }
        $picked = if ($c.pickFolder) { & $c.pickFolder } elseif ($d.ShowDialog() -eq 'OK') { $d.SelectedPath }
        if ($picked) { $c.game.Text = $picked; & $fUpdate $c }
    }.GetNewClosure())
    $c.backup.add_Click({
        $z = & $fBackup $c.savesRoot
        $c.savesNote.Text = $(if ($z) { "Backed up: $z" } else { 'No saves yet.' })
    }.GetNewClosure())
    $c.restore.add_Click({
        $d = New-Object System.Windows.Forms.OpenFileDialog
        $d.InitialDirectory = $backupDir
        $d.Filter = 'Save backups (*.zip)|*.zip'
        if ($d.ShowDialog() -ne 'OK') { return }
        $ok = [System.Windows.Forms.MessageBox]::Show("Replace all your saves with this backup?`r`nYour current saves are backed up first.", 'KLBF Lab', 'YesNo', 'Question')
        if ($ok -ne 'Yes') { return }
        $z = & $fRestore $d.FileName $c.savesRoot
        & $fSaves $c
        $c.savesNote.Text = "Restored. The saves before it: $z"
    }.GetNewClosure())
    $c.copy.add_Click({
        $from = $c.saves | Where-Object slot -eq $c.copyFrom.SelectedItem
        $to = $c.saves | Where-Object slot -eq $c.copyTo.SelectedItem
        if (-not $from -or -not $to -or $from.dir -eq $to.dir) { $c.savesNote.Text = 'Pick two different saves.'; return }
        $ask = if ($c.confirm) { $c.confirm } else { { param($t) [System.Windows.Forms.MessageBox]::Show($t, 'KLBF Lab', 'YesNo', 'Warning') -eq 'Yes' } }
        if (-not (& $ask "Copy $($from.slot) ($($from.title)) over $($to.slot) ($($to.title))?`r`nAll saves are backed up first.")) { return }
        $z = & $fCopy $from $to $c.savesRoot
        & $fSaves $c
        $c.savesNote.Text = "Copied. The saves before it: $z"
    }.GetNewClosure())
    $c.play.add_Click({ $c.form.Tag = 'play'; $c.form.Close() }.GetNewClosure())
    $c.prepare.add_Click({ $c.form.Tag = 'prepare'; $c.form.Close() }.GetNewClosure())
    @{ form = $f; c = $c }
}

# The Saves tab from the saves folder.
function Update-KlbfSavesTab($c) {
    $c.saves = @(Get-KlbfSaves $c.savesRoot)
    $c.savesList.Items.Clear(); $c.copyFrom.Items.Clear(); $c.copyTo.Items.Clear()
    foreach ($s in $c.saves) {
        $it = New-Object System.Windows.Forms.ListViewItem($s.slot)
        [void]$it.SubItems.Add($s.title); [void]$it.SubItems.Add($s.date.ToString('yyyy-MM-dd HH:mm')); [void]$it.SubItems.Add('{0:N0} KB' -f ($s.size / 1KB))
        [void]$c.savesList.Items.Add($it)
        [void]$c.copyFrom.Items.Add($s.slot); [void]$c.copyTo.Items.Add($s.slot)
    }
    if (-not $c.saves) { $c.savesNote.Text = $(if ($c.savesRoot) { 'No saves yet.' } else { 'No saves folder yet: it appears after the first save in the game.' }) }
}

function Set-KlbfFormChoice($c, $Choice) {
    if ($Choice.game) { $c.game.Text = $Choice.game }
    $c.profX.Checked = $Choice.profile -ne 'S'; $c.profS.Checked = $Choice.profile -eq 'S'
    $c.upFsr.Checked = $Choice.upscaler -ne 'title'; $c.upTitle.Checked = $Choice.upscaler -eq 'title'
    $c.presets[$Choice.preset].Checked = $true
    $c.output.SelectedIndex = $c.output.Items.IndexOf($Choice.output)
    $c.fg.Checked = $Choice.frameGen; $c.adv.Checked = $Choice.advanced
    Update-KlbfForm $c
}

function Get-KlbfFormChoice($c) {
    ConvertTo-KlbfChoice ([pscustomobject]@{
        profile = $(if ($c.profS.Checked) { 'S' } else { 'X' })
        upscaler = $(if ($c.upTitle.Checked) { 'title' } else { 'fsr' })
        preset = @($c.presets.Keys | Where-Object { $c.presets[$_].Checked })[0]
        output = [string]$c.output.SelectedItem
        frameGen = [bool]$c.fg.Checked
        advanced = [bool]$c.adv.Checked
        game = [string]$c.game.Text
    })
}

# What applies to what: Profile S has the title's FSR, render size and output; frame generation needs FSR.
function Update-KlbfForm($c) {
    $s = $c.profS.Checked
    $fsr = $s -or $c.upFsr.Checked
    $c.upFsr.Enabled = -not $s; $c.upTitle.Enabled = -not $s
    foreach ($k in $c.presets.Keys) { $c.presets[$k].Enabled = -not $s -and $fsr }
    $c.output.Enabled = -not $s
    $c.outputNote.Text = $(if ($s) { 'Profile S: the title sets its own render size and output.' } else { '' })
    $c.fg.Enabled = $fsr
    # Play, bench and shaders need the game files
    $ok = Test-KlbfGameFolder $c.game.Text
    $c.gameDot.ForeColor = Get-KlbfColor $(if ($ok) { 0x22C55E } else { 0xEF4444 })
    $c.gameText.Text = if ($ok) { 'Game files found' } elseif ($c.game.Text) { 'This folder does not contain the game files' } else { 'Choose the folder where your copy of the game is installed (Browse...)' }
    foreach ($b in $c.play, $c.bench, $c.prepare) { $b.Enabled = $ok }
}

# Shows the window: @{ action = 'play'|'prepare'|'bench'; choice; benchPlayer; benchSave } or $null if closed.
function Show-KlbfMenu($Choice, [string]$ShaderStatus = '', [string]$Victory = '', [string]$Data = '', [string]$Game = '', [string]$Cache = '') {
    $os = Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue
    $info = @{ victory = $Victory; data = $Data; game = $Game; cache = $Cache; windows = $(if ($os) { "$($os.Caption) (build $($os.BuildNumber))" } else { '' }) }
    $m = New-KlbfMenuForm $Choice (Get-KlbfHardware) $ShaderStatus $info
    [void]$m.form.ShowDialog()
    if (-not $m.form.Tag) { return $null }
    @{ action = [string]$m.form.Tag; choice = Get-KlbfFormChoice $m.c; benchPlayer = $m.c.benchPlayer; benchSave = $m.c.benchSave }
}

# "You changed how the game is rendered..." with Prepare now / Later: $true for Prepare now.
function Confirm-KlbfPrepare([string]$Text) {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $f = New-Object System.Windows.Forms.Form
    $f.Text = 'KLBF Lab'; Set-KlbfTheme $f; $f.FormBorderStyle = 'FixedDialog'
    $f.MaximizeBox = $false; $f.MinimizeBox = $false; $f.StartPosition = 'CenterScreen'
    $f.ClientSize = New-Object System.Drawing.Size(420, 130)
    $l = New-Object System.Windows.Forms.Label; $l.Text = $Text; $l.SetBounds(16, 14, 388, 56); $f.Controls.Add($l)
    $now = New-Object System.Windows.Forms.Button; $now.Text = 'Prepare now'; $now.DialogResult = 'Yes'; $now.SetBounds(196, 84, 100, 30)
    $later = New-Object System.Windows.Forms.Button; $later.Text = 'Later'; $later.DialogResult = 'No'; $later.SetBounds(304, 84, 100, 30)
    Set-KlbfButton $now $true; Set-KlbfButton $later
    $f.Controls.AddRange(@($now, $later)); $f.AcceptButton = $now; $f.CancelButton = $later
    $f.ShowDialog() -eq 'Yes'
}

# Progress of `victory shaders` from its lines: "N shaders found in <file>" (the total, summed over files),
# "N shaders in the cache, S s" every 1000. @{ total; done } updated in place.
function Update-KlbfPrepareProgress($State, [string]$Line) {
    if ($Line -match '^(\d+) shaders found in ') { $State.total += [int]$Matches[1] }
    elseif ($Line -match '^(\d+) shaders in the cache') { $State.done = [int]$Matches[1] }
}

# Runs `victory shaders <rpf>` with a progress window and Cancel (env already set by play.ps1): $true when
# it finished. A bar when the exe says its total (fsrx3 and later), else a moving bar and the last line.
function Show-KlbfPrepare([string]$Victory, [string]$Rpf) {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $psi = New-Object System.Diagnostics.ProcessStartInfo($Victory)
    $psi.Arguments = 'shaders "' + $Rpf + '"'
    $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true
    $psi.RedirectStandardOutput = $true; $psi.RedirectStandardError = $true
    $f = New-Object System.Windows.Forms.Form
    $f.Text = 'KLBF Lab'; Set-KlbfTheme $f; $f.FormBorderStyle = 'FixedDialog'
    $f.MaximizeBox = $false; $f.MinimizeBox = $false; $f.StartPosition = 'CenterScreen'
    $f.ClientSize = New-Object System.Drawing.Size(460, 140)
    $head = New-Object System.Windows.Forms.Label; $head.Text = 'Preparing shaders (a few minutes, once per build)...'; $head.SetBounds(16, 12, 428, 20)
    $bar = New-Object System.Windows.Forms.ProgressBar; $bar.SetBounds(16, 38, 428, 22); $bar.Style = 'Marquee'
    $last = New-Object System.Windows.Forms.Label; $last.SetBounds(16, 66, 428, 20)
    $cancel = New-Object System.Windows.Forms.Button; $cancel.Text = 'Cancel'; $cancel.SetBounds(344, 98, 100, 30)
    Set-KlbfButton $cancel
    $f.Controls.AddRange(@($head, $bar, $last, $cancel))
    $st = @{ total = 0; done = 0; read = $null; ok = $false; p = $null }
    $fProgress = ${function:Update-KlbfPrepareProgress}
    $timer = New-Object System.Windows.Forms.Timer; $timer.Interval = 200
    $timer.add_Tick({
        while ($st.read -and $st.read.IsCompleted) {
            $line = $st.read.Result
            if ($null -eq $line) { $st.read = $null; break }
            & $fProgress $st $line
            $last.Text = $line
            $st.read = $st.p.StandardOutput.ReadLineAsync()
        }
        if ($st.total -gt 0) {
            $bar.Style = 'Continuous'; $bar.Maximum = $st.total; $bar.Value = [math]::Min($st.done, $st.total)
            $last.Text = '{0} of {1} shaders' -f $st.done, $st.total
        }
        if (-not $st.read -and $st.p.HasExited) {
            $st.ok = $st.p.ExitCode -eq 0
            $f.Close()
        }
    }.GetNewClosure())
    $cancel.add_Click({ try { $st.p.Kill() } catch {}; $f.Close() }.GetNewClosure())
    $f.add_Shown({
        $st.p = [System.Diagnostics.Process]::Start($psi)
        $null = $st.p.StandardError.ReadToEndAsync()
        $st.read = $st.p.StandardOutput.ReadLineAsync()
        $timer.Start()
    }.GetNewClosure())
    $f.add_FormClosing({ $timer.Stop(); if ($st.p -and -not $st.p.HasExited) { try { $st.p.Kill() } catch {} } }.GetNewClosure())
    [void]$f.ShowDialog()
    $st.ok
}
