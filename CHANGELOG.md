# Changelog

What each KLBF Lab release adds, in one place, so anyone (or any tool) can see at a glance what a build has
and what it lacks. Every switch below is an environment variable; "off" means it changes nothing unless set.

Base: Op-lite's victory-scarlett (Series X profile, sea fixed; no longer public, our tree keeps its history),
itself from [Marvisak/VIctory](https://github.com/Marvisak/VIctory), plus our AMD work. GPL-3.0.

## v26.11.6 (unreleased)

### Performance
- Op-lite's work of 28 Nov taken out again: in play tests it cost frame rate (driving, sailing), while
  none of its switches was on by default. The earlier Op-lite base stays.

### Launcher
- Clear shader cache no longer freezes the window: the cache is moved aside at once and deleted in the background.
- Quick tab: Copy JSON beside Copy summary, the full result file for the results form (HandshakeHackin, #161).
- First start: the built-in (console) upscaler by default instead of AMD FSR, which can flash black planes from
  the character on water and in low sun. AMD FSR and NVIDIA DLSS stay one click away; a saved choice is kept.
- The start script (Start: autosave or a slot) stops as soon as a button is pressed on the pad: a step waiting
  for a screen that never came kept pressing A in the game.
- launcher.log records each preparation step's process, exit code and last progress when shader translation
  or pipeline preparation stops early (HandshakeHackin, #157).
- "KLBF Lab.exe" in the folder and on the desktop shortcut: the settings window opens with no console window
  flashing first ("KLBF Lab.cmd" stays beside it as the fallback).
- The release notes list both zips with their own SHA-256 and how to check them (docs/klbf/VERIFY.md).
- Start beside Play in Settings: Game menu (as before), New game, Autosave or a slot numbered as the game's menu
  lists it. The save is made the newest (all saves backed up first), then Continue is pressed for you; New game
  is pressed only with no save yet (with saves, pick it in the game's menu). Remembered like the other settings.
- Saves tab: up and down buttons beside the clicked slot move it one place in the game's Load list (the two
  saves trade dates, which the list is sorted by; all saves backed up before the first move, refused while
  the game runs; the autosave stays first).
- Launch options (Experimental tab), as on Steam: `NAME=value` pairs separated by spaces, applied to the game
  over the boxes, remembered, and written at the top of the run log. Only `KLBF_` and `VICTORY_` names and plain
  values are taken; anything else is refused with a message and nothing starts.

### NVIDIA
- NVIDIA DLSS through NVIDIA's official Streamline SDK (v2.14.16, MIT), all off by default; nothing changes
  on AMD or without the choice. Settings, Upscaler: "NVIDIA DLSS (GeForce RTX)" (`-Dlss`, `KLBF_DLSS=1`):
  DLSS Super Resolution does the FSR path's upscale in AMD FSR's place, at the FSR preset's render size.
- Settings, Options: frame generation "NVIDIA DLSS x2" (RTX 40 and 50), "x3", "x4", "x6" (Multi Frame
  Generation, RTX 50; never more than the driver says it makes) beside "AMD FSR x2" (`-DlssFrames 2..6`,
  `KLBF_DLSS_FG`): DLSS Frame Generation on our present, from the upscale's depth and motion vectors, with
  Reflex low latency on. The interface is not given to it apart (it can shimmer on fast motion, as with FSR's).
- Frame rate cap 30 / 40 / 60 in Settings, Options (`-FpsCap`, the existing `KLBF_FPS_CAP`).
- No NVIDIA file is in this repository or in the release zip: the first start with DLSS picked fetches
  NVIDIA's own release from github.com/NVIDIA-RTX/Streamline (276 MB, checked against the SHA-256 GitHub
  publishes for it), keeps only its `bin/x64` DLLs in `streamline\` beside victory.exe, and victory.exe checks
  NVIDIA's signature before loading them. DLSS itself (`nvngx_dlss.dll`, `nvngx_dlssg.dll`) is under the
  NVIDIA RTX SDKs license, fetched from NVIDIA as it ships.
- Defensive, not yet tried on an NVIDIA card: offline, a hash or signature mismatch, a card or driver
  without the feature, a failed device hand-over or a refused upscale: the run goes on with AMD FSR (and
  AMD's frame generation) as before, said in the log.
- Credits (GPL-3.0, code taken and adapted): readmet3xt-lab/foxhound (the Streamline ABI and DLSS
  mode/camera code as they are; library loading, signature check, device hand-over, DLSS evaluation and the
  present markers; the 30/40/60 cap idea), Kemerd/VIctory-NVIDIA-RT and trustno1-coder/VIctory-NVIDIA
  (frame generation 2x to 6x, bounded by what the driver makes).

## v26.11.5 (28 Nov 2026)

### Credits
- fxceless ([victory-fork](https://github.com/fxceless/victory-fork), GPL-3.0): static samplers in one kept order.
- CHECK6960 ([Victory-Nvidia-optimized](https://github.com/CHECK6960/Victory-Nvidia-optimized), GPL-3.0): shorter, non-blocking pipeline wait.
- LibsMQG ([hle-d3d12-amd](https://github.com/LibsMQG/hle-d3d12-amd), GPL-3.0): lazy lists, kept room given back, pool discard switches.

crossducking (RAZDA4A-XEN) and nathanbzrr: thanks for the code this release takes in (GPL-3.0).

### Rendering
- Trees, foliage, fences, wooden posts and signs no longer vanish or turn black: the raw views of buffers
  that do not begin on 16 bytes, made for a list's own views, were set to null (372 to 575 a frame); they are
  now moved to their bytes as the staged ones were. On by default; `KLBF_RAW_VIEW_FIX=0` turns it off.
  From crossducking (RAZDA4A-XEN, Wave 11). Tickets #142, #145.

### Memory (off by default, Experimental tab)
- `KLBF_8GB_SAFE=1`: texture uploads stop at 65 % of the card's budget instead of 90 %, for 8 GB cards
  whose live budget drops once the world is up. From nathanbzrr (RAZDA4A-XEN PR #1).
- `KLBF_EMERGENCY_RECLAIM=1`: with under 2.5 GB of free commit, or the shared GPU memory at 88 % of its
  budget, idle command memory and spare chunks are freed at once, before an allocation can fail with
  0x8007000E. From crossducking (RAZDA4A-XEN).

### Stutter and memory (all off: set the switch to try)
- `KLBF_SAMPLER_ORDER=1`: static samplers get the same place in the driver's table at every start, so the
  driver's shader cache answers instead of compiling hundreds of pipelines again after each start.
- `KLBF_PIPELINE_NOBLOCK=1`: a pipeline off the screen still being built is waited for 80 ms a frame, not
  500, and not again until it is built (`VICTORY_PIPELINE_WAIT_MS` still sets the wait).
- `KLBF_LIST_LAZY=1`: our command lists hold a driver list only while they record.
- `KLBF_SHRINK_KEPT=1`: room only a burst needed is given back.
- `KLBF_POOL_DISCARD_FRESH=1`, `KLBF_POOL_DISCARD_SOFT=1`: two finer rules for pool pages given back.

## v26.11.4 (27 Nov 2026)

Thanks to the testers whose reports led to this release: Domi, INGLIP, deydey103, Solar, pacoravan,
Dominic, MalarkZA, Aphantasiac, KingKaom and wilsonwilsonlg.

### Stability
- The game no longer ends with "write to 0x0 at +0x2215c35" (or "+0x23cf683") a minute or two in, during the
  opening movies: that was the title's own hang watchdog, set off while shaders were still being built. It is
  now off; `KLBF_TITLE_WATCHDOG=1` keeps it. Thanks to KingKaom (#69) and wilsonwilsonlg (#116).
- crashes.log now names a refused graphics memory reservation (the cause behind the stop at +0x19f4f58).
  Thanks to Aphantasiac (#136).

### Download
- Download KLBF-Lab.zip: everything is inside a "KLBF Lab" folder when you extract it.
  KLBF-Lab-win64.zip (the same files, flat) stays for the automatic update.
- Prefer to build it yourself? See [BUILD.md](BUILD.md).
- When a newer version is out, the big Play button becomes Update: one click updates and reopens the launcher.

### Bench
- Bench wherever you like: any save, any place. An optional "Where were you?" note (e.g. city driving) goes
  on the card, in the result file and in the text to share; no fixed scene name any more.
- The card shows a Save ID (a short fingerprint of the save it loaded): same ID, same starting point.
- The card no longer lists the launcher itself among the experimental switches.
- After a bench, Play in the reopened Settings window starts a normal game again: the bench switches now
  reach only the benched game, not the launcher that stays open. From HandshakeHackin (#147).

### Controls
- Keyboard and mouse: a click in the game takes the mouse for the camera (F9 or leaving the window lets it go);
  keys by position, usual PC defaults, works beside a controller. New Controls tab in Settings: on/off,
  sensitivity, invert, keys. Ideas from LooperSalty's AnyXboxX and eikkapine's keyboard and mouse controls.
- Rumble stops when the game loses focus or closes, and never runs more than 1 s without a refresh.
  `KLBF_RUMBLE=0` turns it off.

### Picture
- Faces: no pipeline skipped for good while shaders are still compiling.
- Trees and plants that flicker: `KLBF_TREE_FIX=1` (off by default) gives each list a larger budget.

### Game window
- The game window takes the keyboard focus at the start: F8, F10, F11 and Alt+F4 work without a click
  first (the launcher lets it take the foreground; the log says "game window has the keyboard focus").
- Borderless fullscreen: the black backdrop always stays behind the game (it could cover the picture).
- F8: a short white flash and "Screenshot saved" in the caption, or "Screenshot failed: <reason>" in the
  caption and the log. Never silent.
- VRAM in the caption and on the bench card: used / installed as sold ("VRAM 10.6/16 GB"); DXGI's budget
  only in the JSON.

### Launcher
- Every run's log in `Saved Games\KLBF Lab\logs` (run-<date>.log, the last 10 kept) with crashes.log;
  "Report a bug" points there, the user's profile hidden.
- Beside the version: "Up to date", "Update available: ..." or "Couldn't check for updates".
- The first-run desktop shortcut offer says whether the shortcut was created, and asks again after a failure.
- The README says what to do if it doesn't start (Unblock the zip, or play.ps1 with PowerShell). The launcher
  unblocks its own files.
- A failed start is never silent: any launcher error is shown in a message box in front of everything and
  written to `logs\launcher-error.txt`
  (PowerShell version, execution policy, language mode, files still marked as downloaded).
- The pipelines step runs only when runs wrote down new pipelines: the bench no longer prepares again.
- Updates: the window reopens after an update, and the backup stays inside the build folder.
- The settings window opens even when Windows blocks its helpers (screens, Add-Type).

## v26.11.3 (27 Nov 2026)

- The launcher starts from "KLBF Lab.cmd" instead of "KLBF Lab.vbs": Windows Defender's cloud check flagged the .vbs (a script starting a hidden PowerShell) as a trojan, a false positive. An existing desktop shortcut is repaired automatically.

## v26.11.2 (27 Nov 2026)

- Hotfix: the launcher opened nothing on PCs where the small screen helper is blocked (antivirus, restricted Windows); it now opens with a fallback. Thanks to the testers who sent the error.

## v26.11.1 (27 Nov 2026)

Versions follow YY.MM.N (year, month, release of the month), like AMD's drivers.

### Launcher (KLBF Lab Settings)
- Version at the bottom left; when a newer release is out, "Update available" beside it: one click
  downloads it, checks the zip and every file (SHA-256), swaps the files (put back on any failure) and
  reopens Settings. Saves and settings are never touched.
- Settings window on first start: profile X / S, AMD FSR 4 (RDNA 4) or FSR 3.1 (other cards) or the built-in
  upscaler, presets Quality / Balanced / Performance / Ultra Performance, output size, frame generation (FG x2),
  display (which monitor), window or borderless fullscreen, saved profiles.
- Default for every PC: Profile X, FSR Ultra Performance, 3840x2160 output, no frame generation.
  If FSR cannot start, the built-in upscaler is used.
- "Your PC" panel: GPU generation, VRAM, RAM, screen, with a verdict per line.
- Shaders prepared before the first start, with one progress bar from 0 to 100 %; the game never starts
  without them. Cancel stops everything.
- First start downloads the official Microsoft Agility SDK and AMD FidelityFX DLLs, checked by SHA-256.
- Saves tab (slots, names hidden by default), backups before every launch and bench, copy / restore.
- Data in `Saved Games\KLBF Lab` (saves, shader cache, settings); a one-time copy from older folders.
- Quick tab: last bench card (copy image / copy text), shortcuts, desktop shortcut.
- Paths, user and PC names are never shown unless "Show" is pressed.

### In game
- F8 screenshot (game image only, small KLBF Lab mark with the stats), F10 settings, F11 restart,
  Alt+Enter borderless fullscreen (16:9 kept, black bars on wider screens).
- Caption: `KLBF Lab | 34 FPS (base 17) | 1% | ms | draws | profile, upscaler (render -> output) | VRAM | RAM (resident + shared)`.

### Bench (KLBF Lab Bench BM-v1)
- The card shows every launcher setting, the real FSR version, frames shown with frame generation
  and the real ones ("base"), installed RAM, the save slot number.
- A fade to black after lit frames is left out of the black-frame count (#124, reported by ParaNyx).

### Fixes
- RX 6000 (RDNA 2): the 2 GB aperture segment no longer fills (device loss at world load), #127 by
  MalarkZA, kyokyotosh and HandshakeHackin.
- RX 9070 XT: AMD compiler crash at amdxc64.dll+0x861b9f fixed (diagnosed with BrokeAsh, KingKaom).
- High-performance GPU chosen on laptops with two GPUs (#47).

### Other launchers (victory.exe started directly)
- Our universal defaults apply; F10 / F11 are left to the game; F8 and Alt+Enter stay.

### Experimental switches (off), rewritten from eikkapine/victory-amd's ideas
`KLBF_NIGHT_LIGHTS`, `KLBF_PORT_ALL`, `KLBF_BUFFER_NO_WAIT`, `KLBF_PRIORITY`, `KLBF_FPS_CAP=<fps>`,
`KLBF_TEX_FIFO`, `KLBF_TEX_CAP`.

### Credits
Op-lite, cool_fox, eikkapine, MalarkZA, kyokyotosh, HandshakeHackin, ParaNyx, BrokeAsh, KingKaom,
avatarmanz, Ryuki, and every tester who posted a bench.
