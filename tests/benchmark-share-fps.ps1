# Run in Windows PowerShell 5.1 or PowerShell 7. No GPU or game files are needed.
param([string]$ResultFile, [switch]$ExpectOriginal)
$ErrorActionPreference = 'Stop'
. (Join-Path (Split-Path -Parent $PSScriptRoot) 'launcher-menu.ps1')

function Assert-Rates($Result, [string]$Expected) {
    $headline = ((Get-KlbfBenchShareText $Result) -split '\r?\n')[0]
    if (-not $headline.EndsWith($Expected, [StringComparison]::Ordinal)) {
        throw "Expected [$Expected], got [$headline]"
    }
    Write-Output $headline
}

# Keep only the counters needed from #156, without device or player identifiers.
$run = @{ fps = @{ avg = 18.82; low_1pct = 5.09 }; fps_shown = @{ avg = 35.32; low_1pct = 9.55 }
    settings = @{ frame_generation = $true }; gpu_vram_gb = 16; ram_gb = 32 }
if ($ResultFile) { $publicRun = Get-Content -LiteralPath $ResultFile -Raw -Encoding UTF8 | ConvertFrom-Json }
$recorded = '35.32 FPS (base 18.82), 1% low 9.55 (base 5.09)'
if ($ExpectOriginal) {
    Assert-Rates $run '38 FPS (base 19), 1% low 5.1'
    if ($ResultFile) { Assert-Rates $publicRun '38 FPS (base 19), 1% low 5.1' }
    return
}
Assert-Rates $run $recorded
if ($ResultFile) { Assert-Rates $publicRun $recorded }
$run.fps_shown.Remove('low_1pct')
Assert-Rates $run '35.32 FPS (base 18.82), base 1% low 5.1'
$run.Remove('fps_shown')
Assert-Rates $run 'base 18.8 FPS, base 1% low 5.1'
$run.fps_shown = @{ avg = 0.0; low_1pct = 0.0 }
Assert-Rates $run '0.0 FPS (base 18.82), 1% low 0.0 (base 5.09)'
$run.settings.frame_generation = $false
Assert-Rates $run '18.8 FPS, 1% low 5.1'
