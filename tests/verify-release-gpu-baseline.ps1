param()
$ErrorActionPreference = 'Stop'
# Cargo is run deliberately against three known failing assertions before the second build.
$PSNativeCommandUseErrorActionPreference = $false
$tests = [ordered]@{
    'pipeline::host::tests::depth_bounds_pipeline_loads_and_filters_draws' = 'outside [0.6, 1.0] must be rejected'
    'pool::pages::tests::a_copy_on_pages_is_mapped_and_gives_its_pages_back' = 'rounding over 25 %'
    'pool::pages::migrate::tests::a_moved_copy_keeps_its_texels' = 'rounding over 25 %'
}
function Run-Case([string]$Name, [string]$Failure = '') {
    $log = (& cargo test --locked -p victory-gpu --lib $Name -- --exact --nocapture 2>&1 | Out-String)
    $code = $LASTEXITCODE
    Write-Host $log
    if ($Failure) {
        if ($code -eq 0 -or -not $log.Contains($Failure)) { throw "Did not reproduce the expected baseline failure for $Name (exit $code)" }
    } elseif ($code -ne 0 -or $log -notmatch 'test result: ok\. 1 passed') {
        throw "Expected one passing test for $Name (exit $code)"
    }
}
foreach ($case in $tests.GetEnumerator()) { Run-Case $case.Key $case.Value }

# Compare the driver's actual reserved and placed sizes, keeping the production 25% guard unchanged.
$pages = 'crates/victory-gpu/src/pool/pages.rs'
$migration = 'crates/victory-gpu/src/pool/pages/migrate.rs'
$probe = @'

#[cfg(test)]
mod fixture_allocation_probe {
    use super::*;
    #[test]
    fn measure_bc1_mip_chains() {
        let host = crate::host::host().expect("D3D12 software device required");
        for (width, height) in [(512u64, 512u32), (1024, 512), (1024, 1024), (2048, 1024), (2048, 2048), (4096, 2048)] {
            let mut desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: width, Height: height, DepthOrArraySize: 1, MipLevels: (width.ilog2() + 1) as u16,
                Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_BC1_UNORM,
                SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                ..Default::default()
            };
            let placed = unsafe { host.device.GetResourceAllocationInfo(0, &[desc]) }.SizeInBytes;
            desc.Layout = D3D12_TEXTURE_LAYOUT_64KB_UNDEFINED_SWIZZLE;
            let tiled = unsafe { host.device.GetResourceAllocationInfo(0, &[desc]) }.SizeInBytes;
            println!("BC1 {}x{} mips={} placed={} tiled={} within_budget={}", width, height, desc.MipLevels, placed, tiled, tiled <= placed + placed / 4);
        }
    }
}
'@
$original = [IO.File]::ReadAllText($pages)
$old = "Width: 1024," + [Environment]::NewLine + "            Height: 512,"
if (-not $original.Contains($old)) { $old = $old.Replace([Environment]::NewLine, [string][char]10) }
if (($original.Split([string[]]@($old), [StringSplitOptions]::None)).Length -ne 2) { throw 'Unexpected page-test fixture' }
$original = $original.Replace($old, $old.Replace('1024', '2048').Replace('512', '1024'))
$original = $original.Replace('MipLevels: 11,', 'MipLevels: 12,')
[IO.File]::WriteAllText($pages, $original + $probe)
$original = [IO.File]::ReadAllText($migration)
$from = 'moved_keeps(host, desc(windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_BC1_UNORM, 10));'
$to = @'
moved_keeps(host, D3D12_RESOURCE_DESC {
            Width: 2048, Height: 1024,
            ..desc(windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_BC1_UNORM, 12)
        });
'@
if (-not $original.Contains($from)) { throw 'Unexpected migration-test fixture' }
[IO.File]::WriteAllText($migration, $original.Replace($from, $to))
Run-Case 'pool::pages::fixture_allocation_probe::measure_bc1_mip_chains'
Run-Case 'pool::pages::tests::a_copy_on_pages_is_mapped_and_gives_its_pages_back'
Run-Case 'pool::pages::migrate::tests::a_moved_copy_keeps_its_texels'
