# KLBF Lab: Settings' Start beside Play (game menu, new game, autosave, a slot), headless (no window, no game).
# powershell -NoProfile -File tests\launcher-start.ps1
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$t = Join-Path ([IO.Path]::GetTempPath()) "klbf-start-test-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $t | Out-Null
$env:KLBF_HOME = Join-Path $t 'home'
. (Join-Path $repo 'launcher-menu.ps1')
$fails = 0
function Check([string]$What, [bool]$Ok) { if ($Ok) { "ok    $What" } else { "FAIL  $What"; $script:fails++ } }
# A save container as the game writes one: manifest, data (0) and header (1) with its title at 0x24
function New-Save([string]$Root, [string]$Leaf, [string]$Title, [datetime]$When) {
    $v = Join-Path $Root "p\u\$Leaf\v1"
    New-Item -ItemType Directory -Force -Path $v | Out-Null
    Write-KlbfManifest (Join-Path $v 'manifest') @('SARC', 'Save', '0', '1')
    [IO.File]::WriteAllBytes((Join-Path $v '0'), [byte[]](1..64))
    [IO.File]::WriteAllBytes((Join-Path $v '1'), [byte[]](@(0) * 0x24) + [Text.Encoding]::Unicode.GetBytes($Title) + [byte[]](0, 0))
    Get-ChildItem -LiteralPath $v -File | ForEach-Object { $_.LastWriteTime = $When }
}

try {
    $data = Join-Path $t 'data'; $root = Join-Path $data 'title\saves'
    New-Item -ItemType Directory -Force -Path $root | Out-Null
    $p = Get-KlbfStartPlan 'new' $root
    Check 'new game, no save: the new-game presses' ($p.keys -eq $script:KlbfKeys.newGame -and -not $p.save)
    Check 'game menu: no presses' (-not (Get-KlbfStartPlan 'menu' $root).keys)

    $old = (Get-Date).AddDays(-3)
    New-Save $root 'SARC0~0000000000000001' '(Auto) A' $old.AddHours(2)
    New-Save $root 'SARC1~0000000000000002' 'B' $old.AddHours(1)
    New-Save $root 'SARC2~0000000000000003' 'C' $old
    $saves = @(Get-KlbfSaves $root)
    Check 'numbered as the menu: Autosave, Slot 1, Slot 2' (($saves.slot -join ',') -eq 'Autosave,Slot 1,Slot 2')
    $p = Get-KlbfStartPlan 'new' $root
    Check 'new game with saves: left to the player' (-not $p.keys -and $p.note -match 'New game')
    $p = Get-KlbfStartPlan 'auto' $root
    Check 'autosave: Continue on the autosave' ($p.keys -eq $script:KlbfKeys.continue -and $p.save.auto)
    $p = Get-KlbfStartPlan 'SARC2~0000000000000003' $root
    Check 'a slot: Continue on that save' ($p.keys -eq $script:KlbfKeys.continue -and $p.save.slot -eq 'Slot 2')
    Check 'a gone save: the game menu' (-not (Get-KlbfStartPlan 'SARC9~0000000000000009' $root).keys)

    # Made the newest: first in the Load list after the autosave, and the newest manifest (what the game reads)
    Set-KlbfNewestSave $p.save
    $saves = @(Get-KlbfSaves $root)
    Check 'picked save is now Slot 1' ((Split-Path -Leaf $saves[1].dir) -eq 'SARC2~0000000000000003')
    $newest = Get-ChildItem -Path (Join-Path $root '*\*\*\v1\manifest') | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    Check 'its manifest is the newest' ($newest.FullName -like '*SARC2~*')

    # Remembered like the other settings; anything else read back as the game menu
    $f = Join-Path $t 'settings.json'
    $c = Get-KlbfDefaults; $c.start = 'SARC2~0000000000000003'; Save-KlbfSettings $c $f
    Check 'remembered' ((Read-KlbfSettings $f).start -eq 'SARC2~0000000000000003')
    Check 'default: the game menu' ((Get-KlbfDefaults).start -eq 'menu')
    Check 'invalid: the game menu' ((ConvertTo-KlbfChoice ([pscustomobject]@{ start = '..\x' })).start -eq 'menu')

    # The window: Start beside Play lists the menu, new game and every save, and gives the choice back
    $hw = @{ gpu = 'AMD Radeon RX 6900 XT'; vramGB = 16; ramGB = 32; screen = '2560x1440' }
    $ch = Get-KlbfDefaults; $ch.start = 'auto'
    $m = New-KlbfMenuForm $ch $hw '' @{ data = $data; windows = 'Windows 11' }
    Check 'window: the start list' ((@($m.c.startList.Items) -join ',') -eq 'Game menu,New game,Autosave,Slot 1,Slot 2')
    Check 'window: the saved choice selected' ((Get-KlbfFormChoice $m.c).start -eq 'auto')
    $m.c.startList.SelectedIndex = 3
    Check 'window: a slot picked gives its folder' ((Get-KlbfFormChoice $m.c).start -eq 'SARC2~0000000000000003')
    $m.form.Dispose()
} finally {
    Remove-Item -LiteralPath $t -Recurse -Force -ErrorAction SilentlyContinue
}
if ($fails) { "$fails failed"; exit 1 } else { 'all passed' }
