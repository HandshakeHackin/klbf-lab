$root = Join-Path $env:RUNNER_TEMP 'session-fixture'
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
& .\play.ps1 @argsForGame
$before = Get-Content -Raw -LiteralPath $env:SESSION_RECORD
Write-Host "Fresh ordinary launch: $before"
if ($before -notmatch 'mode=\[\]' -or $before -notmatch 'controls=\[\]') { throw 'Unexpected initial environment' }
Remove-Item -LiteralPath $env:SESSION_RECORD
& .\play.ps1 @argsForGame -Bench -BenchPlayer fixture -BenchLength 15
$bench = Get-Content -Raw -LiteralPath $env:SESSION_RECORD
Write-Host "Completed benchmark child: $bench"
if ($bench -notmatch 'mode=\[bm-v1\] player=\[fixture\] length=\[medium\]' -or $bench -notmatch 'controls=\[rep\(') { throw 'Benchmark child did not receive the preset' }
Remove-Item -LiteralPath $env:SESSION_RECORD
& .\play.ps1 @argsForGame
$after = Get-Content -Raw -LiteralPath $env:SESSION_RECORD
Write-Host "Ordinary launch after completed benchmark: $after"
if ($after -notmatch 'mode=\[bm-v1\] player=\[fixture\] length=\[medium\]' -or $after -notmatch 'controls=\[rep\(') { throw 'Expected pre-fix behavior was not reproduced' }
