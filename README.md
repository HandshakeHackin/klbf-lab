<p align="center"><img src="docs/klbf/logo.svg" width="120" alt="KLBF Lab"></p>
<h1 align="center">KLBF Lab 63.1 — test build (26 Nov 2026)</h1>
<p align="center">
  <img src="https://img.shields.io/badge/version-Lab%2063.1-1f46c8" alt="version: Lab 63.1">
  <img src="https://img.shields.io/badge/tested%20on-RX%209070%20XT-555" alt="tested on RX 9070 XT">
  <img src="https://img.shields.io/badge/stage-experimental-ffb000" alt="experimental">
  <a href="docs/klbf/MEASUREMENT.md"><img src="https://img.shields.io/badge/fps-shared%20measurement%20method-2ea44f" alt="shared measurement method"></a>
</p>

> **📏 A shared frame-rate standard.** Numbers between projects cannot be compared today (empty seas, menus, one lucky second). We propose one method for everyone: same starting save, same route, three tables (frame rate seen in the world, stability index, average performance index). **[Read the method](docs/klbf/MEASUREMENT.md)**, use it, and post your numbers.

KLBF Lab is a test build of a compatibility layer that runs an Xbox Series title natively on Windows from files you supply. It gathers the best fixes we could measure, aimed at **AMD Radeon (RDNA, 64-lane) cards**. This build is based on Neptune (Traffic & zm0kiemedia) plus contributors' fixes credited below, and our pipeline pre-build that survives AMD driver crashes.

**No game files are included.** You need your own legally obtained copy. New to this? Follow the [beginner's guide](docs/GUIDE.md).

**New in Lab 63.1:**
- **Bench timer on any screen:** during a run the game window's title shows the time ("KLBF Lab · Bench 2:13 / 5:00", or "waiting for the world" before it), and the corner label follows the game window, also on a second monitor. The FPS caption stays off until the results.
- **The bench for other projects:** [ADD-THE-BENCH.md](docs/klbf/ADD-THE-BENCH.md) shows a VIctory-derived project how to add it (files, 4 hooks, variables, credits); projects that add it are listed there.

**New in Lab 63:**
- **Bench BM-v1 built in:** `$env:KLBF_BENCH = 'bm-v1'` before `play.ps1` measures 5:00 from the first world frame with input locked (a small corner label only says it is running, no figures), then shows a certificate and saves it (PNG, JSON, text) in `bench-results` next to the exe, named `KLBF-Bench-BM-v1-YYYYMMDD-HHMM-<serial>`. Min and max are the slowest and fastest 1-second windows; the JSON also records the settings (profile, upscaler, texture cap, pipelines skipped and denied, draws per frame, dropped draws). Procedure and results board: [BM-v1](https://github.com/PastiiJay/klbf-lab/blob/main/docs/klbf/BENCH-BM-V1.md). Bench side of PR #78 by Vladimir-Human (its input record and replay is left out: the replay drifted).
- **Skip movies:** `-SkipMovies open|end|fast` (`VICTORY_SKIP_MOVIES`, by zocram4cc and Yz, PR #77) skips the opening movies.
- **Mesh index fix:** mesh vertex indices masked to 9 bits (from Marvisak/VIctory, by tjthebigbearcat-hub): fixes dark faces and green distant shrubs.
- **Run the pre-build again** (`.\play.ps1 -Prebuild`): the mesh fix changes the shader translation, so the old shader cache no longer matches.

## Speed

**Lab 63 figures: not measured yet.** The tables below are still Lab 61's.

No cherry-picked numbers: these come from ordinary play, not from a quiet scene chosen to look good (scenes with only open water run higher). Measured during a real play session (controller), RX 9070 XT 16 GB, Ryzen 7 9800X3D, 32 GB RAM, Series S profile + FSR 4, the settings of [`play.ps1`](play.ps1):

We propose a shared way to measure, so that every project can compare on equal footing: [docs/klbf/MEASUREMENT.md](docs/klbf/MEASUREMENT.md). The numbers below follow it.

**Frame rate seen in the world** (peaks by situation)

| Situation | fps |
|---|---|
| Standing still, looking at the sky | up to 27 |
| On foot in a quiet area (by a lake or a river) | up to 23 |
| Standing still in traffic, busy neighbourhood | up to 17 |
| On foot on a motorway slip road | up to 16 |
| On foot in a busy neighbourhood | up to 14 |
| Driving over a bridge above water | up to 17 |
| Driving a truck at moderate speed | up to 15 |
| Driving fast through streets (the world streams in) | up to 12 |

<!-- live-start -->
**Stability index**: how the frame rate is spread over a real play session still running, **37 minutes so far without a crash** (385 samples, on foot and driving; updated every 5 minutes of play):

| fps | Share of the time |
|---|---|
| 18 and more | 16 % |
| 15–18 | 28 % |
| 12–15 | 30 % |
| 9–12 | 15 % |
| 5–9 | 5 % |
| under 5 (freezes) | 6 % |

**Average performance index**, same session: average 14.0 fps, median 14.5, 5 % low 1.8, 1 % low 0.4 (the lows are the freezes).
<!-- live-end -->

Occasional short freezes in any situation (on foot or driving); they do not crash the game and it stays playable. We are working on them now.

Route: start at the male lead's house right after first arriving there (~3% progress), from the save made after waking up in the morning (same time of day every run), then wander his neighbourhood, some driving and some walking. One machine only: other cards and drivers may behave differently. Full log: [docs/klbf/results.md](docs/klbf/results.md).

Known issues:
- Occasional freezes when the card or PC memory fills (fewer since textures are capped at 1024 px; try `-LowMemory`).
- Road paint markings and some small props (parking rocks) flicker.
- Bushes look coarse.
- Some shadows flicker near trees; very distant trees sometimes flash black.
- Water surfaces still flicker on AMD; swimming still works.
- Mirrors stay black (they need ray tracing).
- Car windows (glass) are still not drawn at default settings: you can break into a car but the glass is invisible.
- Resolution is Series S, upscaled.
- AMD RDNA 2 cards (RX 6000 series) get safe defaults automatically (`VICTORY_AHEAD_FIRST`, `VICTORY_CARD_FLAGGED`, `VICTORY_PASSES_AHEAD` and `VICTORY_RELAY` off unless you set them), which avoids the device loss after 25-72 s. Thanks to @BlackBu7let for the bisect (#52). For a single-switch retest through `play.ps1`, set the corresponding environment variable before launching. Remove it before the default run (for example, `Remove-Item Env:VICTORY_AHEAD_FIRST -ErrorAction SilentlyContinue`).

## Build and run

Needs Git, [rustup](https://rustup.rs), Visual Studio Build Tools (C++ workload) and AMD's [FidelityFX SDK](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK) 2.4 unzipped to `C:\FidelityFX-SDK`.

```powershell
git clone -b beta-2 https://github.com/PastiiJay/klbf-lab
cd klbf-lab
cargo build --release
powershell -ExecutionPolicy Bypass -File .\play.ps1 -GameDir C:\Games\Title -Prebuild   # once per build, about 10-20 min; resumes by itself after a driver crash
powershell -ExecutionPolicy Bypass -File .\play.ps1 -GameDir C:\Games\Title   # play
```

`-SeriesX` plays in Series X mode (no FSR needed, slower). `-SkipMovies end` skips the opening movies (`open` and `fast` are other ways to do it). Textures are capped at 1024 px by default; `-LowMemory` caps them at 512 px: fewer freezes when memory is full, softer textures. `-FfxDir` points at another FSR folder. Plug the controller in before starting.

### First start (Windows 10 and 11)

`victory.exe` needs Microsoft's D3D12 Agility SDK 1.620 in a `D3D12\` folder next to it (Windows 10's built-in D3D12 refuses our root signatures). The first `play.ps1` run asks once, then downloads the official [Microsoft.Direct3D.D3D12 1.620.2](https://www.nuget.org/packages/Microsoft.Direct3D.D3D12/1.620.2) package from nuget.org, checks that the DLLs are signed by Microsoft and puts them there. `-Yes` skips the question, `-NoAgility` skips the step.

By hand: download that package, open it as a zip and copy `build/native/bin/x64/D3D12Core.dll` and `d3d12SDKLayers.dll` into `target\release\D3D12\`.

### Seconds per frame under Wine

If the world takes seconds a frame under Wine/Proton while the GPU is mostly idle and `wineserver` uses most of a core, the cause is in Wine, not in this branch: wineserver keeps each process's mapped views in a list and walks it on every map and unmap ([Wine bug 60868](https://bugs.winehq.org/show_bug.cgi?id=60868)). The title remaps its memory pages by the thousand each frame, so every call gets slower as views pile up. `perf` on `wineserver` shows almost all of its time in `req_unmap_view` and `is_valid_view_addr`, and the log's texture and copy times grow with the remaps.

A `wineserver` that keeps the views in a tree fixes it: [CachyOS/wine-cachyos#50](https://github.com/CachyOS/wine-cachyos/pull/50) (by ajsem1337, based on neonbyte1's version; also measured in [fxceless/victory-fork#184](https://github.com/fxceless/victory-fork/issues/184)). It changes only `server/`, so it can be built for the Wine of your Proton and dropped into a copy of that Proton as `files/bin/wineserver`; the server protocol version must match the Proton build (regenerate `server_protocol.h` with `tools/make_requests` before building). On an RX 6900 XT with Proton Experimental 11.0-20261112, same build and settings, the first minutes in the world went from 2,068 to 509 ms a frame (textures 482 → 73 ms, copies 417 → 99 ms) and `wineserver` from ~70 % to ~6 % of a core.

Frame captures (`VICTORY_CAPTURE_WORLD`) cost about 235 ms each on the submitting thread (measured under Proton on an RX 6900 XT). For a speed run, `unset VICTORY_CAPTURE_WORLD` before launch: the switch checks only whether the variable is set, so `0` still captures.

## Which settings for my PC?

From what testers have reported so far. To measure and compare on your machine, run the BM-v1 bench (see "New in Lab 63").

| Hardware | Settings | Source |
|---|---|---|
| Any | Series S + FSR (the default) is what we measure with and the smoothest. `-SeriesX` looks sharper but is slower. | our runs |
| 8 GB VRAM (RTX 4060, RTX 2080 reports) | `-LowMemory` (`VICTORY_TEXTURE_MAX_DIM=512`): it removed black textures and the deep freeze. | #63, #65 |
| 12–16 GB VRAM | 1024, our default. | our runs |
| AMD RX 6000 (RDNA 2) | Safe defaults are now applied automatically. | #52, #82 |
| 16 GB RAM | Page file of 32 GB or more on an SSD, plus `-LowMemory`. Expect stutter. Commit can exceed 41 GB on 32 GB machines with the shipped pools. | #13 |
| Windows 10 | Supported with Microsoft's D3D12 Agility SDK 1.620 in `target\release\D3D12\` (`play.ps1` fetches it, see "First start"). | #11, #85 |
| NVIDIA | Confirmed running on RTX 2080 and RTX 4060. | #65 |

## Settings

`play.ps1` sets these (the pre-build and the game must use the same ones: the shader cache depends on them):

- If character mouths distort on Proton, set `VICTORY_HALF_VIEWS=0` before pre-build and play to use the older 16-bit buffer path. This may leave facial animation at rest. Unset it to use the default path; the shader cache distinguishes the settings.
- Our pre-build and skip list: `VICTORY_SKIP_PIPELINES` = [docs/klbf/skip-amd.txt](docs/klbf/skip-amd.txt); pipelines the driver crashed on are skipped from `prebuild-crashed.txt`.
- New in this build: `VICTORY_CLAMP_AFTER_WRITER=1` (zm0kiemedia), `VICTORY_PORT_CONSTANTS=1` and `VICTORY_PORT_DESCRIPTORS=1` (avatarmanz's renderer port), `VICTORY_DYNAMIC_SLOTS=786432` (more descriptor slots: no more draws dropped, fixes flicker), `VICTORY_INDIRECT_RAW=0` and `VICTORY_STAGE_POOL_MB=2048` (WillyboPeep).
- New in Lab 62: `VICTORY_TEXTURE_MAX_DIM=1024` by default (Evan-Lowry's cap; loading screens, menus and short-mip images are kept at full size, `VICTORY_TEXTURE_MAX_DIM_KEEP_UI=0` turns that off), `-LowMemory` sets 512. `VICTORY_LIBRARY_LOCK=0`: Ammar Zubair's AMD pipeline-library lock is on by default on AMD in the code, but the serialized compile was too slow in play, so the launcher turns it off (remove the line to try it). `VICTORY_TEXTURE_LRU=1` (samishating) is available, off by default.
- Series S: `--console series-s` and `VICTORY_FSR_DLL=<FidelityFX SDK>\Kits\FidelityFX\signedbin\amd_fidelityfx_upscaler_dx12.dll`.
- Neptune's set: `VICTORY_CARD_WINDOWS_MB=4096`, `BAKE_TEXTURES_MS=10000`, `EARLY_TEXTURES_MS=40`, `WAVE_OWN_LDS_GROUPS=64,128`, `WIDE_MERGE=1`, `RELAY_OWN_QUEUE=1`, `UNDER_WRITES_WORDS=1`, `UNDER_WRITES_RUNS=100000`, `UNDER_WRITES_MASKS=1`, `UNDER_WRITES_MASKS_LEAST=1`, `WIDE_REFRESH=1`, `ASK_ONCE=1`, `TEXTURE_NO_WAIT=1`, `CARD_REHASH_RUNS=1`, `JOIN_COPIES=1`, `D3D12_BACKGROUND=0`, `LATE_WAIT_LISTS=1024`, `WINDOW_TAILS=1`, `TEXTURE_STAGE_MB=512`, `CARD_BUFFERS_MIN_KB=64`, `ASK_QUIET=8`, `COPY_CHANGED=1`, `QUIET_BACK=1`, `LIST_POOL_MOST=2048`, `LIST_POOL_MB=2048`, `WIDE_THREADS=12`, `TEXTURE_HASH_EVERY=32`, `LEVELS_ON_DEMAND=1`, `WINDOW_KB=1024`, `CARD_BUFFERS_MB=1536`, `CARD_WINDOW_UPLOAD=1`, `DROP_DXIL=1`, `DESC_DATA=1`, `VRAM_IDLE_MS=15000,2500,1000`, `PIPELINE_WAIT_MS=2500`, `OCEAN_READBACK=1`, `WATCH_4K=1`, `THIN=1`, `OVERLAY=0` (all with the `VICTORY_` prefix).

Please report results in an [issue](https://github.com/PastiiJay/klbf-lab/issues): GPU, driver, how long it ran, fps, what looks wrong. Text only, no game screenshots. Patch notes: [CHANGELOG.md](CHANGELOG.md).

Product names are only used to say what this works with; they belong to their owners. KLBF Lab is not affiliated with them. License: GPL-3.0 ([LICENSE](LICENSE)).

## Credits

Every commit keeps its author. Thank you:

- [@Marvisak](https://github.com/Marvisak) for [VIctory](https://github.com/Marvisak/VIctory), the base of everything here, and [@fxceless](https://github.com/fxceless) for [victory-fork](https://github.com/fxceless/victory-fork).
- Traffic and [@zm0kiemedia](https://github.com/zm0kiemedia) for Neptune, the base of this build, and the indirect-argument clamps.
- [@avatarmanz](https://github.com/avatarmanz) ([VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX)): renderer port 1 and 2.
- [@Evan-Lowry](https://github.com/Evan-Lowry): the texture size cap.
- Ammar Zubair: the AMD pipeline-library lock (amdxc64.dll crash).
- samishating (from AKA0GHOST/victory-fork): least-recently-used texture copies (`VICTORY_TEXTURE_LRU`).
- [@WillyboPeep](https://github.com/WillyboPeep) ([neptune-ve](https://github.com/WillyboPeep/neptune-ve)): loading-screen pipelines, section views, profiler and settings.
- [@proneonn](https://github.com/proneonn) (Neon, [Vertex](https://github.com/proneonn/Vertex)) and [@tippythetaidum](https://github.com/tippythetaidum): face fixes.
- [@GamingRipperDoc](https://github.com/GamingRipperDoc) (fxceless/victory-fork#1216, analysis by [@IxMxAMAR](https://github.com/IxMxAMAR)): device-loss fix.
- [@IxMxAMAR](https://github.com/IxMxAMAR) and [@MalfoyDraco99](https://github.com/MalfoyDraco99) (fxceless/victory-fork#1213): AMD compiler crash fix.
- Carried in Neptune's base: [@myokex](https://github.com/myokex) ([victory-amd](https://github.com/myokex/victory-amd)), JoshuaO22, Spexivus, joroyt and tightfall.
- KLBF Lab ([@PastiiJay](https://github.com/PastiiJay)): the crash-surviving pipeline pre-build, the skip list, KLBF Lab names, tests and docs.
