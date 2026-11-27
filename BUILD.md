# Build it yourself (if you'd rather not run our prebuilt zip)

## Prerequisites

- Windows 10 or 11, 64-bit.
- Rust, installed with [rustup](https://rustup.rs). The pinned toolchain (`rust-toolchain.toml`) installs
  itself the first time you build.
- Visual Studio 2022 Build Tools with the "Desktop development with C++" workload.
- Git.

## Steps

In a terminal (PowerShell):

```
git clone --branch v26.11.4 https://github.com/PastiiJay/klbf-lab
cd klbf-lab
rustup toolchain install
cargo build --profile diag --features diagnostics --locked -p victory
```

This is the same build command as our release workflow (`.github/workflows/release.yml`). The exe is
`target\diag\victory.exe`.

To get the same zip layout as our release:

```
powershell -ExecutionPolicy Bypass -File scripts\package-release.ps1 -Tag v26.11.4 -Victory target\diag\victory.exe -Out .\dist
```

This writes `dist\KLBF-Lab.zip` (the files inside a "KLBF Lab" folder, the one people download) and
`dist\KLBF-Lab-win64.zip` (the same files, flat, for the automatic update). Extract `KLBF-Lab.zip` anywhere,
open the "KLBF Lab" folder and double-click "KLBF Lab.cmd".

## Notes

- Your build will not be byte-identical to ours (paths, timestamps, toolchain details): compare how it
  behaves, not its hash.
- The launcher still downloads the official components on the first start, each file checked by SHA-256:
  Microsoft's DirectX 12 Agility SDK and AMD FSR (AMD FidelityFX SDK).
- Nothing of the game is included or built: KLBF Lab only runs your own copy.
