# The Quick tab's result JSON must match the file text, including Unicode and whitespace.
# Run on Windows with powershell -NoProfile -File tests/benchmark-clipboard.ps1
$ErrorActionPreference = 'Stop'
$fixture = Join-Path ([IO.Path]::GetTempPath()) ("klbf-clip-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixture | Out-Null
$previousHome = $env:KLBF_HOME
$m = $null
try {
    $env:KLBF_HOME = $fixture
    . (Join-Path (Split-Path -Parent $PSScriptRoot) 'launcher-menu.ps1')
    $hardware = @{ gpu = 'AMD Radeon RX 6900 XT'; vramGB = 16; ramGB = 32; screen = '2560x1440' }
    $info = @{ data = $fixture; victory = (Join-Path $fixture 'victory.exe'); windows = 'Windows 11'; screens = @() }
    $dir = New-Item -ItemType Directory (Join-Path $fixture 'bench-results')
    $stem = Join-Path $dir.FullName 'KLBF-Bench-BM-v1-test'
    $clipboard = @{ text = ''; writes = 0 }

    foreach ($case in 'empty','card-only','complete') {
        if ($case -eq 'card-only') {
            Add-Type -AssemblyName System.Drawing
            $pic = New-Object System.Drawing.Bitmap(2, 2)
            try { $pic.Save("$stem.png", [System.Drawing.Imaging.ImageFormat]::Png) } finally { $pic.Dispose() }
        }
        if ($case -eq 'complete') {
            $json = @(
                '{'
                ('  "where": "caf' + [char]0x00e9 + '",')
                '  "fps": { "avg": 9.00, "low_1pct": 1.40 },'
                '  "settings": {"profile":"S","frame_generation":false},'
                '  "serial": "00abcd"'
                '}'
                ''
            ) -join [Environment]::NewLine
            [IO.File]::WriteAllText("$stem.json", $json, (New-Object Text.UTF8Encoding($false)))
        }
        $m = New-KlbfMenuForm (Get-KlbfDefaults) $hardware '' $info
        $m.c.clipboard = { param($text) $clipboard.text = $text; $clipboard.writes++ }.GetNewClosure()
        $m.c.tabs.SelectedIndex = 2
        $m.form.Show()
        [Windows.Forms.Application]::DoEvents()
        if ($m.c.copyJson.Enabled -ne ($case -eq 'complete')) { throw "Unexpected JSON button state for $case" }
        if ($case -eq 'complete') {
            $buttons = @($m.c.copyImage, $m.c.copyText, $m.c.copyJson, $m.c.quickNote)
            for ($i = 1; $i -lt $buttons.Count; $i++) {
                if ($buttons[$i-1].Bounds.IntersectsWith($buttons[$i].Bounds)) { throw 'Overlapping benchmark controls' }
            }
            $m.c.copyJson.PerformClick()
            if ($clipboard.writes -ne 1 -or $clipboard.text -cne $json) { throw 'Result file text changed in transit' }
            $m.c.copyText.PerformClick()
            if ($clipboard.writes -ne 2 -or $clipboard.text -notlike '*9.0 FPS*' -or $clipboard.text -eq $json) { throw 'Copy summary failed' }
            Remove-Item -LiteralPath "$stem.json"
            $m.c.copyJson.PerformClick()
            if ($clipboard.writes -ne 2 -or $m.c.quickNote.Text -notlike 'Could not copy*') { throw 'Missing result overwrote clipboard or showed success' }
        }
        $m.form.Dispose()
        $m = $null
        Write-Output "Passed: $case"
    }
} finally {
    if ($m) { $m.form.Dispose() }
    $env:KLBF_HOME = $previousHome
    Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
}
