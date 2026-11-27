# KLBF Lab: the run logs, "Report a bug" and the preparation's steps, headless (no window, no game).
# powershell -NoProfile -File tests\launcher-logs.ps1
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$t = Join-Path ([IO.Path]::GetTempPath()) "klbf-logs-test-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $t | Out-Null
$env:KLBF_HOME = Join-Path $t 'home'
. (Join-Path $repo 'launcher-menu.ps1')
$fails = 0
function Check([string]$What, [bool]$Ok) { if ($Ok) { "ok    $What" } else { "FAIL  $What"; $script:fails++ } }

try {
    $logs = Get-KlbfLogDir
    Check 'logs in the home' ($logs -eq (Join-Path $env:KLBF_HOME 'logs'))

    # A run: its output and its errors in run-<date>.log, its exit code back, the script not stopped by an error line
    $code = Invoke-KlbfLogged "$env:SystemRoot\System32\cmd.exe" @('/c', 'echo said on output& echo said on errors 1>&2& exit /b 10') $logs
    $log = @(Get-ChildItem -LiteralPath $logs -Filter 'run-*.log')
    Check 'exit code passed on' ($code -eq 10)
    Check 'one run log' ($log.Count -eq 1)
    $text = Get-Content -LiteralPath $log[0].FullName -Raw
    Check 'output in the log' ($text -match 'said on output')
    Check 'errors in the log' ($text -match 'said on errors')
    Check 'exit code in the log' ($text -match 'exit code 10')

    # The last 10 kept
    1..14 | ForEach-Object { Set-Content -LiteralPath (Join-Path $logs ('run-2026-01-{0:d2}_00-00-00.log' -f $_)) -Value $_ }
    Set-Content -LiteralPath (Join-Path $logs 'crashes.log') -Value 'crash'
    Remove-KlbfOldLogs $logs
    $left = @(Get-ChildItem -LiteralPath $logs -Filter 'run-*.log' | Sort-Object Name)
    Check 'ten run logs kept' ($left.Count -eq 10)
    Check 'the newest kept' ($left[-1].Name -eq $log[0].Name)
    Check 'crashes.log never removed' (Test-Path -LiteralPath (Join-Path $logs 'crashes.log'))

    # Report a bug: the newest log, and the folder with the user's profile hidden
    (Get-Item -LiteralPath (Join-Path $logs 'crashes.log')).LastWriteTime = (Get-Date).AddMinutes(5)
    Check 'newest log found' ((Get-KlbfLatestLog '' $logs) -eq (Join-Path $logs 'crashes.log'))
    Check 'no log: none' ((Get-KlbfLatestLog (Join-Path $t 'none') (Join-Path $t 'none')) -eq '')
    $env:KLBF_HOME = Join-Path $env:USERPROFILE 'Saved Games\KLBF Lab'
    $hw = @{ gpu = 'AMD Radeon RX 9070 XT'; vramGB = 16; ramGB = 32; screen = '2560x1440' }
    $report = Get-KlbfBugReport (Get-KlbfDefaults) $hw '' '' 'Win 11'
    Check 'report names the logs folder' ($report -match [regex]::Escape('Logs folder: %USERPROFILE%\Saved Games\KLBF Lab\logs'))
    Check 'report hides the profile' (-not $report.Contains($env:USERPROFILE))

    # The preparation: the pipelines after the translation whenever a book exists, whatever else
    $book = Join-Path $t 'known-pipelines.bin'
    Check 'first start: translation only (no book)' ((@(Get-KlbfPreparePhases 'x.rpf' $book) | ForEach-Object { $_.what }) -join ',' -eq 'Translating shaders')
    Set-Content -LiteralPath $book -Value 'book'
    Check 'book: translation then pipelines' ((@(Get-KlbfPreparePhases 'x.rpf' $book) | ForEach-Object { $_.what }) -join ',' -eq 'Translating shaders,Preparing pipelines')
    Check 'pipelines alone' ((@(Get-KlbfPreparePhases 'x.rpf' $book -NoTranslate) | ForEach-Object { $_.what }) -join ',' -eq 'Preparing pipelines')
    Check 'nothing to do' (@(Get-KlbfPreparePhases 'x.rpf' '' -NoTranslate).Count -eq 0)

    # Prepared once for Play: the bench (same cache, same stamp) prepares nothing again; new pipelines written
    # down by a run are made at the next start
    $cache = Join-Path $t 'cache'
    $bookC = Join-Path $cache 'pipelines\known-pipelines.bin'
    Check 'no book: nothing to make' (-not (Test-KlbfWarmNeeded $bookC (Get-KlbfWarmMark $cache)))
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $bookC) | Out-Null; Set-Content -LiteralPath $bookC -Value 'book'
    Check 'book never prepared: make it' (Test-KlbfWarmNeeded $bookC (Get-KlbfWarmMark $cache))
    Set-KlbfWarmed $cache
    Check 'prepared for Play: the bench does not prepare again' (-not (Test-KlbfWarmNeeded $bookC (Get-KlbfWarmMark $cache)))
    (Get-Item -LiteralPath $bookC).LastWriteTimeUtc = (Get-Date).ToUniversalTime().AddMinutes(1)
    Check 'new pipelines written down: made at the next start' (Test-KlbfWarmNeeded $bookC (Get-KlbfWarmMark $cache))
    $play = Get-KlbfSwitches (Get-KlbfDefaults); $bench = Get-KlbfBenchSwitches (Get-KlbfDefaults) 'someone'
    Check 'the bench translates as Play does (same settings but -Bench)' ((@($bench.Keys | Where-Object { $_ -notin 'Bench', 'BenchPlayer' }) -join ',') -eq (@($play.Keys) -join ','))

    # The first-run shortcut offer: a yes makes it and says so; a failure is said and asked again; a no is remembered
    $env:KLBF_LAUNCHER_DIR = Join-Path $t 'launcher'
    $mark = Join-Path $env:KLBF_LAUNCHER_DIR 'shortcut-asked.txt'
    $build = Join-Path $t 'build'; New-Item -ItemType Directory -Path $build | Out-Null
    $lnk = Join-Path $t 'KLBF Lab.lnk'
    $yes = { param($q) $script:asked++; $true }
    $bad = Get-KlbfShortcutSpec (Join-Path $t 'no such folder')
    $script:asked = 0
    $r = Invoke-KlbfShortcutOffer $bad $yes $lnk
    Check 'failure said' ($r -like 'Desktop shortcut not created: *')
    Check 'failure not remembered as yes' (-not (Test-Path -LiteralPath $mark))
    $r = Invoke-KlbfShortcutOffer (Get-KlbfShortcutSpec $build) $yes $lnk
    Check 'asked again after a failure, then made' ($r -eq 'Desktop shortcut created.' -and $script:asked -eq 2 -and (Test-Path -LiteralPath $lnk) -and (Test-Path -LiteralPath (Join-Path $build 'KLBF Lab.vbs')))
    Check 'yes remembered once made' ((Get-Content -LiteralPath $mark -TotalCount 1) -eq 'yes')
    Check 'never asked twice' ((Invoke-KlbfShortcutOffer (Get-KlbfShortcutSpec $build) $yes $lnk) -eq '' -and $script:asked -eq 2)
    Remove-Item -LiteralPath $mark, $lnk
    Check 'no: nothing to say' ((Invoke-KlbfShortcutOffer (Get-KlbfShortcutSpec $build) { $false } $lnk) -eq '' -and -not (Test-Path -LiteralPath $lnk))
    Check 'no remembered' ((Get-Content -LiteralPath $mark -TotalCount 1) -eq 'no')
    $env:KLBF_HOME = Join-Path $t 'home'
    Add-KlbfLauncherLog 'Desktop shortcut created.'
    Check 'launcher log line' ((Get-Content -LiteralPath (Join-Path (Get-KlbfLogDir) 'launcher.log') -Raw) -match 'Desktop shortcut created\.')
} finally {
    Remove-Item -LiteralPath $t -Recurse -Force -ErrorAction SilentlyContinue
}
if ($fails) { "$fails check(s) failed"; exit 1 }
'all checks passed'
