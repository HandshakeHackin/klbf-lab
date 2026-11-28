# Exercise the actual WinForms preparation window using a stand-in process with deterministic output.
param([switch]$ExpectOriginal)
$ErrorActionPreference = 'Stop'
$dir = Join-Path ([IO.Path]::GetTempPath()) ("klbf-prep-test-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $dir | Out-Null
try {
    $env:KLBF_HOME = $dir
    . (Join-Path (Split-Path -Parent $PSScriptRoot) 'launcher-menu.ps1')
    $source = Join-Path $dir 'Fixture.cs'
    $exe = Join-Path $dir 'Fixture.exe'
    $book = Join-Path $dir 'book.bin'
    $rpf = Join-Path $dir 'shaders.rpf'
    $log = Join-Path $dir 'logs\launcher.log'
    New-Item -ItemType File -Path $book,$rpf | Out-Null
    @'
using System;
using System.Diagnostics;
using System.IO;
class Fixture {
    static int Main(string[] args) {
        string phase = args[0];
        string status = Environment.GetEnvironmentVariable("TEST_FAIL_PHASE");
        File.AppendAllText(Environment.GetEnvironmentVariable("TEST_CHILDREN"),
            phase + ":" + Process.GetCurrentProcess().Id + Environment.NewLine);
        if (phase == "shaders") {
            Console.WriteLine("4 shaders found in fixture");
            Console.WriteLine("2 shaders in the cache");
        } else if (phase == "warm") {
            Console.WriteLine("fixture; 88557 to make, 2 processes");
        } else {
            return 33;
        }
        return phase == status ? -7 : 0;
    }
}
'@ | Set-Content -LiteralPath $source -Encoding ascii
    $compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
    & $compiler /nologo /target:exe "/out:$exe" $source
    if ($LASTEXITCODE -ne 0) { throw "Fixture compile failed: $LASTEXITCODE" }
    foreach ($phase in 'shaders','warm','neither') {
        $env:TEST_FAIL_PHASE = $phase
        $env:TEST_CHILDREN = Join-Path $dir "$phase.txt"
        if (Test-Path $log) { Remove-Item -LiteralPath $log }
        $result = Show-KlbfPrepare $exe $rpf $book
        if ($result -ne ($phase -ne 'shaders')) { throw "Incorrect preparation result for $phase" }
        $rows = @(Get-Content -LiteralPath $env:TEST_CHILDREN)
        $expectedPhases = if ($phase -eq 'shaders') { @('shaders') } else { @('shaders','warm') }
        if ($rows.Count -ne @($expectedPhases).Count) { throw "Unexpected children: $rows" }
        $record = if (Test-Path $log) { Get-Content -LiteralPath $log -Raw } else { '' }
        foreach ($line in $rows) {
            $name, $childId = $line.Split(':')
            $label = if ($name -eq 'shaders') { 'Translating shaders' } else { 'Preparing pipelines' }
            $expected = "$label started (PID $childId)"
            if ($record.Contains($expected) -ne (-not $ExpectOriginal)) { throw "Unexpected start record for $line" }
            if ($name -eq $phase) {
                $count = if ($name -eq 'shaders') { '2 of 4' } else { '0 of 88557' }
                $expected = "$label (PID $childId) exited with 0xFFFFFFF9 ($count)"
                if ($record.Contains($expected) -ne (-not $ExpectOriginal)) { throw "Unexpected exit record for $line" }
            }
        }
        if ($phase -eq 'neither' -and $record -match 'exited with') { throw 'Successful preparation reported a failure' }
        Write-Host "Injected failure=$phase; preparation result=$result; children=$($rows -join ', '); log:"
        Write-Host $record
    }
} finally {
    Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue
}
