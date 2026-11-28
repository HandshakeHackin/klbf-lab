# Exercise the real Settings form on a short work area and after a display change.
param([switch]$ExpectOriginal)
$ErrorActionPreference = 'Stop'
$oldHome = $env:KLBF_HOME
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('klbf-screen-' + [guid]::NewGuid().ToString('N'))
$m = $null
try {
    $env:KLBF_HOME = $fixture
    New-Item -ItemType Directory -Path $fixture | Out-Null
    . (Join-Path (Split-Path -Parent $PSScriptRoot) 'launcher-menu.ps1')
    $hardware = @{ gpu = 'AMD Radeon RX 6600M'; vramGB = 8; ramGB = 16; screen = '1366x768' }
    $screens = @(
        @{ primary = $true; bounds = '1366x768'; work = @(0, 0, 1366, 728) }
        @{ primary = $false; bounds = '1280x720'; work = @(-1280, 24, 1280, 640) }
        @{ primary = $false; bounds = '1920x1080'; work = @(0, 0, 1920, 1040) }
        @{ primary = $false; bounds = '500x700'; work = @(0, 0, 500, 660) }
    )
    $info = @{ data = $fixture; victory = (Join-Path $fixture 'victory.exe'); windows = 'Windows 11'; screens = $screens }
    $m = New-KlbfMenuForm (Get-KlbfDefaults) $hardware 'Shaders: ready for this build.' $info
    $m.form.Show()
    [Windows.Forms.Application]::DoEvents()
    foreach ($index in 0,1,2,3,0) {
        $m.c.display.SelectedIndex = $index
        [Windows.Forms.Application]::DoEvents()
        $w = $screens[$index].work
        $area = New-Object Drawing.Rectangle($w[0], $w[1], $w[2], $w[3])
        $m.form.ScrollControlIntoView($m.c.play)
        [Windows.Forms.Application]::DoEvents()
        $client = $m.form.RectangleToScreen($m.form.ClientRectangle)
        $visible = [Drawing.Rectangle]::Intersect($area, $client)
        $play = $m.c.play.RectangleToScreen($m.c.play.ClientRectangle)
        $reachable = $visible.Contains($play)
        Write-Output "Work area $area, form $($m.form.Bounds), Play $play, reachable=$reachable"
        if ($ExpectOriginal) {
            if ($reachable) { throw 'The unchanged launcher unexpectedly reached Play on this work area' }
            break
        }
        if (-not $area.Contains($m.form.Bounds) -or -not $reachable) { throw 'Play is outside the selected work area' }
        $m.form.ScrollControlIntoView($m.c.tabs)
        [Windows.Forms.Application]::DoEvents()
        if ($m.c.tabs.Top -lt 0) { throw 'Settings did not scroll back to the first tab' }
    }
} finally {
    if ($m) { $m.form.Dispose() }
    $env:KLBF_HOME = $oldHome
    Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
}
