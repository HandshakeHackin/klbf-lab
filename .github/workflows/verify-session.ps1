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
try { & .\play.ps1 @argsForGame -Bench -BenchPlayer fixture -BenchLength 15; throw 'Expected benchmark rejection' }
catch { if ($_.Exception.Message -notmatch 'shaders are not translated') { throw } }
if (Test-Path -LiteralPath $env:SESSION_RECORD) { throw 'Rejected bench launched the game' }
& .\play.ps1 @argsForGame
$after = Get-Content -Raw -LiteralPath $env:SESSION_RECORD
Write-Host "Ordinary launch after rejected benchmark: $after"
if ($after.Trim() -ne $before.Trim()) { throw 'Rejected bench changed the next ordinary game' }
$mark = '{0}|{1}|diag|probe=1|rays=' -f (Get-Item $stub).Length, (Get-Item $stub).LastWriteTimeUtc.Ticks
Set-Content -LiteralPath (Join-Path $argsForGame.Cache 'scarlett-shaders-ahead.txt') -Value $mark -Encoding utf8
$pipes = Join-Path $argsForGame.Cache 'pipelines'
New-Item -ItemType Directory -Path $pipes | Out-Null
1..1000 | Set-Content -LiteralPath (Join-Path $pipes 'early-pipelines.txt')
$env:VICTORY_AUTO_INPUT = 'manual-sequence'
$env:KLBF_BENCH_PLAYER = 'original-player'
& .\play.ps1 @argsForGame -Bench -BenchLength 15 -BenchPlayer fixture
$bench = Get-Content -Raw -LiteralPath $env:SESSION_RECORD
Write-Host "Accepted benchmark: $bench"
if ($bench -notmatch 'mode=\[bm-v1\] player=\[fixture\] length=\[medium\]' -or $bench -notmatch 'controls=\[rep\(') { throw 'Child did not receive bench settings' }
& .\play.ps1 @argsForGame
$post = Get-Content -Raw -LiteralPath $env:SESSION_RECORD
Write-Host "Ordinary launch after accepted benchmark: $post"
if ($post -notmatch 'mode=\[\] player=\[original-player\] length=\[\]' -or $post -notmatch 'controls=\[manual-sequence\]') { throw 'Caller settings were not restored' }
