# Changelog

What each KLBF Lab release adds, in one place, so anyone (or any tool) can see at a glance what a build has
and what it lacks. Every switch below is an environment variable; "off" means it changes nothing unless set.

Base: Op-lite's victory-scarlett (Series X profile, sea fixed; no longer public, our tree keeps its history),
itself from [Marvisak/VIctory](https://github.com/Marvisak/VIctory), plus our AMD work. GPL-3.0.

## Next

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
