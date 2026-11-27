# KLBF Lab: the launcher's updater, headless (no window, no network): a fake v26.11.1 build and a fake v26.11.2
# release (JSON + zip made by scripts\package-release.ps1). powershell -NoProfile -File tests\launcher-update.ps1
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$t = Join-Path ([IO.Path]::GetTempPath()) "klbf-update-test-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $t | Out-Null
$env:KLBF_LAUNCHER_DIR = Join-Path $t 'launcher'
. (Join-Path $repo 'launcher-menu.ps1')
$fails = 0
function Check([string]$What, [bool]$Ok) { if ($Ok) { "ok    $What" } else { "FAIL  $What"; $script:fails++ } }
function Get-Sums([string]$Dir) { $h = @{}; Get-ChildItem -LiteralPath $Dir -File | ForEach-Object { $h[$_.Name] = (Get-FileHash -LiteralPath $_.FullName).Hash }; $h }
function Same($a, $b) { ($a.Keys.Count -eq $b.Keys.Count) -and -not @($a.Keys | Where-Object { $a[$_] -ne $b[$_] }) }

try {
    # Two releases: victory.exe differs (a text file stands for it)
    $zips = @{}
    foreach ($v in 'v26.11.1', 'v26.11.2') {
        $exe = Join-Path $t "victory-$v.exe"; Set-Content -LiteralPath $exe -Value "fake victory $v"
        $zips[$v] = & (Join-Path $repo 'scripts\package-release.ps1') -Tag $v -Victory $exe -Out (Join-Path $t "dist-$v") -Source $repo
    }
    $sha = (Get-FileHash -Algorithm SHA256 -LiteralPath $zips['v26.11.2']).Hash.ToLower()
    $json = Join-Path $t 'latest.json'
    @{ tag_name = 'v26.11.2'; html_url = 'https://github.com/PastiiJay/klbf-lab/releases/tag/v26.11.2'
        body = "Download KLBF-Lab-win64.zip ... SHA-256 of the zip: $sha"
        assets = @(@{ name = 'KLBF-Lab-win64.zip'; browser_download_url = $zips['v26.11.2'] }) } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $json
    # A fresh v26.11.1 build folder with the player's extras
    $n = 0
    $newBuild = {
        $d = Join-Path $t "build$script:n"; $script:n++
        Expand-Archive -LiteralPath $zips['v26.11.1'] -DestinationPath $d
        New-Item -ItemType Directory -Path (Join-Path $d 'bench-results'), (Join-Path $d 'D3D12') | Out-Null
        Set-Content (Join-Path $d 'bench-results\card.txt') 'my card'; Set-Content (Join-Path $d 'D3D12\D3D12Core.dll') 'component'
        Set-Content (Join-Path $d 'game-folder.txt') 'D:\Games\Mine'
        $d
    }

    # (27/11) The fallback starter in the zip, the README says what to do if it doesn't start
    $probe = & $newBuild
    $cmd = Join-Path $probe 'KLBF Lab.cmd'
    Check 'zip: KLBF Lab.cmd starts play.ps1 hidden with -Setup' ((Test-Path -LiteralPath $cmd) -and (Get-Content -LiteralPath $cmd -Raw) -match '^@start "" powershell\.exe .*-WindowStyle Hidden -File "%~dp0play\.ps1" -Victory "%~dp0victory\.exe" -Setup')
    Check "zip: README's if it doesn't start" ((Get-Content -LiteralPath (Join-Path $probe 'README.txt') -Raw) -match "If it doesn't start" )
    # (27/11) The internet mark goes from the folder's files
    $marked = Join-Path $probe 'play.ps1'
    Set-Content -LiteralPath $marked -Stream Zone.Identifier -Value "[ZoneTransfer]`r`nZoneId=3"
    Unblock-KlbfFolder $probe
    Check 'launcher unblocks its own files' (-not (Get-Item -LiteralPath $marked -Stream * | Where-Object Stream -eq 'Zone.Identifier'))

    $b = & $newBuild; $before = Get-Sums $b
    Check 'VERSION read from the build' ((Get-KlbfVersion $b) -eq 'v26.11.1')
    Check 'no VERSION (source checkout): no version' ((Get-KlbfVersion $t) -eq '')
    $u = Get-KlbfUpdate 'v26.11.1' $json
    Check 'newer release found, zip and SHA-256 from the notes' ($u.tag -eq 'v26.11.2' -and $u.sha -eq $sha -and $u.zip -eq $zips['v26.11.2'])
    Check 'same version: no update' ($null -eq (Get-KlbfUpdate 'v26.11.2' $json))
    Check 'source checkout: no update' ($null -eq (Get-KlbfUpdate '' $json))
    # YY.MM.N compared as numbers; the old v0.x tags are older
    $order = { param($have, $latest) Set-Content -LiteralPath $json -Value ((Get-Content -LiteralPath $json -Raw) -replace '"tag_name":\s*"[^"]*"', "`"tag_name`": `"$latest`"") ; $null -ne (Get-KlbfUpdate $have $json) }
    Check 'YY.MM.N order: 26.11.1 < 26.11.2 < 26.12.1 < 27.1.1, v0.27.0 older' ((& $order 'v26.11.2' 'v26.12.1') -and (& $order 'v26.12.1' 'v27.1.1') -and (& $order 'v26.11.9' 'v26.11.10') -and (& $order 'v0.27.0' 'v26.11.1') -and -not (& $order 'v26.11.1' 'v0.28.0') -and -not (& $order 'v26.12.1' 'v26.11.2'))
    $null = & $order 'v26.11.1' 'v26.11.2'
    Check 'offline check: silent $null' ($null -eq (Get-KlbfUpdate 'v26.11.1' 'http://127.0.0.1:9/latest'))
    # (27/11) The state beside the version
    $dot = [char]0xB7
    $s = Get-KlbfUpdateCheck 'v26.11.1' $json
    Check 'state: update' ($s.state -eq 'update' -and $s.update.tag -eq 'v26.11.2' -and (Get-KlbfVersionLine 'v26.11.1' $s) -eq "v26.11.1 $dot")
    $s = Get-KlbfUpdateCheck 'v26.11.2' $json
    Check 'state: up to date' ($s.state -eq 'latest' -and (Get-KlbfVersionLine 'v26.11.2' $s) -eq "v26.11.2 $dot Up to date")
    $s = Get-KlbfUpdateCheck 'v26.11.1' 'http://127.0.0.1:9/latest'
    Check "state: couldn't check (offline)" ($s.state -eq 'unknown' -and (Get-KlbfVersionLine 'v26.11.1' $s) -eq "v26.11.1 $dot Couldn't check for updates")
    $garbled = Join-Path $t 'garbled.json'; Set-Content -LiteralPath $garbled -Value '{"message": "API rate limit exceeded"}'
    Check "state: couldn't check (rate limit)" ((Get-KlbfUpdateCheck 'v26.11.1' $garbled).state -eq 'unknown')
    Check 'state: source checkout' ((Get-KlbfUpdateCheck '' $json).state -eq 'none' -and (Get-KlbfVersionLine '' $null) -eq 'dev')

    $r = Install-KlbfUpdate @{ tag = 'v26.11.2'; zip = 'http://127.0.0.1:9/x.zip'; sha = $sha } $b
    Check "offline download refused, unchanged ($($r.message))" (-not $r.ok -and (Same $before (Get-Sums $b)))
    $r = Install-KlbfUpdate @{ tag = 'v26.11.2'; zip = $u.zip; sha = ('0' * 64) } $b
    Check "bad zip hash refused, unchanged ($($r.message))" (-not $r.ok -and $r.message -like '*SHA-256*' -and (Same $before (Get-Sums $b)))
    $r = Install-KlbfUpdate @{ tag = 'v26.11.2'; zip = $u.zip; sha = '' } $b
    Check 'no SHA-256 in the notes refused' (-not $r.ok -and (Same $before (Get-Sums $b)))

    # A zip whose hash is right (in the notes) but one file differs from its SHA256SUMS.txt
    $bad = Join-Path $t 'bad'; Expand-Archive -LiteralPath $u.zip -DestinationPath $bad
    Add-Content -LiteralPath (Join-Path $bad 'victory.exe') 'tampered'
    $badZip = Join-Path $t 'bad.zip'; Compress-Archive -Path (Join-Path $bad '*') -DestinationPath $badZip
    $r = Install-KlbfUpdate @{ tag = 'v26.11.2'; zip = $badZip; sha = (Get-FileHash -Algorithm SHA256 -LiteralPath $badZip).Hash.ToLower() } $b
    Check "file not matching SHA256SUMS.txt refused ($($r.message))" (-not $r.ok -and $r.message -like '*victory.exe does not match*' -and (Same $before (Get-Sums $b)))

    # The game running from the build folder (ping.exe, hidden, stands for it)
    $game = Join-Path $b 'bench-results\victory.exe'; Copy-Item (Join-Path $env:SystemRoot 'System32\PING.EXE') $game
    $p = Start-Process -FilePath $game -ArgumentList '-n', '30', '127.0.0.1' -WindowStyle Hidden -PassThru
    Start-Sleep -Milliseconds 500
    $r = Install-KlbfUpdate $u $b
    Stop-Process -Id $p.Id -Force; $p.WaitForExit(); Remove-Item -LiteralPath $game
    Check "game running refused ($($r.message))" (-not $r.ok -and $r.message -like '*game is running*' -and (Same $before (Get-Sums $b)))

    # A locked file of the build: the old files moved so far go back
    $lock = [IO.File]::Open((Join-Path $b 'play.ps1'), 'Open', 'Read', 'Read')
    $r = Install-KlbfUpdate $u $b
    $lock.Close()
    Check "locked build file: rolled back ($($r.message))" (-not $r.ok -and $r.message -like '*put back*' -and (Same $before (Get-Sums $b)))

    # A locked new file while moving in: the old files and the moved-in new ones go back
    $b2 = & $newBuild; $before2 = Get-Sums $b2
    $stage = Join-Path $t 'stage'; Expand-Archive -LiteralPath $u.zip -DestinationPath $stage; $staged = Get-Sums $stage
    $lock = [IO.File]::Open((Join-Path $stage 'victory.exe'), 'Open', 'Read', 'Read')
    $threw = $false; try { Switch-KlbfBuild $stage $b2 } catch { $threw = $true }
    $lock.Close()
    Check 'locked new file: rolled back, stage intact' ($threw -and (Same $before2 (Get-Sums $b2)) -and (Same $staged (Get-Sums $stage)))

    # The good update
    $r = Install-KlbfUpdate $u $b
    Check "good update installed ($($r.message))" ($r.ok -and $r.message -eq "Updated to v26.11.2 $([char]0x2014) shaders will be prepared again")
    Check 'new files in place, all match SHA256SUMS.txt' ((& { $now = Get-Sums $b; -not @($staged.Keys | Where-Object { $now[$_] -ne $staged[$_] }) }) -and (Get-KlbfVersion $b) -eq 'v26.11.2' -and (Get-Content (Join-Path $b 'victory.exe')) -eq 'fake victory v26.11.2')
    Check 'bench-results, D3D12 and game-folder.txt kept' ((Get-Content (Join-Path $b 'bench-results\card.txt')) -eq 'my card' -and (Test-Path (Join-Path $b 'D3D12\D3D12Core.dll')) -and (Test-Path (Join-Path $b 'game-folder.txt')))
    Check 'old files inside the folder (.previous-v26.11.1), nothing beside it' ((Get-Content (Join-Path $b '.previous-v26.11.1\victory.exe')) -eq 'fake victory v26.11.1' -and -not (Test-Path "$b.previous") -and -not (Test-Path "$b.new") -and -not (Test-Path (Join-Path $b '.new-v26.11.2')))

    # (27/11) The Update link's tick, as the window runs it: a finished install closes the window to reopen it
    # (its closure held nothing before: "The property 'updating' cannot be found")
    Add-Type -AssemblyName System.Windows.Forms
    $form = New-Object System.Windows.Forms.Form
    $cc = @{ form = $form; updating = $true; status = New-Object System.Windows.Forms.Label; update = New-Object System.Windows.Forms.LinkLabel; play = New-Object System.Windows.Forms.Button; gamePath = ''; quiet = $true }
    $done = { param($ok) $p = [powershell]::Create(); $null = $p.AddScript("@{ ok = `$$ok; message = 'Updated to v26.11.2' }"); $j = $p.BeginInvoke(); $null = $j.AsyncWaitHandle.WaitOne(10000); @{ ps = $p; job = $j } }
    $timer = New-Object System.Windows.Forms.Timer
    $tick = Get-KlbfUpdateTick $cc (& $done 'true') $timer { $true }
    $err = try { & $tick; '' } catch { "$_" }
    Check "update tick: no error, window closes to reopen ($err)" (-not $err -and -not $cc.updating -and $form.Tag -eq 'updated')
    $form2 = New-Object System.Windows.Forms.Form; $cc.form = $form2; $cc.updating = $true
    $err = try { & (Get-KlbfUpdateTick $cc (& $done 'false') $timer { $true }); '' } catch { "$_" }
    Check "update tick, failed install: said, window stays ($err)" (-not $err -and -not $cc.updating -and $form2.Tag -ne 'updated' -and $cc.status.Text -eq 'Updated to v26.11.2' -and $cc.update.Enabled)
    $env:KLBF_UPDATED = $null

    # Components checked by hash: a file of another version is fetched again
    Check 'component with another hash is not "there"' (-not (Install-KlbfComponent agility $b -CheckOnly))
} finally {
    Remove-Item -LiteralPath $t -Recurse -Force -ErrorAction SilentlyContinue
}
if ($fails) { "$fails failed"; exit 1 } else { 'all passed' }
