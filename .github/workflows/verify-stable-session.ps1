$root = Join-Path $env:RUNNER_TEMP 'stable-session-fixture'
$d3d = Join-Path $root 'D3D12'
New-Item -ItemType Directory -Path $d3d -Force | Out-Null
foreach ($path in @('game_scarlett_master.exe','D3D12/D3D12Core.dll')) {
    New-Item -ItemType File -Path (Join-Path $root $path) | Out-Null
}
$stub = Join-Path $root 'record-env.cmd'
$env:SESSION_RECORD = Join-Path $root 'launch.txt'
Set-Content -LiteralPath $stub -Encoding ascii -Value @(
    '@echo off'
    'setlocal EnableDelayedExpansion'
    '> "%SESSION_RECORD%" echo mode=[%KLBF_BENCH%] player=[%KLBF_BENCH_PLAYER%] length=[%KLBF_BENCH_LENGTH%]'
    '>> "%SESSION_RECORD%" echo controls=[!VICTORY_AUTO_INPUT!]'
    'exit /b 0'
)
$argsForGame = @{ Game=$root; Victory=$stub; Cache=(Join-Path $root 'cache'); Data=(Join-Path $root 'data'); NoPrecompile=$true }
foreach ($name in 'KLBF_BENCH', 'KLBF_BENCH_PLAYER', 'KLBF_BENCH_LENGTH', 'VICTORY_AUTO_INPUT') {
    [Environment]::SetEnvironmentVariable($name, $null, 'Process')
}

& .\play.ps1 @argsForGame
$before = Get-Content -Raw -LiteralPath $env:SESSION_RECORD
Write-Host "Fresh ordinary launch: $before"
if ($before -notmatch 'mode=\[\] player=\[\] length=\[\]' -or $before -notmatch 'controls=\[\]') {
    throw 'Unexpected initial environment'
}

Remove-Item -LiteralPath $env:SESSION_RECORD
try {
    & .\play.ps1 @argsForGame -Bench -BenchPlayer rejected -BenchLength 15 -Fsr -FfxDir (Join-Path $root 'missing-ffx')
    throw 'Expected missing-FSR rejection'
} catch {
    if ($_.Exception.Message -notmatch 'AMD FSR not found') { throw }
}
if (Test-Path -LiteralPath $env:SESSION_RECORD) { throw 'Rejected benchmark launched the game' }
& .\play.ps1 @argsForGame
$afterError = Get-Content -Raw -LiteralPath $env:SESSION_RECORD
Write-Host "Ordinary launch after rejected benchmark: $afterError"
if ($afterError.Trim() -ne $before.Trim()) { throw 'Rejected benchmark changed the next ordinary launch' }

$env:VICTORY_AUTO_INPUT = 'manual-sequence'
$env:KLBF_BENCH_PLAYER = 'original-player'
& .\play.ps1 @argsForGame -Bench -BenchLength 15 -BenchPlayer fixture
$bench = Get-Content -Raw -LiteralPath $env:SESSION_RECORD
Write-Host "Accepted benchmark: $bench"
if ($bench -notmatch 'mode=\[bm-v1\] player=\[fixture\] length=\[medium\]' -or $bench -notmatch 'controls=\[rep\(') {
    throw 'Child did not receive benchmark settings'
}
& .\play.ps1 @argsForGame
$afterBench = Get-Content -Raw -LiteralPath $env:SESSION_RECORD
Write-Host "Ordinary launch after accepted benchmark: $afterBench"
if ($afterBench -notmatch 'mode=\[\] player=\[original-player\] length=\[\]' -or $afterBench -notmatch 'controls=\[manual-sequence\]') {
    throw 'Caller settings were not restored'
}
