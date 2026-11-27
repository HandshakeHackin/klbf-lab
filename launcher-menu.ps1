# KLBF Lab: the launcher's setup window and shader preparation, dot-sourced by play.ps1.
# Settings: %LOCALAPPDATA%\KLBF-Lab\launcher\settings.json (the current choice) and presets\<name>.json
# (KLBF_LAUNCHER_DIR moves that folder, for tests).

$script:KlbfModes = '1280x720', '1664x936', '1920x1080', '2560x1440', '3840x2160'
# AMD's presets: play.ps1's -FsrQuality values and their names. Native AA (1.0) is not offered yet: at 1.0
# the Profile X memory plan overflows (victory-loader resolution.rs).
$script:KlbfPresets = [ordered]@{
    native = 'Native AA'; quality = 'Quality'; balanced = 'Balanced'; performance = 'Performance'; ultraperformance = 'Ultra Performance'
}

function Get-KlbfLauncherDir {
    if ($env:KLBF_LAUNCHER_DIR) { return $env:KLBF_LAUNCHER_DIR }
    Join-Path $env:LOCALAPPDATA 'KLBF-Lab\launcher'
}

# The owner's setting: Profile X, FSR 4 Performance, 3840x2160, frame generation, hooks and texture trim
# (= -Tout -FsrQuality performance -Output 3840x2160).
function Get-KlbfDefaults {
    [ordered]@{ profile = 'X'; upscaler = 'fsr'; preset = 'performance'; output = '3840x2160'; frameGen = $true; advanced = $true }
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
    $c
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

function New-KlbfFont([float]$Size = 9, [string]$Style = 'Regular') {
    New-Object System.Drawing.Font('Segoe UI', $Size, [System.Drawing.FontStyle]::$Style)
}

# The setup window, not shown: @{ form; c (its controls) }. $ShaderStatus: one line under the choices.
function New-KlbfMenuForm($Choice, [string]$Screen, [string]$ShaderStatus = '') {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    [System.Windows.Forms.Application]::EnableVisualStyles()
    $f = New-Object System.Windows.Forms.Form
    $f.Text = 'KLBF Lab'
    $f.Font = New-KlbfFont
    $f.FormBorderStyle = 'FixedDialog'; $f.MaximizeBox = $false; $f.MinimizeBox = $false
    $f.StartPosition = 'CenterScreen'
    $f.ClientSize = New-Object System.Drawing.Size(520, 560)
    $f.BackColor = [System.Drawing.Color]::White
    $tip = New-Object System.Windows.Forms.ToolTip
    $c = @{ tip = $tip }
    $add = { param($parent, $ctl, $x, $y, $w, $h) $ctl.SetBounds($x, $y, $w, $h); $parent.Controls.Add($ctl); $ctl }
    $label = { param($parent, $text, $x, $y, $w, $h = 20) $l = New-Object System.Windows.Forms.Label; $l.Text = $text; & $add $parent $l $x $y $w $h }
    $radio = { param($parent, $text, $x, $y, $w) $r = New-Object System.Windows.Forms.RadioButton; $r.Text = $text; & $add $parent $r $x $y $w 22 }
    $group = { param($text, $y, $h) $g = New-Object System.Windows.Forms.GroupBox; $g.Text = $text; & $add $f $g 16 $y 488 $h }

    $title = & $label $f 'How do you want to play?' 16 10 488 28
    $title.Font = New-KlbfFont 13 'Bold'

    $g = & $group 'Profile' 44 52
    $c.profX = & $radio $g 'Profile X (recommended)' 12 20 220
    $c.profS = & $radio $g 'Profile S' 250 20 220

    $g = & $group 'Upscaler' 102 118
    $c.upFsr = & $radio $g 'AMD FSR 4 (recommended)' 12 20 220
    $c.upTitle = & $radio $g "The title's own upscaler" 250 20 230
    $p = New-Object System.Windows.Forms.Panel
    [void](& $add $g $p 8 46 474 66)
    [void](& $label $p 'FSR preset:' 4 2 120)
    $c.presets = @{}
    $at = @{ native = 4, 22, 196; quality = 206, 22, 120; balanced = 332, 22, 130; performance = 4, 44, 196; ultraperformance = 206, 44, 200 }
    foreach ($k in $script:KlbfPresets.Keys) {
        $text = $script:KlbfPresets[$k] + $(if ($k -eq 'native') { ' (coming soon)' } else { '' })
        $c.presets[$k] = & $radio $p $text $at[$k][0] $at[$k][1] $at[$k][2]
    }
    $c.presets.native.Enabled = $false
    $tip.SetToolTip($c.presets.native, 'Coming soon')
    $tip.SetToolTip($p, 'Native AA: coming soon')

    $g = & $group 'Output resolution' 226 78
    $c.output = New-Object System.Windows.Forms.ComboBox
    $c.output.DropDownStyle = 'DropDownList'
    foreach ($m in $script:KlbfModes) { [void]$c.output.Items.Add($m) }
    [void](& $add $g $c.output 12 22 140 24)
    $c.screen = & $label $g "Your screen: $Screen" 170 25 300
    $c.outputNote = & $label $g '' 12 52 460

    $g = & $group 'Options' 310 74
    $c.fg = New-Object System.Windows.Forms.CheckBox
    $c.fg.Text = 'Frame generation (FG x2)'
    [void](& $add $g $c.fg 12 20 460 22)
    $c.adv = New-Object System.Windows.Forms.CheckBox
    $c.adv.Text = 'Advanced: memory hooks and texture trim (faster, uses less RAM)'
    [void](& $add $g $c.adv 12 44 470 22)

    $g = & $group 'Saved profiles' 390 58
    $c.saved = New-Object System.Windows.Forms.ComboBox
    $c.saved.DropDownStyle = 'DropDownList'
    foreach ($n in (Get-KlbfPresetNames)) { [void]$c.saved.Items.Add($n) }
    if ($c.saved.Items.Count) { $c.saved.SelectedIndex = 0 }
    [void](& $add $g $c.saved 12 22 200 24)
    $btn = { param($parent, $text, $x, $y, $w, $h = 28) $b = New-Object System.Windows.Forms.Button; $b.Text = $text; & $add $parent $b $x $y $w $h }
    $c.load = & $btn $g 'Load' 222 20 78
    $c.saveAs = & $btn $g "Save as$([char]0x2026)" 306 20 84
    $c.delete = & $btn $g 'Delete' 396 20 78

    $c.status = & $label $f $ShaderStatus 16 456 488 36
    $c.play = & $btn $f 'Play' 320 500 184 40
    $c.play.Font = New-KlbfFont 11 'Bold'
    $c.prepare = & $btn $f 'Prepare shaders' 16 500 184 40
    $f.AcceptButton = $c.play

    $c.form = $f
    Set-KlbfFormChoice $c $Choice
    # A closure sees variables, not this file's functions: they go in as variables.
    $fUpdate = ${function:Update-KlbfForm}; $fSet = ${function:Set-KlbfFormChoice}; $fGet = ${function:Get-KlbfFormChoice}
    $fPath = ${function:Get-KlbfPresetPath}; $fRead = ${function:Read-KlbfSettings}; $fSave = ${function:Save-KlbfSettings}
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
    $c.play.add_Click({ $c.form.Tag = 'play'; $c.form.Close() }.GetNewClosure())
    $c.prepare.add_Click({ $c.form.Tag = 'prepare'; $c.form.Close() }.GetNewClosure())
    @{ form = $f; c = $c }
}

function Set-KlbfFormChoice($c, $Choice) {
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
    })
}

# What applies to what: Profile S has the title's FSR, render size and output; frame generation needs FSR.
function Update-KlbfForm($c) {
    $s = $c.profS.Checked
    $fsr = $s -or $c.upFsr.Checked
    $c.upFsr.Enabled = -not $s; $c.upTitle.Enabled = -not $s
    foreach ($k in $c.presets.Keys) { $c.presets[$k].Enabled = -not $s -and $fsr -and $k -ne 'native' }
    $c.output.Enabled = -not $s
    $c.outputNote.Text = $(if ($s) { 'Profile S: the title sets its own render size and output.' } else { '' })
    $c.fg.Enabled = $fsr
}

# Shows the window: @{ action = 'play'|'prepare'; choice } or $null if closed.
function Show-KlbfMenu($Choice, [string]$ShaderStatus = '') {
    $m = New-KlbfMenuForm $Choice (Get-KlbfScreenSize) $ShaderStatus
    [void]$m.form.ShowDialog()
    if (-not $m.form.Tag) { return $null }
    @{ action = [string]$m.form.Tag; choice = Get-KlbfFormChoice $m.c }
}

# "You changed how the game is rendered..." with Prepare now / Later: $true for Prepare now.
function Confirm-KlbfPrepare([string]$Text) {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $f = New-Object System.Windows.Forms.Form
    $f.Text = 'KLBF Lab'; $f.Font = New-KlbfFont; $f.FormBorderStyle = 'FixedDialog'
    $f.MaximizeBox = $false; $f.MinimizeBox = $false; $f.StartPosition = 'CenterScreen'
    $f.ClientSize = New-Object System.Drawing.Size(420, 130)
    $l = New-Object System.Windows.Forms.Label; $l.Text = $Text; $l.SetBounds(16, 14, 388, 56); $f.Controls.Add($l)
    $now = New-Object System.Windows.Forms.Button; $now.Text = 'Prepare now'; $now.DialogResult = 'Yes'; $now.SetBounds(196, 84, 100, 30)
    $later = New-Object System.Windows.Forms.Button; $later.Text = 'Later'; $later.DialogResult = 'No'; $later.SetBounds(304, 84, 100, 30)
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
    $f.Text = 'KLBF Lab'; $f.Font = New-KlbfFont; $f.FormBorderStyle = 'FixedDialog'
    $f.MaximizeBox = $false; $f.MinimizeBox = $false; $f.StartPosition = 'CenterScreen'
    $f.ClientSize = New-Object System.Drawing.Size(460, 140)
    $head = New-Object System.Windows.Forms.Label; $head.Text = 'Preparing shaders (a few minutes, once per build)...'; $head.SetBounds(16, 12, 428, 20)
    $bar = New-Object System.Windows.Forms.ProgressBar; $bar.SetBounds(16, 38, 428, 22); $bar.Style = 'Marquee'
    $last = New-Object System.Windows.Forms.Label; $last.SetBounds(16, 66, 428, 20)
    $cancel = New-Object System.Windows.Forms.Button; $cancel.Text = 'Cancel'; $cancel.SetBounds(344, 98, 100, 30)
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
