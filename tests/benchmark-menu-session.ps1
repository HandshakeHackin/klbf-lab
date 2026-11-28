# Exercise the stable-amd launcher's menu dispatch and real child-process boundary on Windows.
param(
    [Parameter(Mandatory)][string]$Source,
    [switch]$ExpectKnownLeak
)
$ErrorActionPreference = 'Stop'
$Source = (Resolve-Path $Source).Path
$fixture = New-Item -ItemType Directory (Join-Path ([IO.Path]::GetTempPath()) ('klbf-menu-' + [guid]::NewGuid().ToString('N')))
try {
    $dir = $fixture.FullName
    Copy-Item (Join-Path $Source 'play.ps1'), (Join-Path $Source 'launcher-menu.ps1') $dir
    # Substitute the interactive controls and external SDK download in this private copy.
    # The launcher still reads/writes settings, validates cache readiness, and starts its child.
    @'

function Install-KlbfComponent { return $true }
function Invoke-KlbfShortcutOffer {}
function Show-KlbfMenu($Choice) {
    if ($global:menuChoices.Count -eq 0) { throw 'Unexpected extra menu' }
    $action = $global:menuChoices.Dequeue()
    Write-Host "Choosing $action in Settings"
    return @{ choice = $Choice; action = $action; benchPlayer = 'fixture'; benchWhere = 'test-area' }
}
'@ | Add-Content -LiteralPath (Join-Path $dir 'launcher-menu.ps1')
    @'
[pscustomobject]@{
    mode = [string]$env:KLBF_BENCH
    player = [string]$env:KLBF_BENCH_PLAYER
    length = [string]$env:KLBF_BENCH_LENGTH
    area = [string]$env:KLBF_BENCH_WHERE
    scripted = [bool]$env:VICTORY_AUTO_INPUT
} | ConvertTo-Json -Compress | Add-Content -LiteralPath $env:TEST_RECORD -Encoding utf8
'@ | Set-Content -LiteralPath (Join-Path $dir 'record.ps1') -Encoding utf8
    @('@echo off', 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "%~dp0record.ps1"', 'exit /b %errorlevel%') |
        Set-Content -LiteralPath (Join-Path $dir 'record.cmd') -Encoding ascii
    New-Item -ItemType File (Join-Path $dir 'game_scarlett_master.exe') | Out-Null
    @'
param([string]$Scenario)
$ErrorActionPreference = 'Stop'
$dir = $PSScriptRoot
$env:KLBF_HOME = Join-Path $dir 'home'
$env:KLBF_LAUNCHER_DIR = Join-Path $env:KLBF_HOME 'launcher'
$env:KLBF_OLD_HOMES = Join-Path $dir 'empty-old-home'
$env:KLBF_NO_DIALOG = '1'
$env:TEST_RECORD = Join-Path $dir 'games.jsonl'
New-Item -ItemType Directory $env:KLBF_HOME -Force | Out-Null
. (Join-Path $dir 'launcher-menu.ps1')
$settings = Join-Path $env:KLBF_LAUNCHER_DIR 'settings.json'
if (-not (Test-Path $settings)) {
    $c = Get-KlbfDefaults
    $c.upscaler = 'title'
    $c.frameGen = $false
    $c.advanced = $false
    Save-KlbfSettings $c $settings
}
$opts = @{ Game=$dir; Victory=(Join-Path $dir 'record.cmd'); Cache=(Join-Path $dir 'cache'); Data=(Join-Path $dir 'data'); SkipShaderCheck=$true; Setup=$true }
$play = Join-Path $dir 'play.ps1'
$global:menuChoices = New-Object 'System.Collections.Generic.Queue[string]'
switch ($Scenario) {
    cold {
        $global:menuChoices.Enqueue('bench')
        & $play @opts
        if ($LASTEXITCODE -ne 3) { throw "Cold benchmark returned $LASTEXITCODE" }
        $errFile = Join-Path $env:KLBF_HOME 'logs\launcher-error.txt'
        if ((Get-Content $errFile -Raw) -notlike '*shaders are not translated*') { throw 'Unexpected cold-cache rejection' }
        if (Test-Path $env:TEST_RECORD) { throw 'Cold benchmark started the game' }
        $global:menuChoices.Enqueue('play')
    }
    ready {
        $stamp = Get-KlbfShaderStamp $opts.Victory 'diag' '1' ''
        Set-Content (Join-Path $opts.Cache 'scarlett-shaders-ahead.txt') $stamp
        $pipelines = New-Item -ItemType Directory (Join-Path $opts.Cache 'pipelines') -Force
        1..1000 | Set-Content (Join-Path $pipelines.FullName 'early-pipelines.txt')
        $global:menuChoices.Enqueue('bench')
        $global:menuChoices.Enqueue('play')
    }
    fresh {
        if (-not (Test-Path $settings)) { throw 'Expected existing settings' }
        $global:menuChoices.Enqueue('play')
    }
}
& $play @opts
if ($LASTEXITCODE -ne 0 -or $global:menuChoices.Count -ne 0) { throw "Launcher did not complete $Scenario (exit $LASTEXITCODE)" }
'@ | Set-Content -LiteralPath (Join-Path $dir 'drive.ps1') -Encoding utf8
    $driver = Join-Path $dir 'drive.ps1'
    $record = Join-Path $dir 'games.jsonl'
    $shell = (Get-Process -Id $PID).Path
    function Assert-Child($row, [bool]$bench, [string]$area) {
        $mode = ''; $player = ''; $length = ''
        if ($bench) { $mode = 'bm-v1'; $player = 'fixture'; $length = 'short' }
        if ($row.mode -ne $mode -or $row.player -ne $player -or $row.length -ne $length -or $row.scripted -ne $bench -or $row.area -ne $area) {
            throw ('Unexpected game environment: ' + ($row | ConvertTo-Json -Compress))
        }
    }
    foreach ($stage in 'cold','ready','fresh') {
        if (Test-Path $record) { Remove-Item -LiteralPath $record }
        & $shell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $driver $stage
        if ($LASTEXITCODE -ne 0) { throw "Driver for $stage exited $LASTEXITCODE" }
        $rows = @(Get-Content -LiteralPath $record | ForEach-Object { $_ | ConvertFrom-Json })
        Write-Host ("{0}: {1}" -f $stage, (ConvertTo-Json -InputObject $rows -Compress))
        if ($stage -eq 'ready') {
            if ($rows.Count -ne 2) { throw 'Expected benchmark and nested ordinary Play' }
            Assert-Child $rows[0] $true 'test-area'
            Assert-Child $rows[1] ([bool]$ExpectKnownLeak) ''
        } else {
            if ($rows.Count -ne 1) { throw 'Expected one ordinary Play' }
            Assert-Child $rows[0] ($stage -eq 'cold' -and $ExpectKnownLeak) ''
        }
    }
} finally {
    Remove-Item -LiteralPath $fixture.FullName -Recurse -Force
}
