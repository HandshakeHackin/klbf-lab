//! (ours) KLBF Lab Bench · Bayside Morning v1 (BM-v1). On with `KLBF_BENCH=bm-v1` only; nothing
//! changes otherwise.
//!
//! The player loads the fixed save and touches nothing: the measurement starts at the first world
//! frame (`crate::bench::world_frame() == 1`) and lasts 5:00. During it only frame times are kept
//! (one push per present). From world frame 1 + 5 s the game gets a neutral controller and no
//! keyboard ([`input_locked`]); presses are only counted. F10 or closing the window stops early
//! (PARTIAL). At the end the results go to `bench-results` next to the exe (`KLBF_BENCH_DIR`
//! overrides): JSON, text, and a PNG of the certificate. The certificate (the postcard template
//! `klbf_card.png`, made by `tools/klbf-card-template.py`, with the figures written over it)
//! then covers the whole client area, opaque, until a key or button is pressed or 60 s pass, and
//! the run ends. During the run the time (no figures) shows in the game window's title and in a
//! small label at the window's top-left corner.
//!
//! Hooks into the rest of the layer, and how to add the module to another VIctory-derived tree:
//! `docs/klbf/ADD-THE-BENCH.md`.
//!
//! The card is shown the way Neptune's overlay shows its panel: a premultiplied [`Bitmap`] in a
//! click-through, topmost layered window (`UpdateLayeredWindow`), GDI as the glyph rasteriser.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::Win32::Graphics::Dxgi::{DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO, IDXGIAdapter3};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows_sys::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::pixels::{Bitmap, short_cpu, short_gpu};

#[cfg(test)]
const RUN: Duration = Duration::from_secs(300);
const GRACE: Duration = Duration::from_secs(5);
const CARD_FOR: Duration = Duration::from_secs(60);
const FREEZE_MS: f64 = 200.0;
const VK_F10: i32 = 0x79;
const VK_F12: i32 = 0x7B;
/// The certificate without its variable texts, 1280 x 820 RGBA for the sketch's 1000 x 640 units.
const TEMPLATE: &[u8] = include_bytes!("klbf_card.png");
/// Build-time project and commit (`build.rs`).
const PROJECT: &str = env!("KLBF_PROJECT");
const PROJECT_SOURCE: &str = env!("KLBF_PROJECT_SOURCE");
const SHA: &str = env!("KLBF_SHA");
const ID: &str = "BM-v1";

/// `KLBF_BENCH_LENGTH` (27/11): `short` 5 minutes (BM-v1, the default and the board's test), `medium` 15,
/// `long` 30, `marathon` 60. Same place, same rules; the longer ones say whether a build holds.
fn length() -> (&'static str, u64) {
    static L: LazyLock<(&'static str, u64)> = LazyLock::new(|| {
        match std::env::var("KLBF_BENCH_LENGTH").map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Ok("medium") => ("Medium", 15),
            Ok("long") => ("Long", 30),
            Ok("marathon") => ("Marathon", 60),
            _ => ("Short", 5),
        }
    });
    *L
}

/// How long the run lasts.
fn run_for() -> Duration {
    Duration::from_secs(length().1 * 60)
}

/// The test's name: `BM-v1` for the 5-minute run, `BM-v1-Marathon` and so on for the longer ones.
fn id() -> String {
    match length() {
        (_, 5) => ID.to_string(),
        (name, _) => format!("{ID}-{name}"),
    }
}

/// The run's target time, `5:00` .. `60:00`.
fn target() -> String {
    mmss(run_for())
}

/// Whether `KLBF_BENCH=bm-v1` is set.
pub fn active() -> bool {
    static ON: LazyLock<bool> =
        LazyLock::new(|| std::env::var("KLBF_BENCH").is_ok_and(|v| v.trim().eq_ignore_ascii_case("bm-v1")));
    *ON
}

/// When the first world frame was presented (the measurement's start).
static START: OnceLock<Instant> = OnceLock::new();

/// Whether the game gets a neutral controller and no keyboard now: from world frame 1 + 5 s on,
/// until the process ends (the card included).
pub fn input_locked() -> bool {
    active() && START.get().is_some_and(|t| t.elapsed() >= GRACE)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Waiting,
    Running,
    Done,
}

struct Run {
    phase: Phase,
    last: Option<Instant>,
    /// Frame times since the start, ms.
    ms: Vec<f64>,
    black: u64,
    /// [`counters`] at the start.
    base: [u64; 4],
}

static RUN_STATE: Mutex<Run> = Mutex::new(Run { phase: Phase::Waiting, last: None, ms: Vec::new(), black: 0, base: [0; 4] });
/// Presses the lock kept from the game.
static BLOCKED: AtomicU64 = AtomicU64::new(0);
static CARD: OnceLock<(Card, Instant)> = OnceLock::new();
static SYS: OnceLock<Sys> = OnceLock::new();
static ADAPTER: Mutex<Option<IDXGIAdapter3>> = Mutex::new(None);
static EXIT: OnceLock<fn()> = OnceLock::new();

fn run() -> std::sync::MutexGuard<'static, Run> {
    RUN_STATE.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------------------------------
// The tree: everything the module reads from the rest of the layer, here and nowhere else (plus
// `crate::pixels`). A port to another VIctory-derived tree changes only these lines; a figure the
// tree does not have may return 0 (docs/klbf/ADD-THE-BENCH.md).

/// Shaders translated, pipelines made, draws run, draws asked for (run or dropped), so far.
/// (Scarlett tree: no running pipeline or asked-draw count, 0.)
fn counters() -> [u64; 4] {
    [crate::shader_cache::counts().1, 0, crate::draw::drawn(), 0]
}

/// "S" (Series S profile) or "X".
fn profile() -> &'static str {
    if crate::console::current() == crate::console::Console::SeriesS { "S" } else { "X" }
}

/// The texture size cap in effect, px (0: none); pipelines skipped; pipelines denied.
/// (Scarlett tree: none of the three, 0.)
fn tree_settings() -> [u64; 3] {
    [0; 3]
}

/// The process's working set, bytes.
fn ram_bytes() -> u64 {
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS { cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
    // SAFETY: our own process and a sized structure.
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    c.WorkingSetSize as u64
}

/// Highest working set and highest card-memory use seen during the measured run (once a second), bytes.
static PEAK: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

/// The card's memory in use and its budget now, bytes.
fn vram_now() -> (u64, u64) {
    let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
    let adapter = ADAPTER.lock().unwrap_or_else(|e| e.into_inner()).clone();
    // SAFETY: a plain query filling our struct.
    match adapter.map(|a| unsafe { a.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info) }) {
        Some(Ok(())) => (info.CurrentUsage, info.Budget),
        _ => (0, 0),
    }
}

/// (27/11) Live figures after the run's time in the window title, once a second: frames per second over the last
/// second, the process's RAM, the whole CPU's use and the card's busy share (input is locked, so they cannot sway
/// the run). Empty on the first call.
fn live_figures() -> String {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetSystemTimes;
    static PREV: Mutex<Option<(Instant, usize, u64, u64, [u64; 2])>> = Mutex::new(None);
    static GPU: AtomicU64 = AtomicU64::new(u64::MAX);
    let ft = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    let (mut idle, mut kernel, mut user) = (FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 }, FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 }, FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 });
    // SAFETY: three FILETIMEs of ours.
    unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
    let (idle, all) = (ft(idle), ft(kernel) + ft(user));
    let now = Instant::now();
    let frames = run().ms.len();
    let busy = [0, 1].map(|i| crate::host::CARD_BUSY_US[i].load(Ordering::Relaxed));
    let mut prev = PREV.lock().unwrap_or_else(|e| e.into_inner());
    let out = match *prev {
        Some((t, f, i0, a0, b0)) => {
            let secs = now.duration_since(t).as_secs_f64().max(0.001);
            let cpu = if all > a0 { 100.0 * (1.0 - idle.saturating_sub(i0) as f64 / (all - a0) as f64) } else { 0.0 };
            if busy[1] > b0[1] {
                GPU.store((100 * busy[0].saturating_sub(b0[0]) / (busy[1] - b0[1])).min(100), Ordering::Relaxed);
            }
            let gpu = GPU.load(Ordering::Relaxed);
            format!(
                " \u{b7} {:.0} FPS \u{b7} RAM {:.1} GB \u{b7} CPU {:.0} %{}",
                frames.saturating_sub(f) as f64 / secs,
                ram_bytes() as f64 / (1024.0 * 1024.0 * 1024.0),
                cpu.clamp(0.0, 100.0),
                if gpu == u64::MAX { String::new() } else { format!(" \u{b7} GPU {gpu} %") }
            )
        }
        None => String::new(),
    };
    let next = if busy[1] > prev.map_or(0, |p| p.4[1]) { busy } else { prev.map_or(busy, |p| p.4) };
    *prev = Some((now, frames, idle, all, next));
    out
}

/// One sample of RAM and card memory for the peaks, while the run is measured.
fn sample_peaks() {
    if run().phase == Phase::Running {
        PEAK[0].fetch_max(ram_bytes(), Ordering::Relaxed);
        PEAK[1].fetch_max(vram_now().0, Ordering::Relaxed);
    }
}

/// The card's busy share at the run's start ([busy, all] microseconds), for the card's "card busy".
static BUSY_BASE: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

fn device_removed() -> bool {
    crate::descriptor::host::device_removed().is_some()
}

/// The fps of the slowest `pct` of the frames (ms, sorted ascending).
fn low_pct_fps(sorted_asc: &[f64], pct: f64) -> f64 {
    if sorted_asc.is_empty() {
        return 0.0;
    }
    let count = ((sorted_asc.len() as f64 * pct).ceil() as usize).clamp(1, sorted_asc.len());
    let tail = &sorted_asc[sorted_asc.len() - count..];
    let avg_ms = tail.iter().sum::<f64>() / count as f64;
    if avg_ms > 0.0 { 1000.0 / avg_ms } else { 0.0 }
}

/// Refuses a results folder inside a source checkout (an ancestor with `.victory-root`, as
/// victory-core's `ensure_outside_repo`, which this crate does not depend on).
fn outside_repo(dir: &std::path::Path) -> Result<(), String> {
    let dir = std::path::absolute(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    match dir.ancestors().find(|a| a.join(".victory-root").is_file()) {
        Some(root) => Err(format!("refusing to write benchmark output {} inside repository {}", dir.display(), root.display())),
        None => Ok(()),
    }
}

fn encode_png(w: u32, h: u32, rgb: &[u8]) -> Vec<u8> {
    fn chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        png.extend((data.len() as u32).to_be_bytes());
        let start = png.len();
        png.extend(kind);
        png.extend(data);
        let mut crc = !0u32;
        for &b in &png[start..] {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
            }
        }
        png.extend((!crc).to_be_bytes());
    }
    let raw = crate::pixels::filter_image(rgb, w as usize * 3, 3);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend(w.to_be_bytes());
    header.extend(h.to_be_bytes());
    header.extend([8, 2, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &header);
    let mut packed = vec![0u8; zlib_rs::compress_bound(raw.len())];
    let (idat, rc) = zlib_rs::compress_slice(&mut packed, &raw, zlib_rs::DeflateConfig::new(6));
    debug_assert_eq!(rc, zlib_rs::ReturnCode::Ok);
    chunk(&mut png, b"IDAT", idat);
    chunk(&mut png, b"IEND", &[]);
    png
}

// ---------------------------------------------------------------------------------------------

/// How the run ends after the card (the title's own end: suspend, save the pipeline cache, exit).
pub fn on_exit(f: fn()) {
    let _ = EXIT.set(f);
}

/// The window is being closed: the results are written (PARTIAL before 5:00); the close goes on.
pub fn window_closed() {
    if active() {
        finish(Some("window closed"));
    }
}

/// Called from `queue.rs` (`present`) for every presented frame.
pub(crate) fn on_present(black: bool, world_frame: u64) {
    if !active() {
        return;
    }
    let now = Instant::now();
    let mut r = run();
    let phase = r.phase;
    match phase {
        Phase::Done => return,
        Phase::Waiting => {
            if world_frame >= 1 {
                let _ = START.set(now);
                for i in 0..2 {
                    BUSY_BASE[i].store(crate::host::CARD_BUSY_US[i].load(Ordering::Relaxed), Ordering::Relaxed);
                }
                r.phase = Phase::Running;
                r.last = Some(now);
                r.base = counters();
                tracing::info!("KLBF Lab Bench {}: measuring {} from the first world frame", id(), target());
            }
            return;
        }
        Phase::Running => {}
    }
    if let Some(last) = r.last {
        r.ms.push(now.duration_since(last).as_secs_f64() * 1000.0);
    }
    r.last = Some(now);
    r.black += u64::from(black);
    drop(r);
    if START.get().is_some_and(|s| now.duration_since(*s) >= run_for()) {
        finish(None);
    }
}

// ---------------------------------------------------------------------------------------------
// Figures

#[derive(Clone, Copy, Default, Debug)]
struct Summary {
    frames: usize,
    avg: f64,
    median: f64,
    low1: f64,
    low5: f64,
    min: f64,
    max: f64,
    pct12: f64,
    freezes: usize,
    longest_ms: f64,
}

/// The frame rate of each run of consecutive frames lasting at least 1 s (frames / their time), in
/// order; the trailing run shorter than 1 s is left out.
fn window_fps(ms: &[f64]) -> Vec<f64> {
    let (mut out, mut sum, mut n) = (Vec::new(), 0.0, 0usize);
    for &m in ms {
        sum += m;
        n += 1;
        if sum >= 1000.0 {
            out.push(n as f64 * 1000.0 / sum);
            (sum, n) = (0.0, 0);
        }
    }
    out
}

/// `ms` in order. Min and max are the lowest and highest 1-second window ([`window_fps`]; the
/// average when the run is shorter); the lows are per frame.
fn summarize(ms: &[f64]) -> Summary {
    if ms.is_empty() {
        return Summary::default();
    }
    let windows = window_fps(ms);
    let mut sorted = ms.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    let total: f64 = sorted.iter().sum();
    let fps = |m: f64| if m > 0.0 { 1000.0 / m } else { 0.0 };
    let median = if n.is_multiple_of(2) { 0.5 * (sorted[n / 2 - 1] + sorted[n / 2]) } else { sorted[n / 2] };
    let fast: f64 = sorted.iter().filter(|&&m| m <= 1000.0 / 12.0).sum();
    Summary {
        frames: n,
        avg: fps(total / n as f64),
        median: fps(median),
        low1: low_pct_fps(&sorted, 0.01),
        low5: low_pct_fps(&sorted, 0.05),
        min: windows.iter().copied().reduce(f64::min).unwrap_or_else(|| fps(total / n as f64)),
        max: windows.iter().copied().reduce(f64::max).unwrap_or_else(|| fps(total / n as f64)),
        pct12: if total > 0.0 { fast * 100.0 / total } else { 0.0 },
        freezes: sorted.iter().filter(|&&m| m > FREEZE_MS).count(),
        longest_ms: sorted[n - 1],
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stamp {
    Certified,
    Partial,
    Invalid,
}

impl Stamp {
    fn word(self) -> &'static str {
        match self {
            Stamp::Certified => "CERTIFIED",
            Stamp::Partial => "PARTIAL",
            Stamp::Invalid => "INVALID",
        }
    }
    fn colour(self) -> [u8; 3] {
        match self {
            Stamp::Certified => RED,
            Stamp::Partial => [199, 119, 0],
            Stamp::Invalid => [90, 61, 102],
        }
    }
}

fn mmss(d: Duration) -> String {
    let s = d.as_secs();
    format!("{}:{:02}", s / 60, s % 60)
}

/// CERTIFIED: 5:00 reached and consistent (device kept, black frames at most 1 %, no stall of
/// 10 s or more, at least 300 frames). PARTIAL: stopped early. INVALID: not consistent.
/// Input cannot change the run: it is kept from the game.
fn verdict(early: bool, elapsed: Duration, s: &Summary, black: u64, removed: bool) -> (Stamp, String) {
    if removed {
        return (Stamp::Invalid, "device removed".into());
    }
    if early {
        return (Stamp::Partial, format!("PARTIAL {} / {} \u{2013} not comparable", mmss(elapsed), target()));
    }
    if s.frames < 300 {
        return (Stamp::Invalid, "too few frames".into());
    }
    if black * 100 > s.frames as u64 {
        return (Stamp::Invalid, "black frames over 1 %".into());
    }
    if s.longest_ms >= 10_000.0 {
        return (Stamp::Invalid, "a stall of 10 s or more".into());
    }
    (Stamp::Certified, format!("{} complete \u{b7} no input", target()))
}

/// The machine, read once when the window is up.
#[derive(Clone, Default)]
struct Sys {
    gpu: String,
    vram_gb: f64,
    cpu: String,
    ram_gb: f64,
    os: String,
}

impl Sys {
    fn line(&self) -> String {
        format!(
            "{} {:.0} GB \u{b7} {} \u{b7} {:.0} GB \u{b7} {}",
            short_gpu(&self.gpu),
            self.vram_gb,
            short_cpu(&self.cpu),
            self.ram_gb,
            self.os
        )
    }
}

/// The settings the run was measured with, from the counters the runtime keeps.
#[derive(Clone, Default, Debug)]
struct Settings {
    /// "S" or "X".
    profile: &'static str,
    /// "FSR" (`VICTORY_FSR_DLL` set) or "none".
    upscaler: &'static str,
    /// `VICTORY_TEXTURE_MAX_DIM` in effect, px; 0: no cap.
    texture_max_dim: u64,
    /// Pipelines on the skip list (`VICTORY_SKIP_PIPELINES`) plus pre-build driver crashes.
    skipped: u64,
    /// Host pipelines refused by the driver or whose shaders did not translate.
    denied: u64,
    draws_per_frame: f64,
    /// Draws and mesh dispatches asked for during the run and not run on the host.
    dropped_pct: f64,
}

impl Settings {
    /// `drawn` and `asked`: draws run and asked for during the run's `frames`.
    fn read(frames: usize, drawn: u64, asked: u64) -> Settings {
        let [texture_max_dim, skipped, denied] = tree_settings();
        Settings {
            profile: profile(),
            upscaler: if std::env::var_os("VICTORY_FSR_DLL").is_some_and(|v| !v.is_empty()) { "FSR" } else { "none" },
            texture_max_dim,
            skipped,
            denied,
            draws_per_frame: if frames > 0 { drawn as f64 / frames as f64 } else { 0.0 },
            dropped_pct: if asked > 0 { asked.saturating_sub(drawn) as f64 * 100.0 / asked as f64 } else { 0.0 },
        }
    }

    /// The card's short block.
    fn lines(&self) -> [String; 4] {
        let textures = if self.texture_max_dim > 0 { format!("textures \u{2264} {} px", self.texture_max_dim) } else { "textures uncapped".into() };
        [
            format!("profile {} \u{b7} {}", self.profile, self.upscaler),
            textures,
            format!("{:.0} draws/frame \u{b7} {:.1} % dropped", self.draws_per_frame, self.dropped_pct),
            format!("pipelines: {} skipped \u{b7} {} denied", self.skipped, self.denied),
        ]
    }

    fn line(&self) -> String {
        let textures = if self.texture_max_dim > 0 { format!("textures \u{2264} {} px", self.texture_max_dim) } else { "textures uncapped".into() };
        format!(
            "profile {} \u{b7} {} \u{b7} {textures} \u{b7} {:.0} draws/frame \u{b7} {:.1} % dropped \u{b7} {} skipped \u{b7} {} denied",
            self.profile, self.upscaler, self.draws_per_frame, self.dropped_pct, self.skipped, self.denied
        )
    }
}

struct Card {
    stamp: Stamp,
    reason: String,
    serial: String,
    /// "26 Nov 2026", "26.11.26", "2026-11-26T21:04:05".
    date: String,
    date_short: String,
    date_iso: String,
    elapsed: Duration,
    s: Summary,
    sys: Sys,
    set: Settings,
    from: String,
    player: String,
    blocked: u64,
    /// "RAM peak 20.8 GB · VRAM 10.0 / 15.6 GB · card busy 57 %".
    used: String,
    share: String,
    /// "KLBF-Bench-BM-v1-YYYYMMDD-HHMM-<serial>": the PNG, JSON and text files' name.
    file_stem: String,
    png_name: String,
}

fn player() -> String {
    let p: String = std::env::var("KLBF_BENCH_PLAYER").unwrap_or_default().chars().filter(|c| !c.is_control()).take(32).collect();
    let p = p.trim();
    if p.is_empty() { "anonymous".into() } else { p.into() }
}

/// Freezes the measurement once, writes the results and puts the card up.
fn finish(early: Option<&'static str>) {
    let mut r = run();
    if r.phase == Phase::Done {
        return;
    }
    r.phase = Phase::Done;
    let ms = std::mem::take(&mut r.ms);
    let (black, base) = (r.black, r.base);
    drop(r);
    let elapsed = START.get().map_or(Duration::ZERO, Instant::elapsed);
    let early = early.filter(|_| elapsed < run_for());
    let removed = device_removed();
    let s = summarize(&ms);
    let (stamp, reason) = verdict(early.is_some(), elapsed.min(run_for()), &s, black, removed);
    let now = counters();
    let set = Settings::read(s.frames, now[2].saturating_sub(base[2]), now[3].saturating_sub(base[3]));
    let t = local_time();
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let month = MONTHS[usize::from(t.wMonth.clamp(1, 12)) - 1];
    let declared = if PROJECT_SOURCE == "declared" { " (declared)" } else { "" };
    let mut card = Card {
        stamp,
        reason,
        serial: String::new(),
        date: format!("{} {month} {}", t.wDay, t.wYear),
        date_short: format!("{:02}.{:02}.{:02}", t.wDay, t.wMonth, t.wYear % 100),
        date_iso: format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond),
        elapsed: elapsed.min(run_for()),
        s,
        sys: SYS.get().cloned().unwrap_or_default(),
        set,
        from: format!("{PROJECT} @ {SHA}{declared}"),
        player: player(),
        blocked: BLOCKED.load(Ordering::Relaxed),
        used: String::new(),
        share: String::new(),
        file_stem: String::new(),
        png_name: String::new(),
    };
    // Memory at the end, and the peaks sampled once a second during the measured run.
    let vram = vram_now();
    let peak_ram = PEAK[0].load(Ordering::Relaxed).max(ram_bytes());
    let peak_vram = PEAK[1].load(Ordering::Relaxed).max(vram.0);
    let extra = Extra {
        black,
        removed,
        compiled: [now[0].saturating_sub(base[0]), now[1].saturating_sub(base[1])],
        vram,
        ram: ram_bytes(),
        peaks: (peak_ram, peak_vram),
        why: early.unwrap_or("the full time reached"),
    };
    let busy = [0, 1].map(|i| crate::host::CARD_BUSY_US[i].load(Ordering::Relaxed).saturating_sub(BUSY_BASE[i].load(Ordering::Relaxed)));
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    card.used = format!(
        "peak RAM {:.1} GB \u{b7} peak VRAM {:.1} / {:.1} GB{}",
        peak_ram as f64 / GB,
        peak_vram as f64 / GB,
        vram.1 as f64 / GB,
        if busy[1] > 0 { format!(" \u{b7} card busy {:.0} %", 100.0 * busy[0] as f64 / busy[1] as f64) } else { String::new() }
    );
    let mut doc = result_json(&card, &extra, &window_fps(&ms));
    card.serial = serial(&doc);
    card.share = format!(
        "[KLBF Lab Bench {}] {PROJECT}@{SHA} | {} | {} | avg {:.1} fps, 1% low {:.1} | {} | {}",
        id(),
        short_gpu(&card.sys.gpu),
        short_cpu(&card.sys.cpu),
        s.avg,
        s.low1,
        stamp.word(),
        card.serial
    );
    // Local date and time first, so the results sort and never overwrite each other.
    card.file_stem = format!("KLBF-Bench-{}-{:04}{:02}{:02}-{:02}{:02}-{}", id(), t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, card.serial);
    card.png_name = format!("bench-results\\{}.png", card.file_stem);
    if let J::O(m) = &mut doc {
        m.insert("serial", J::S(card.serial.clone()));
        m.insert("share", J::S(card.share.clone()));
    }
    write_results(&card, &extra, &doc.canonical());
    let _ = CARD.set((card, Instant::now()));
}

struct Extra {
    black: u64,
    removed: bool,
    compiled: [u64; 2],
    vram: (u64, u64),
    ram: u64,
    /// Highest working set and card memory during the measured run.
    peaks: (u64, u64),
    why: &'static str,
}

fn local_time() -> windows_sys::Win32::Foundation::SYSTEMTIME {
    // SAFETY: a plain struct filled by the call.
    unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    }
}

fn results_dir() -> Option<PathBuf> {
    std::env::var_os("KLBF_BENCH_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok()?.parent().map(|p| p.join("bench-results")))
}

const MB: f64 = 1024.0 * 1024.0;

/// A JSON value, written in canonical form ([`J::canonical`]).
enum J {
    Null,
    B(bool),
    I(u64),
    /// Written rounded to 2 decimals.
    F(f64),
    S(String),
    A(Vec<J>),
    O(BTreeMap<&'static str, J>),
}

impl J {
    fn obj<const N: usize>(fields: [(&'static str, J); N]) -> J {
        J::O(fields.into_iter().collect())
    }

    /// The text Python's `json.dumps(v, sort_keys=True, separators=(',', ':'), ensure_ascii=False)`
    /// gives for this value read back with `json.loads`: keys sorted, no spaces, non-ASCII as is,
    /// the same escapes, floats in shortest round-trip form (`{:?}` and Python's `repr` agree on
    /// 2-decimal values from 0.01 up to 1e16).
    fn canonical(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            J::Null => out.push_str("null"),
            J::B(b) => out.push_str(if *b { "true" } else { "false" }),
            J::I(n) => out.push_str(&n.to_string()),
            J::F(x) => {
                let r = if x.is_finite() { (x * 100.0).round() / 100.0 } else { 0.0 };
                // `+ 0.0` turns -0.0 into 0.0.
                out.push_str(&format!("{:?}", r + 0.0));
            }
            J::S(s) => {
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        '\u{8}' => out.push_str("\\b"),
                        '\u{c}' => out.push_str("\\f"),
                        c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                        c => out.push(c),
                    }
                }
                out.push('"');
            }
            J::A(v) => {
                out.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    x.write(out);
                }
                out.push(']');
            }
            J::O(m) => {
                out.push('{');
                for (i, (k, x)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    J::S((*k).to_string()).write(out);
                    out.push(':');
                    x.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// The serial (schema 1): the first 8 hex characters of SHA-256 over the UTF-8 bytes of the result
/// JSON in canonical form ([`J::canonical`]) without its "serial" and "share" fields (the share
/// line carries the serial). The check, in Python:
/// `d.pop('serial'); d.pop('share'); hashlib.sha256(json.dumps(d, sort_keys=True,
/// separators=(',', ':'), ensure_ascii=False).encode('utf-8')).hexdigest()[:8]`.
fn serial(doc: &J) -> String {
    sha256(doc.canonical().as_bytes()).iter().take(4).map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 (FIPS 180-4): the checker has Python's standard library only, so not blake3.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
        0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
        0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
        0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
        0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&(data.len() as u64 * 8).to_be_bytes());
    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for (k, wi) in K.iter().zip(w) {
            let [a, b, c, d, e, f, g, hh] = v;
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let t1 = hh.wrapping_add(s1).wrapping_add((e & f) ^ (!e & g)).wrapping_add(*k).wrapping_add(wi);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let t2 = s0.wrapping_add((a & b) ^ (a & c) ^ (b & c));
            v = [t1.wrapping_add(t2), a, b, c, d.wrapping_add(t1), e, f, g];
        }
        for (x, y) in h.iter_mut().zip(v) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 32];
    for (o, x) in out.chunks_exact_mut(4).zip(h) {
        o.copy_from_slice(&x.to_be_bytes());
    }
    out
}

/// The result without "serial" and "share" (schema 1).
fn result_json(c: &Card, x: &Extra, per_second: &[f64]) -> J {
    let s = &c.s;
    let t = &c.set;
    J::obj([
        ("bench", J::S("KLBF Lab Bench".into())),
        ("test", J::S("Bayside Morning v1".into())),
        ("test_id", J::S(id())),
        ("schema", J::I(1)),
        ("stamp", J::S(c.stamp.word().into())),
        ("reason", J::S(c.reason.clone())),
        ("stop", J::S(x.why.into())),
        ("date", J::S(c.date_iso.clone())),
        ("project", J::S(PROJECT.into())),
        ("project_source", J::S(PROJECT_SOURCE.into())),
        ("sha", J::S(SHA.into())),
        ("player", J::S(c.player.clone())),
        ("gpu", J::S(c.sys.gpu.clone())),
        ("gpu_vram_gb", J::F(c.sys.vram_gb)),
        ("cpu", J::S(c.sys.cpu.clone())),
        ("ram_gb", J::F(c.sys.ram_gb)),
        ("os", J::S(c.sys.os.clone())),
        ("duration_s", J::F(c.elapsed.as_secs_f64())),
        ("target_s", J::I(run_for().as_secs())),
        ("frames", J::I(s.frames as u64)),
        (
            "fps",
            J::obj([
                ("avg", J::F(s.avg)),
                ("median", J::F(s.median)),
                ("low_1pct", J::F(s.low1)),
                ("low_5pct", J::F(s.low5)),
                ("min", J::F(s.min)),
                ("max", J::F(s.max)),
            ]),
        ),
        // The frame rate of each 1-second window, in order: what min and max are taken from.
        ("fps_per_second", J::A(per_second.iter().map(|&f| J::F(f)).collect())),
        ("pct_time_ge_12fps", J::F(s.pct12)),
        ("freezes_over_200ms", J::I(s.freezes as u64)),
        ("longest_frame_ms", J::F(s.longest_ms)),
        ("black_frames", J::I(x.black)),
        ("device_removed", J::B(x.removed)),
        ("inputs_blocked", J::I(c.blocked)),
        ("shaders_translated", J::I(x.compiled[0])),
        ("pipelines_compiled", J::I(x.compiled[1])),
        ("vram_end_mb", J::F(x.vram.0 as f64 / MB)),
        ("ram_peak_mb", J::F(x.peaks.0 as f64 / MB)),
        ("vram_peak_mb", J::F(x.peaks.1 as f64 / MB)),
        ("vram_budget_mb", J::F(x.vram.1 as f64 / MB)),
        ("ram_end_mb", J::F(x.ram as f64 / MB)),
        (
            "settings",
            J::obj([
                ("profile", J::S(t.profile.into())),
                ("upscaler", J::S(t.upscaler.into())),
                // A size in px, not MB: the runtime caps textures by their largest side (null: no cap).
                ("texture_max_dim", if t.texture_max_dim > 0 { J::I(t.texture_max_dim) } else { J::Null }),
                ("pipelines_skipped", J::I(t.skipped)),
                ("pipelines_denied", J::I(t.denied)),
                ("draws_per_frame", J::F(t.draws_per_frame)),
                ("dropped_draws_pct", J::F(t.dropped_pct)),
            ]),
        ),
    ])
}

fn write_results(c: &Card, x: &Extra, json: &str) {
    let s = &c.s;
    let text = [
        "KLBF Lab Bench \u{b7} Bayside Morning v1 (BM-v1)".to_string(),
        format!("Result:     {} \u{2013} {}", c.stamp.word(), c.reason),
        format!("Serial:     {}", c.serial),
        format!("Date:       {}", c.date_iso),
        format!("From:       {} \u{b7} player: {}", c.from, c.player),
        format!("Machine:    {}", c.sys.line()),
        format!("Settings:   {}", c.set.line()),
        format!("Duration:   {} / {} ({})", mmss(c.elapsed), target(), x.why),
        format!(
            "FPS:        avg {:.1} \u{b7} median {:.1} \u{b7} 1% low {:.1} \u{b7} 5% low {:.1} \u{b7} min {:.1} \u{b7} max {:.1} (1 s windows)",
            s.avg, s.median, s.low1, s.low5, s.min, s.max
        ),
        format!("Time >= 12 fps: {:.1} %", s.pct12),
        format!("Freezes (>200 ms): {} (longest frame {:.0} ms)", s.freezes, s.longest_ms),
        format!("Black frames: {} \u{b7} device removed: {}", x.black, if x.removed { "yes" } else { "no" }),
        format!("Inputs blocked: {}", c.blocked),
        format!("Compiled during the run: {} shaders, {} pipelines", x.compiled[0], x.compiled[1]),
        format!("At the end: VRAM {:.0} / {:.0} MB \u{b7} RAM {:.0} MB", x.vram.0 as f64 / MB, x.vram.1 as f64 / MB, x.ram as f64 / MB),
        format!("Peaks during the run: RAM {:.0} MB \u{b7} VRAM {:.0} MB", x.peaks.0 as f64 / MB, x.peaks.1 as f64 / MB),
        String::new(),
        "Share:".into(),
        c.share.clone(),
        String::new(),
    ]
    .join("\r\n");
    let Some(dir) = results_dir() else {
        tracing::warn!("KLBF Lab Bench: no folder for the results");
        return;
    };
    if let Err(e) = outside_repo(&dir) {
        tracing::warn!(error = %e, "KLBF Lab Bench: results not written");
        return;
    }
    let stem = dir.join(&c.file_stem);
    let png = render_png(c);
    let written = std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(stem.with_extension("json"), json))
        .and_then(|()| std::fs::write(stem.with_extension("txt"), text))
        .and_then(|()| std::fs::write(stem.with_extension("png"), png));
    match written {
        Ok(()) => {
            let path = stem.with_extension("png");
            tracing::info!(path = %path.display(), share = %c.share, "KLBF Lab Bench: results saved");
            println!("KLBF Lab Bench {} saved: {}\n{}", id(), path.display(), c.share);
        }
        Err(e) => tracing::warn!(error = %e, dir = %dir.display(), "KLBF Lab Bench: results not written"),
    }
}

/// The card alone at the template's size, as an RGB PNG.
fn render_png(c: &Card) -> Vec<u8> {
    let bm = render_card(c, 1280, 820, false);
    let rgb: Vec<u8> = bm.px.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    encode_png(bm.w as u32, bm.h as u32, &rgb)
}

// ---------------------------------------------------------------------------------------------
// PNG decoding (8-bit RGB or RGBA, not interlaced), for the template

/// Straight RGBA pixels of `png`.
fn decode_png(png: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if png.get(..8)? != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let (mut at, mut w, mut h, mut bpp, mut idat) = (8, 0usize, 0usize, 0usize, Vec::new());
    while at + 8 <= png.len() {
        let len = u32::from_be_bytes(png[at..at + 4].try_into().ok()?) as usize;
        let kind = &png[at + 4..at + 8];
        let data = png.get(at + 8..at + 8 + len)?;
        match kind {
            b"IHDR" => {
                w = u32::from_be_bytes(data[0..4].try_into().ok()?) as usize;
                h = u32::from_be_bytes(data[4..8].try_into().ok()?) as usize;
                bpp = match (data[8], data[9], data[12]) {
                    (8, 2, 0) => 3,
                    (8, 6, 0) => 4,
                    _ => return None,
                };
            }
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        at += 12 + len;
    }
    let stride = w * bpp;
    let mut raw = vec![0u8; h * (stride + 1)];
    let (out, rc) = zlib_rs::decompress_slice(&mut raw, &idat, zlib_rs::InflateConfig::default());
    if rc != zlib_rs::ReturnCode::Ok || out.len() != h * (stride + 1) || bpp == 0 {
        return None;
    }
    let mut px = vec![0u8; h * stride];
    for y in 0..h {
        let kind = raw[y * (stride + 1)];
        let src = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        let (done, row) = px.split_at_mut(y * stride);
        let prev = if y > 0 { &done[(y - 1) * stride..] } else { &[][..] };
        let row = &mut row[..stride];
        for i in 0..stride {
            let a = if i >= bpp { row[i - bpp] } else { 0 };
            let b = prev.get(i).copied().unwrap_or(0);
            let c = if i >= bpp { prev.get(i - bpp).copied().unwrap_or(0) } else { 0 };
            let pred = match kind {
                1 => a,
                2 => b,
                3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                4 => crate::pixels::paeth(a, b, c),
                _ => 0,
            };
            row[i] = src[i].wrapping_add(pred);
        }
    }
    if bpp == 3 {
        px = px.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    }
    Some((w, h, px))
}

// ---------------------------------------------------------------------------------------------
// The thread and its window (idle until the card)

/// Starts our thread for the game window `hwnd` (the first call only; with `KLBF_BENCH=bm-v1`).
pub(crate) fn start(hwnd: windows::Win32::Foundation::HWND, adapter: Option<IDXGIAdapter3>) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    if !active() {
        return;
    }
    let game = hwnd.0 as usize;
    ONCE.call_once(|| {
        let spawned = std::thread::Builder::new().name("klbf-bench".into()).spawn(move || thread(game as HWND, adapter));
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "KLBF Lab Bench thread not started");
        }
    });
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn reg(path: &str, name: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    let (key, value) = (wide(path), wide(name));
    let mut buf = [0u16; 256];
    let mut bytes = std::mem::size_of_val(&buf) as u32;
    // SAFETY: a buffer of the size passed.
    let rc = unsafe {
        RegGetValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut bytes)
    };
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    (rc == 0).then(|| String::from_utf16_lossy(&buf[..len]).trim().to_string())
}

fn read_sys(adapter: Option<&IDXGIAdapter3>) -> Sys {
    // SAFETY: a plain query.
    let desc = adapter.and_then(|a| unsafe { a.GetDesc().ok() });
    let gpu = desc
        .as_ref()
        .map(|d| String::from_utf16_lossy(&d.Description).trim_end_matches('\0').trim().to_string())
        .unwrap_or_else(|| "GPU".into());
    let vram_gb = desc.map_or(0.0, |d| d.DedicatedVideoMemory as f64 / (1024.0 * MB));
    // SAFETY: a plain struct filled by the call.
    let ram_gb = unsafe {
        let mut ms = std::mem::zeroed::<MEMORYSTATUSEX>();
        ms.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        GlobalMemoryStatusEx(&mut ms);
        ms.ullTotalPhys as f64 / (1024.0 * MB)
    };
    let nt = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let build: u32 = reg(nt, "CurrentBuild").and_then(|b| b.parse().ok()).unwrap_or(0);
    let mut product = reg(nt, "ProductName").unwrap_or_else(|| "Windows".into());
    // ProductName still says Windows 10 on 11.
    if build >= 22000 {
        product = product.replace("Windows 10", "Windows 11");
    }
    let os = format!("{} {}", product.replace("Windows", "Win"), reg(nt, "DisplayVersion").unwrap_or_default()).trim().to_string();
    Sys {
        gpu,
        vram_gb,
        cpu: reg(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0", "ProcessorNameString").unwrap_or_else(|| "CPU".into()),
        ram_gb,
        os,
    }
}

fn ours_in_front() -> bool {
    let mut pid = 0u32;
    // SAFETY: plain queries.
    unsafe {
        let front = GetForegroundWindow();
        GetWindowThreadProcessId(front, &mut pid);
        !front.is_null() && pid == GetCurrentProcessId()
    }
}

/// XInput slots found empty, looked at again after a second (asking an empty slot is slow).
struct Pads([Option<Instant>; 4]);

impl Pads {
    fn any(&mut self) -> bool {
        let now = Instant::now();
        (0..4).any(|slot| {
            if self.0[slot].is_some_and(|t| now < t) {
                return false;
            }
            let mut st = XINPUT_STATE::default();
            // SAFETY: room for the state.
            if unsafe { XInputGetState(slot as u32, &mut st) } != 0 {
                self.0[slot] = Some(now + Duration::from_secs(1));
                return false;
            }
            let g = st.Gamepad;
            g.wButtons != 0
                || g.bLeftTrigger > 30
                || g.bRightTrigger > 30
                || [g.sThumbLX, g.sThumbLY, g.sThumbRX, g.sThumbRY].iter().any(|v| v.unsigned_abs() > 8000)
        })
    }
}

/// Any key held (while a window of ours is in front) or pad input; F10 and F12 left out when
/// `skip_ours`.
fn any_input(pads: &mut Pads, skip_ours: bool) -> bool {
    let keys = ours_in_front()
        && (0x08..=0xFE)
            .filter(|&vk| !(skip_ours && (vk == VK_F10 || vk == VK_F12)))
            // SAFETY: plain queries.
            .any(|vk| unsafe { GetAsyncKeyState(vk) } < 0);
    keys || pads.any()
}

fn exit_now() {
    static ONCE: AtomicBool = AtomicBool::new(false);
    if ONCE.swap(true, Ordering::SeqCst) {
        return;
    }
    tracing::info!("KLBF Lab Bench: card closed, ending the run");
    match EXIT.get() {
        Some(f) => f(),
        None => std::process::exit(0),
    }
}

/// Sets the layered window's picture and position in one go (as Neptune's overlay does).
///
/// # Safety
/// `window` is a live layered window of this thread.
unsafe fn present(window: HWND, bm: &Bitmap, at: POINT) {
    // SAFETY: a 32-bit top-down DIB the size of the bitmap, filled with its premultiplied pixels
    // as BGRA, handed to UpdateLayeredWindow, then freed.
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        let dc = CreateCompatibleDC(screen);
        let info = dib_info(bm.w as i32, bm.h as i32);
        let mut bits = std::ptr::null_mut();
        let dib = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if !dib.is_null() && !bits.is_null() {
            let dst = std::slice::from_raw_parts_mut(bits.cast::<u8>(), bm.px.len());
            for (d, s) in dst.chunks_exact_mut(4).zip(bm.px.chunks_exact(4)) {
                d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
            }
            let old = SelectObject(dc, dib);
            let size = SIZE { cx: bm.w as i32, cy: bm.h as i32 };
            let from = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            UpdateLayeredWindow(window, screen, &at, &size, dc, &from, 0, &blend, ULW_ALPHA);
            SelectObject(dc, old);
            DeleteObject(dib);
        }
        DeleteDC(dc);
        ReleaseDC(std::ptr::null_mut(), screen);
    }
}

fn dib_info(w: i32, h: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [RGBQUAD { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbReserved: 0 }],
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
    // SAFETY: the standard handlers; the picture is set by `present`, not by painting.
    unsafe {
        match msg {
            WM_NCHITTEST => HTTRANSPARENT as isize,
            WM_MOUSEACTIVATE => MA_NOACTIVATE as isize,
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// The corner label during the run: text only, no figures, so it cannot sway the run.
fn label_text(started: Option<Duration>) -> String {
    match started {
        None => "KLBF Lab Bench \u{b7} waiting for the world".into(),
        Some(d) => format!("KLBF Lab Bench \u{b7} in progress {} / {}", mmss(d.min(run_for())), target()),
    }
}

/// The game window's title during the run (the label may sit on another screen or behind a window;
/// the title shows in the taskbar and on the window's own frame).
fn title_text(started: Option<Duration>) -> String {
    match started {
        None => "KLBF Lab \u{b7} Bench waiting for the world".into(),
        Some(d) => format!("KLBF Lab \u{b7} Bench {} / {}", mmss(d.min(run_for())), target()),
    }
}

/// Whether the bench owns the game window's title (from the start until the results): a caption
/// with figures (FPS) stays off meanwhile.
pub fn holds_title() -> bool {
    active() && run().phase != Phase::Done
}

/// Sets `game`'s title without waiting on its thread for more than 200 ms (a title thread may not
/// pump for a while). `buf` keeps the text alive until the next call.
///
/// # Safety
/// `game` is a window handle (it may be gone).
unsafe fn set_title(game: HWND, buf: &mut Vec<u16>, s: &str) {
    *buf = wide(s);
    let mut out = 0usize;
    // SAFETY: a NUL-terminated text kept in `buf`.
    unsafe { SendMessageTimeoutW(game, WM_SETTEXT, 0, buf.as_ptr() as isize, SMTO_ABORTIFHUNG, 200, &mut out) };
}

/// `s`, small, light on a semi-transparent dark pill.
fn label(s: &str) -> Bitmap {
    let m = text(UI, 13.0, 0.0, s);
    let (w, h) = (m.w + 16, m.h + 8);
    let mut bm = Bitmap::new(w, h);
    bm.fill_round((0, 0, w, h), 6.0, [16, 12, 20], 110);
    paint(&mut bm, &m, 8, 4, 190, [230, 225, 235]);
    bm
}

fn thread(game: HWND, adapter: Option<IDXGIAdapter3>) {
    let _ = SYS.set(read_sys(adapter.as_ref()));
    *ADAPTER.lock().unwrap_or_else(|e| e.into_inner()) = adapter;
    let class = wide("KlbfLabBench");
    const KEYS: usize = 1;
    // SAFETY: window class and window made and used on this thread only.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
        let window = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP,
            0,
            0,
            10,
            10,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if window.is_null() {
            tracing::warn!("KLBF Lab Bench window not created");
            return;
        }
        tracing::info!("KLBF Lab Bench BM-v1 on (F10 stops the run)");
        SetTimer(window, KEYS, 40, None);
        let mut pads = Pads([None; 4]);
        let (mut f10, mut pressed, mut armed) = (false, false, false);
        let mut shown_secs = u64::MAX;
        let (born, mut label_tick) = (Instant::now(), u64::MAX);
        // The game's own title, read without a message (its thread may be busy), put back after.
        let mut normal = [0u16; 256];
        let n = InternalGetWindowText(game, normal.as_mut_ptr(), normal.len() as i32).max(0) as usize;
        let normal = String::from_utf16_lossy(&normal[..n]);
        let (mut title_buf, mut title_back) = (Vec::new(), false);
        // The card at the client size, drawn once per size.
        let mut cache: Option<Bitmap> = None;
        let mut copied = false;
        let mut msg = std::mem::zeroed::<MSG>();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if !(msg.message == WM_TIMER && msg.hwnd == window) {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
                continue;
            }
            if let Some((card, at)) = CARD.get() {
                if !title_back {
                    title_back = true;
                    set_title(game, &mut title_buf, &normal);
                    copied = copy_to_clipboard(window, &card.share);
                    if !copied {
                        tracing::warn!("KLBF Lab Bench: share line not copied to the clipboard");
                    }
                }
                // Released first, then any press (or 60 s) closes.
                let now = any_input(&mut pads, false);
                if !armed {
                    armed = !now;
                } else if now {
                    exit_now();
                }
                if at.elapsed() >= CARD_FOR {
                    exit_now();
                }
                // Redrawn once a second (the countdown) or when the window's size changes.
                let left = CARD_FOR.saturating_sub(at.elapsed()).as_secs();
                let mut client = std::mem::zeroed::<RECT>();
                let mut origin = POINT { x: 0, y: 0 };
                let there = IsWindow(game) != 0
                    && IsIconic(game) == 0
                    && GetClientRect(game, &mut client) != 0
                    && ClientToScreen(game, &mut origin) != 0;
                if !there {
                    ShowWindow(window, SW_HIDE);
                    continue;
                }
                let (cw, ch) = (client.right.max(1) as usize, client.bottom.max(1) as usize);
                let resized = cache.as_ref().is_none_or(|b| (b.w, b.h) != (cw, ch));
                if resized {
                    cache = Some(render_card(card, cw, ch, true));
                } else if left == shown_secs {
                    continue;
                }
                shown_secs = left;
                let Some(base) = cache.as_ref() else { continue };
                let mut bm = Bitmap { w: base.w, h: base.h, px: base.px.clone() };
                footer(&mut bm, &format!("Any key or button closes \u{b7} {left} s"), copied);
                present(window, &bm, origin);
                SetWindowPos(window, HWND_TOPMOST, origin.x, origin.y, bm.w as i32, bm.h as i32, SWP_NOACTIVATE | SWP_SHOWWINDOW);
                continue;
            }
            let d10 = GetAsyncKeyState(VK_F10) < 0;
            if d10 && !f10 && ours_in_front() {
                finish(Some("F10"));
            }
            f10 = d10;
            if input_locked() {
                let now = any_input(&mut pads, true);
                if now && !pressed {
                    BLOCKED.fetch_add(1, Ordering::Relaxed);
                }
                pressed = now;
            }
            // A game that stops presenting still ends at the run's time.
            if START.get().is_some_and(|s| s.elapsed() >= run_for()) {
                finish(None);
            }
            // The title and the corner label, once a second at most.
            let tick = born.elapsed().as_secs();
            if tick != label_tick {
                label_tick = tick;
                sample_peaks();
                let started = START.get().map(Instant::elapsed);
                // Set every second (a caption thread of the host may have put its own back).
                if holds_title() && IsWindow(game) != 0 {
                    // "· compiling N shaders" after it while pipelines are being made (none at 0).
                    let live = if started.is_some() { live_figures() } else { String::new() };
                    let text = crate::shell::compiling(title_text(started) + &live, " \u{b7} ", crate::pipeline::making());
                    set_title(game, &mut title_buf, &text);
                }
                // Placed from the game window's own rectangle, read every time (it may sit on
                // any screen, and move): its top-left, below the frame when the client starts
                // inside it.
                let mut rect = std::mem::zeroed::<RECT>();
                let mut origin = POINT { x: 0, y: 0 };
                if IsWindow(game) != 0 && IsIconic(game) == 0 && ours_in_front() && GetWindowRect(game, &mut rect) != 0 {
                    let inside = ClientToScreen(game, &mut origin) != 0
                        && (rect.left..rect.right).contains(&origin.x)
                        && (rect.top..rect.bottom).contains(&origin.y);
                    let corner = if inside { origin } else { POINT { x: rect.left, y: rect.top } };
                    let bm = label(&label_text(started));
                    let at = POINT { x: corner.x + 12, y: corner.y + 12 };
                    present(window, &bm, at);
                    SetWindowPos(window, HWND_TOPMOST, at.x, at.y, bm.w as i32, bm.h as i32, SWP_NOACTIVATE | SWP_SHOWWINDOW);
                } else {
                    ShowWindow(window, SW_HIDE);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing the variable parts: coverage masks, GDI text

/// A coverage mask (0..=255), `w` x `h`; `ascent`/`adv` for text.
struct Mask {
    w: usize,
    h: usize,
    v: Vec<u8>,
    ascent: f32,
    adv: f32,
}

impl Mask {
    fn new(w: usize, h: usize) -> Mask {
        Mask { w, h, v: vec![0; w * h], ascent: 0.0, adv: 0.0 }
    }

    fn max(&mut self, x: i32, y: i32, c: u8) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            let p = &mut self.v[y as usize * self.w + x as usize];
            *p = (*p).max(c);
        }
    }

    fn get(&self, x: i32, y: i32) -> f32 {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h { f32::from(self.v[y as usize * self.w + x as usize]) } else { 0.0 }
    }

    fn sample(&self, x: f32, y: f32) -> f32 {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (xi, yi) = (x0 as i32, y0 as i32);
        let top = self.get(xi, yi) * (1.0 - fx) + self.get(xi + 1, yi) * fx;
        let bottom = self.get(xi, yi + 1) * (1.0 - fx) + self.get(xi + 1, yi + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }

    fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        for y in y0.round() as i32..y1.round() as i32 {
            for x in x0.round() as i32..x1.round() as i32 {
                self.max(x, y, 255);
            }
        }
    }

    fn blit(&mut self, m: &Mask, x: i32, y: i32) {
        for j in 0..m.h {
            for i in 0..m.w {
                self.max(x + i as i32, y + j as i32, m.v[j * m.w + i]);
            }
        }
    }

    /// `m` turned by `a` radians (clockwise on screen) about its centre, centred at (cx, cy).
    fn blit_rotated(&mut self, m: &Mask, cx: f32, cy: f32, a: f32) {
        let r = ((m.w * m.w + m.h * m.h) as f32).sqrt() / 2.0 + 1.0;
        let (sin, cos) = a.sin_cos();
        let (hw, hh) = (m.w as f32 / 2.0, m.h as f32 / 2.0);
        for y in (cy - r).floor() as i32..=(cy + r).ceil() as i32 {
            for x in (cx - r).floor() as i32..=(cx + r).ceil() as i32 {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                let (sx, sy) = (dx * cos + dy * sin + hw, -dx * sin + dy * cos + hh);
                let c = m.sample(sx - 0.5, sy - 0.5);
                if c > 0.5 {
                    self.max(x, y, c.min(255.0) as u8);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Font(&'static str, i32);
const IMPACT: Font = Font("Impact", 400);
const ARIAL: Font = Font("Arial", 400);
const ARIAL_BOLD: Font = Font("Arial", 700);
const ARIAL_BLACK: Font = Font("Arial Black", 900);
const GEORGIA: Font = Font("Georgia", 400);
const UI: Font = Font("Segoe UI", 400);

/// GDI as a glyph rasteriser only (as Neptune's overlay): `s` drawn white on black into a scratch
/// DIB; its grey level is the coverage. `extra`: pixels added after each character.
fn text(font: Font, px: f32, extra: f32, s: &str) -> Mask {
    let t: Vec<u16> = s.encode_utf16().collect();
    let face = wide(font.0);
    let mut out = Mask::new(0, 0);
    if t.is_empty() {
        return out;
    }
    // SAFETY: a memory DC, a font and a DIB section made, used and freed here; the DIB is read
    // after GdiFlush within its w x h x 4 bytes.
    unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            return out;
        }
        let f = CreateFontW(-(px.round() as i32).max(1), 0, 0, 0, font.1, 0, 0, 0, 1, 0, 0, ANTIALIASED_QUALITY as u32, 0, face.as_ptr());
        if !f.is_null() {
            let old_font = SelectObject(dc, f);
            SetTextCharacterExtra(dc, extra.round() as i32);
            let mut size = SIZE { cx: 0, cy: 0 };
            GetTextExtentPoint32W(dc, t.as_ptr(), t.len() as i32, &mut size);
            let mut tm = std::mem::zeroed::<TEXTMETRICW>();
            GetTextMetricsW(dc, &mut tm);
            let (w, h) = (size.cx + (px * 0.2) as i32 + 2, size.cy.max(1));
            let info = dib_info(w, h);
            let mut bits = std::ptr::null_mut();
            let dib = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            if !dib.is_null() && !bits.is_null() && w > 0 {
                let old = SelectObject(dc, dib);
                SetBkMode(dc, TRANSPARENT as i32);
                SetTextColor(dc, 0x00ff_ffff);
                TextOutW(dc, 1, 0, t.as_ptr(), t.len() as i32);
                GdiFlush();
                let grey = std::slice::from_raw_parts(bits.cast::<u8>(), (w * h) as usize * 4);
                out = Mask::new(w as usize, h as usize);
                for (o, p) in out.v.iter_mut().zip(grey.chunks_exact(4)) {
                    *o = p[1];
                }
                out.ascent = tm.tmAscent as f32;
                out.adv = size.cx as f32;
                SelectObject(dc, old);
                DeleteObject(dib);
            }
            SelectObject(dc, old_font);
            DeleteObject(f);
        }
        DeleteDC(dc);
    }
    out
}

fn put(bm: &mut Bitmap, x: i32, y: i32, rgb: [u8; 3], a: u32) {
    if x >= 0 && y >= 0 {
        bm.over(x as usize, y as usize, rgb, a.min(255));
    }
}

/// `m` laid on `bm` with its corner at (x, y), in `rgb` at opacity `a`.
fn paint(bm: &mut Bitmap, m: &Mask, x: i32, y: i32, a: u32, rgb: [u8; 3]) {
    for j in 0..m.h {
        for i in 0..m.w {
            let c = u32::from(m.v[j * m.w + i]);
            if c > 0 {
                put(bm, x + i as i32, y + j as i32, rgb, (c * a + 127) / 255);
            }
        }
    }
}

fn hex(c: u32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

const INK: u32 = 0x3b2240;
const SOFT: u32 = 0x6b4a5a;
const RED: [u8; 3] = [0xd6, 0x26, 0x4f];
const BACKDROP: [u8; 3] = [0x1d, 0x14, 0x24];

/// Card units (the sketch's 1000 x 640) to pixels.
struct Pen<'a> {
    bm: &'a mut Bitmap,
    ox: f32,
    oy: f32,
    k: f32,
    /// The pixel box (x0, y0, x1, y1, exclusive) of every text drawn, for the layout test.
    boxes: Vec<[i32; 4]>,
}

impl Pen<'_> {
    fn x(&self, u: f32) -> f32 {
        self.ox + u * self.k
    }
    fn y(&self, v: f32) -> f32 {
        self.oy + v * self.k
    }

    /// Text with its baseline at (x, base), left-aligned or centred, shrunk to fit `max_w`.
    /// Returns the width drawn, in units.
    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, font: Font, size: f32, s: &str, x: f32, base: f32, centre: bool, max_w: f32, rgb: [u8; 3]) -> f32 {
        // Shrunk until its inked width fits (font sizes round, so one scaling may not do).
        let (mut px, mut m) = (size * self.k, text(font, size * self.k, 0.0, s));
        for _ in 0..8 {
            let w = ink_width(&m) / self.k;
            if w <= max_w || w <= 0.0 {
                break;
            }
            px *= max_w / w * 0.98;
            m = text(font, px, 0.0, s);
        }
        let w = m.adv / self.k;
        let left = if centre { x - w / 2.0 } else { x };
        let (x0, y0) = (self.x(left).round() as i32 - 1, (self.y(base) - m.ascent).round() as i32);
        paint(self.bm, &m, x0, y0, 255, rgb);
        if let Some([a, b, c, d]) = ink_box(&m) {
            self.boxes.push([x0 + a, y0 + b, x0 + c, y0 + d]);
        }
        w
    }

    fn width(&self, font: Font, size: f32, s: &str) -> f32 {
        text(font, size * self.k, 0.0, s).adv / self.k
    }
}

/// The box (x0, y0, x1, y1, exclusive) of a mask's inked pixels.
fn ink_box(m: &Mask) -> Option<[i32; 4]> {
    let mut b: Option<[i32; 4]> = None;
    for y in 0..m.h {
        for x in 0..m.w {
            if m.v[y * m.w + x] > 0 {
                let (x, y) = (x as i32, y as i32);
                let r = b.get_or_insert([x, y, x + 1, y + 1]);
                *r = [r[0].min(x), r[1].min(y), r[2].max(x + 1), r[3].max(y + 1)];
            }
        }
    }
    b
}

/// From the pen position to the last inked pixel, in px (at least the advance).
fn ink_width(m: &Mask) -> f32 {
    ink_box(m).map_or(m.adv, |b| m.adv.max(b[2] as f32 - 1.0))
}

/// `s` split at `sep` into lines no wider than `max_w` units at `size`; a part too wide alone is
/// split word by word (after a space or a `/`).
fn wrap(pen: &Pen, font: Font, size: f32, s: &str, sep: &str, max_w: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for part in s.split(sep) {
        if let Some(last) = lines.last_mut() {
            let joined = format!("{last}{sep}{part}");
            if pen.width(font, size, &joined) <= max_w {
                *last = joined;
                continue;
            }
        }
        if pen.width(font, size, part) <= max_w {
            lines.push(part.to_string());
            continue;
        }
        let mut fresh = true;
        for word in part.split_inclusive([' ', '/']) {
            match lines.last_mut() {
                Some(last) if !fresh && pen.width(font, size, format!("{last}{word}").trim_end()) <= max_w => last.push_str(word),
                _ => lines.push(word.to_string()),
            }
            fresh = false;
        }
        for line in lines.iter_mut() {
            line.truncate(line.trim_end().len());
        }
    }
    lines
}

/// `s` wrapped into at most `most` lines: at `size`, else at up to 20 % smaller, else the last
/// line keeps the rest (and is shrunk when drawn). Returns the lines and the size.
#[allow(clippy::too_many_arguments)]
fn fit(pen: &Pen, font: Font, size: f32, s: &str, sep: &str, max_w: f32, most: usize) -> (Vec<String>, f32) {
    let mut lines = Vec::new();
    for step in 0..5 {
        let at = size * (1.0 - 0.05 * step as f32);
        lines = wrap(pen, font, at, s, sep, max_w);
        if lines.len() <= most {
            return (lines, at);
        }
    }
    let rest = lines.split_off(most - 1).join(" ");
    lines.push(rest);
    (lines, size * 0.8)
}

fn card_scale(w: usize, h: usize) -> f32 {
    (w as f32 / 1060.0).min(h as f32 / 700.0)
}

/// The footer line under the card, and "Share line copied" under it when `copied`.
fn footer(bm: &mut Bitmap, s: &str, copied: bool) {
    let k = card_scale(bm.w, bm.h);
    let m = text(UI, 14.0 * k, 0.0, s);
    let lines = if copied { 2.0 } else { 1.0 };
    let y = (bm.h as f32 / 2.0 + 330.0 * k).min(bm.h as f32 - lines * m.h as f32 - 2.0);
    paint(bm, &m, ((bm.w as f32 - m.adv) / 2.0) as i32, y as i32, 255, [200, 186, 210]);
    if copied {
        let c = text(UI, 14.0 * k, 0.0, "Share line copied \u{2014} paste it with your card");
        paint(bm, &c, ((bm.w as f32 - c.adv) / 2.0) as i32, (y + m.h as f32) as i32, 255, [200, 186, 210]);
    }
}

/// Puts `s` on the Windows clipboard as CF_UNICODETEXT; false when any step fails.
unsafe fn copy_to_clipboard(owner: HWND, s: &str) -> bool {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
    use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    // In Win32_System_Ole; not worth that feature for one constant.
    const CF_UNICODETEXT: u32 = 13;
    let wide: Vec<u16> = s.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mem = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2);
        if mem.is_null() {
            return false;
        }
        let p = GlobalLock(mem).cast::<u16>();
        if p.is_null() {
            GlobalFree(mem);
            return false;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
        GlobalUnlock(mem);
        if OpenClipboard(owner) == 0 {
            GlobalFree(mem);
            return false;
        }
        // The clipboard owns the memory once SetClipboardData succeeds.
        let ok = EmptyClipboard() != 0 && !SetClipboardData(CF_UNICODETEXT, mem).is_null();
        CloseClipboard();
        if !ok {
            GlobalFree(mem);
        }
        ok
    }
}

static TEMPLATE_PX: LazyLock<Option<(usize, usize, Vec<u8>)>> = LazyLock::new(|| decode_png(TEMPLATE));

/// The certificate, opaque, `w` x `h` px: the template (scaled to fit, centred on a dark backdrop
/// when `framed`, else filling the bitmap) with the variable texts written over it.
fn render_card(c: &Card, w: usize, h: usize, framed: bool) -> Bitmap {
    render_card_boxes(c, w, h, framed).0
}

/// [`render_card`], with the pixel box of every text drawn.
fn render_card_boxes(c: &Card, w: usize, h: usize, framed: bool) -> (Bitmap, Vec<[i32; 4]>) {
    let mut bm = Bitmap::new(w, h);
    for p in bm.px.chunks_exact_mut(4) {
        p.copy_from_slice(&[BACKDROP[0], BACKDROP[1], BACKDROP[2], 255]);
    }
    let (k, ox, oy) = if framed {
        let k = card_scale(w, h);
        (k, (w as f32 - 1000.0 * k) / 2.0, (h as f32 - 640.0 * k) / 2.0 - 10.0 * k)
    } else {
        ((w as f32 / 1000.0).min(h as f32 / 640.0), 0.0, 0.0)
    };
    // The template, bilinear.
    if let Some((tw, th, px)) = TEMPLATE_PX.as_ref() {
        let (tw, th) = (*tw, *th);
        let (dw, dh) = ((1000.0 * k).round() as i32, (640.0 * k).round() as i32);
        let (sx, sy) = (tw as f32 / dw as f32, th as f32 / dh as f32);
        let texel = |x: i32, y: i32| -> [f32; 4] {
            let (x, y) = (x.clamp(0, tw as i32 - 1) as usize, y.clamp(0, th as i32 - 1) as usize);
            let p = &px[(y * tw + x) * 4..][..4];
            let a = f32::from(p[3]) / 255.0;
            [f32::from(p[0]) * a, f32::from(p[1]) * a, f32::from(p[2]) * a, f32::from(p[3])]
        };
        for j in 0..dh {
            for i in 0..dw {
                let (fx, fy) = ((i as f32 + 0.5) * sx - 0.5, (j as f32 + 0.5) * sy - 0.5);
                let (x0, y0) = (fx.floor(), fy.floor());
                let (ax, ay) = (fx - x0, fy - y0);
                let (xi, yi) = (x0 as i32, y0 as i32);
                let (p00, p10, p01, p11) = (texel(xi, yi), texel(xi + 1, yi), texel(xi, yi + 1), texel(xi + 1, yi + 1));
                let v: [f32; 4] = std::array::from_fn(|n| (p00[n] * (1.0 - ax) + p10[n] * ax) * (1.0 - ay) + (p01[n] * (1.0 - ax) + p11[n] * ax) * ay);
                if v[3] < 0.5 {
                    continue;
                }
                let rgb = [0, 1, 2].map(|n| (v[n] * 255.0 / v[3]).clamp(0.0, 255.0) as u8);
                put(&mut bm, ox.round() as i32 + i, oy.round() as i32 + j, rgb, v[3] as u32);
            }
        }
        // (27/11) Longer runs: the template's "5 minutes at ..." line (between the dotted lines at 290
        // and 347) is covered with the background just above it; the run's own length is written below.
        if length().1 != 5 {
            let ys = (294.0 * k * sy) as i32;
            for i in (90.0 * k) as i32..(420.0 * k) as i32 {
                let p = texel(((i as f32 + 0.5) * sx) as i32, ys);
                let rgb = [0, 1, 2].map(|n| (p[n] * 255.0 / p[3].max(1.0)).clamp(0.0, 255.0) as u8);
                for j in (298.0 * k) as i32..(343.0 * k) as i32 {
                    put(&mut bm, ox.round() as i32 + i, oy.round() as i32 + j, rgb, 255);
                }
            }
        }
    }
    let mut pen = Pen { bm: &mut bm, ox, oy, k, boxes: Vec::new() };
    let ink = hex(INK);
    if length().1 != 5 {
        pen.text(GEORGIA, 18.0, &format!("{} minutes at the male lead's house", length().1), 255.0, 336.0, true, 370.0, ink);
    }
    // Left: the date and how it ended.
    pen.text(GEORGIA, 18.0, &format!("{} \u{b7} {}", c.date, c.reason), 255.0, 394.0, true, 370.0, ink);
    // Postmark: serial and date at the bottom, inside the ring.
    pen.text(ARIAL, 11.0, &format!("{} \u{b7} {}", c.serial, c.date_short), 880.0, 176.0, true, 96.0, RED);
    // The band across the postmark's centre, -14 degrees, see-through.
    let kk = pen.k;
    let (bw, bh) = (196.0 * kk, 38.0 * kk);
    let mut word = text(ARIAL_BLACK, 22.0 * kk, 3.0 * kk, c.stamp.word());
    if word.adv > bw - 16.0 * kk {
        word = text(ARIAL_BLACK, 22.0 * kk * (bw - 16.0 * kk) / word.adv, 0.0, c.stamp.word());
    }
    let mut fill = Mask::new(bw as usize, bh as usize);
    fill.rect(0.0, 0.0, bw, bh);
    let mut edge = Mask::new(bw as usize, bh as usize);
    let t = 3.0 * kk;
    edge.rect(0.0, 0.0, bw, t);
    edge.rect(0.0, bh - t, bw, bh);
    edge.rect(0.0, 0.0, t, bh);
    edge.rect(bw - t, 0.0, bw, bh);
    let mut letters = Mask::new(bw as usize, bh as usize);
    letters.blit(&word, ((bw - word.adv) / 2.0) as i32 - 1, ((bh - word.h as f32) / 2.0) as i32);
    let side = (230.0 * kk) as usize;
    let (bx, by) = (pen.x(880.0) - side as f32 / 2.0, pen.y(120.0) - side as f32 / 2.0);
    for (m, rgb, alpha) in [(&fill, hex(0xfff6ee), 115), (&edge, c.stamp.colour(), 255), (&letters, c.stamp.colour(), 230)] {
        let mut turned = Mask::new(side, side);
        turned.blit_rotated(m, side as f32 / 2.0, side as f32 / 2.0, -14f32.to_radians());
        paint(pen.bm, &turned, bx as i32, by as i32, alpha, rgb);
    }
    // From: project and player.
    // From: project (up to 2 lines) and player, between the postmark and the big average.
    let (from, size) = fit(&pen, ARIAL_BOLD, 18.0, &c.from, " ", 360.0, 2);
    let top = if from.len() > 1 { 259.0 } else { 275.0 };
    for (i, line) in from.iter().enumerate() {
        pen.text(ARIAL_BOLD, size, line, 600.0, top + 17.0 * i as f32, false, 360.0, ink);
    }
    // The player stays above the rule under it (at 312).
    pen.text(ARIAL, 15.0, &format!("player: {}", c.player), 600.0, 300.0, false, 360.0, ink);
    // The settings: a small block in the left panel, right of the stamp.
    for (i, line) in c.set.lines().iter().enumerate() {
        pen.text(ARIAL, 13.0, line, 180.0, 490.0 + 18.0 * i as f32, false, 290.0, ink);
    }
    pen.text(ARIAL, 13.0, &c.used, 180.0, 490.0 + 18.0 * 4.0, false, 290.0, ink);
    // The average, big, and the four tiles.
    pen.text(IMPACT, 64.0, &format!("{:.1}", c.s.avg), 535.0, 380.0, false, 135.0, ink);
    let tiles = [format!("{:.1}", c.s.median), format!("{:.1}", c.s.low1), format!("{:.1}", c.s.max), format!("{:.0} %", c.s.pct12)];
    for (i, value) in tiles.iter().enumerate() {
        pen.text(IMPACT, 28.0, value, 535.0 + 108.0 * i as f32 + 50.0, 434.0, true, 92.0, ink);
    }
    // Freezes and duration; the machine in bold (wrapped, then shrunk, to fit); the share line.
    let blocked = if c.blocked > 0 { format!(" \u{b7} {} inputs blocked", c.blocked) } else { String::new() };
    pen.text(ARIAL, 14.0, &format!("freezes {} \u{b7} {} in the world{blocked}", c.s.freezes, mmss(c.elapsed)), 535.0, 484.0, false, 425.0, ink);
    let mut y = 484.0;
    let (machine, size) = fit(&pen, ARIAL_BOLD, 14.0, &c.sys.line(), " \u{b7} ", 425.0, 3);
    for line in &machine {
        y += 19.0;
        pen.text(ARIAL_BOLD, size, line, 535.0, y, false, 425.0, ink);
    }
    y += 14.0;
    let mut x = 535.0;
    while x < 960.0 {
        let (x0, y0, x1, y1) = (pen.x(x), pen.y(y - 0.5), pen.x((x + 3.0).min(960.0)), pen.y(y + 0.5).max(pen.y(y - 0.5) + 1.0));
        for py in y0.round() as i32..y1.round() as i32 {
            for px in x0.round() as i32..x1.round() as i32 {
                put(pen.bm, px, py, hex(0x7a5a6a), 255);
            }
        }
        x += 7.0;
    }
    y += 6.0;
    // The share line in what is left above the "Saved" line (3 lines when the machine took 2).
    let room = (((604.0 - y) / 15.0) as usize).clamp(1, 3);
    let (share, size) = fit(&pen, ARIAL, 12.0, &c.share, " | ", 425.0, room);
    for line in &share {
        y += 15.0;
        pen.text(ARIAL, size, line, 535.0, y, false, 425.0, hex(SOFT));
    }
    // Where the PNG went.
    pen.text(ARIAL, 11.0, &format!("Saved: {}", c.png_name), 500.0, 621.0, true, 940.0, hex(SOFT));
    let boxes = std::mem::take(&mut pen.boxes);
    (bm, boxes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_stay_out_of_the_source_tree() {
        assert!(outside_repo(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("bench-results")).is_err());
        assert!(outside_repo(&std::env::temp_dir().join("bench-results")).is_ok());
    }

    #[test]
    fn title_during_the_run() {
        assert_eq!(title_text(None), "KLBF Lab \u{b7} Bench waiting for the world");
        assert_eq!(title_text(Some(Duration::from_secs(133))), "KLBF Lab \u{b7} Bench 2:13 / 5:00");
        assert_eq!(title_text(Some(Duration::from_secs(400))), "KLBF Lab \u{b7} Bench 5:00 / 5:00");
    }

    #[test]
    fn figures_and_verdict() {
        // 290 s at 60 fps with one 250 ms freeze in the middle.
        let mut ms = vec![1000.0 / 60.0; 17_400];
        ms.insert(8_700, 250.0);
        let s = summarize(&ms);
        assert_eq!(s.frames, 17_401);
        assert_eq!(s.freezes, 1);
        assert!((s.median - 60.0).abs() < 0.01);
        // Min and max: the slowest and fastest 1-second windows, not single frames.
        assert!(s.min > 40.0 && s.min < 50.0, "min {}", s.min);
        assert!((s.max - 60.0).abs() < 0.01, "max {}", s.max);
        // A run shorter than a window: both are the average.
        let short = summarize(&[10.0, 30.0]);
        assert!((short.max - 50.0).abs() < 0.01 && (short.min - 50.0).abs() < 0.01);
        // A 2 s stall is a window of its own at 0.5 fps.
        assert_eq!(window_fps(&[2000.0, 500.0]), vec![0.5]);
        assert!(s.pct12 > 99.0 && s.pct12 < 100.0);
        assert!(s.low1 < s.low5 && s.low5 <= s.avg);
        let full = Duration::from_secs(300);
        assert_eq!(verdict(false, full, &s, 0, false).0, Stamp::Certified);
        assert_eq!(verdict(true, Duration::from_secs(83), &s, 0, false).1, "PARTIAL 1:23 / 5:00 \u{2013} not comparable");
        assert_eq!(verdict(false, full, &s, 1000, false).0, Stamp::Invalid);
        assert_eq!(verdict(false, full, &s, 0, true).0, Stamp::Invalid);
        assert_eq!(summarize(&[]).frames, 0);
    }

    /// The canonical form and serial match Python's (`json.dumps(d, sort_keys=True,
    /// separators=(',', ':'), ensure_ascii=False)`, `hashlib.sha256`), worked out there.
    #[test]
    fn canonical_json_and_serial() {
        assert_eq!(
            sha256(b"abc").iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let doc = J::obj([
            ("b", J::S("\u{e9}\"\\\n\u{1}\u{8}".into())),
            ("a", J::A(vec![J::I(1), J::F(2.5), J::F(30.0), J::Null, J::B(true), J::F(0.1), J::F(1_234_567.891)])),
            ("c", J::obj([("z", J::F(-0.0)), ("y", J::F(16_000.0))])),
        ]);
        assert_eq!(doc.canonical(), r#"{"a":[1,2.5,30.0,null,true,0.1,1234567.89],"b":"é\"\\\n\u0001\b","c":{"y":16000.0,"z":0.0}}"#);
        assert_eq!(serial(&doc), "6797e740");
    }

    /// The certified sample run of the fixture below.
    fn sample_card() -> Card {
        let s = Summary {
            frames: 18_000,
            avg: 60.0,
            median: 60.0,
            low1: 58.5,
            low5: 59.2,
            min: 57.3,
            max: 61.2,
            pct12: 100.0,
            freezes: 0,
            longest_ms: 17.45,
        };
        Card {
            stamp: Stamp::Certified,
            reason: "5:00 complete \u{b7} no input".into(),
            serial: String::new(),
            date: String::new(),
            date_short: String::new(),
            date_iso: "2026-11-26T21:04:05".into(),
            elapsed: RUN,
            s,
            sys: Sys {
                gpu: "AMD Radeon RX 9070 XT".into(),
                vram_gb: 15.92,
                cpu: "AMD Ryzen 7 7800X3D 8-Core Processor".into(),
                ram_gb: 31.9,
                os: "Windows 11 24H2".into(),
            },
            set: Settings {
                profile: "S",
                upscaler: "FSR",
                texture_max_dim: 1024,
                skipped: 12,
                denied: 0,
                draws_per_frame: 4321.5,
                dropped_pct: 0.25,
            },
            from: String::new(),
            player: "anonymous".into(),
            blocked: 0,
            share: String::new(),
            file_stem: String::new(),
            png_name: String::new(),
        }
    }

    /// Very long hardware, project and player names stay inside their panel: every text's inked
    /// pixels lie in the left (15..495) or right (505..985) panel, 15..625 high; only the
    /// centred "Saved" line at the bottom crosses the divider.
    #[test]
    fn long_names_stay_inside_their_panel() {
        let mut card = sample_card();
        card.sys = Sys {
            gpu: "NVIDIA GeForce RTX 5080 Laptop GPU".into(),
            vram_gb: 16.0,
            cpu: "AMD Ryzen 9 7950X3D 16-Core Processor".into(),
            ram_gb: 64.0,
            os: "Windows 10 Enterprise LTSC 21H2".into(),
        };
        card.from = "a-rather-long-organisation-name/an-extremely-long-repository-name @ 0123456789ab (declared)".into();
        card.player = "a-very-long-github-login-name-xx".into();
        card.serial = "3b8db536".into();
        card.share = "[KLBF Lab Bench BM-v1] a-rather-long-organisation-name/an-extremely-long-repository-name@0123456789ab | RTX 5080 Laptop GPU | Ryzen 9 7950X3D 16-Core Processor | avg 60.0 fps, 1% low 58.5 | CERTIFIED | 3b8db536".into();
        card.png_name = "bench-results\\KLBF-Bench-BM-v1-20261126-2104-3b8db536.png".into();
        card.set.draws_per_frame = 123_456.0;
        card.set.skipped = 12_345;
        for gpu in ["NVIDIA GeForce RTX 5080 Laptop GPU", "AVeryLongGpuNameWithoutAnySpacesAtAllThatCannotBeWrappedWordByWordAnywhere"] {
            card.sys.gpu = gpu.into();
            let (w, h) = (1280, 820);
            let (_, boxes) = render_card_boxes(&card, w, h, false);
            let k = (w as f32 / 1000.0).min(h as f32 / 640.0);
            let u = |px: i32| px as f32 / k;
            assert!(boxes.len() > 10);
            for [x0, y0, x1, y1] in boxes {
                let (x0, y0, x1, y1) = (u(x0), u(y0), u(x1), u(y1));
                let rows = y0 >= 15.0 && y1 <= 625.0;
                let left = x0 >= 15.0 && x1 <= 495.0;
                let right = x0 >= 505.0 && x1 <= 985.0;
                let saved = y0 > 600.0 && x0 >= 15.0 && x1 <= 985.0;
                assert!(rows && (left || right || saved), "text at ({x0:.0}, {y0:.0})-({x1:.0}, {y1:.0}) leaves its panel ({gpu})");
            }
        }
    }

    /// Writes the card of the sample run to `KLBF_CARD_PREVIEW` (a .png path), to look at it.
    #[test]
    #[ignore = "writes a file; run with KLBF_CARD_PREVIEW=<file.png>"]
    fn card_preview() {
        let mut card = sample_card();
        card.serial = "3b8db536".into();
        card.share = "[KLBF Lab Bench BM-v1] PastiiJay/klbf-lab@abc1234 | RX 9070 XT | Ryzen 7 7800X3D | avg 60.0 fps, 1% low 58.5 | CERTIFIED | 3b8db536".into();
        card.png_name = "bench-results/KLBF-Bench-BM-v1-20261126-2104-3b8db536.png".into();
        card.from = "PastiiJay/klbf-lab @ abc1234".into();
        card.date = "26 Nov 2026".into();
        card.date_short = "26.11.26".into();
        card.set.draws_per_frame = 8731.0;
        let out = std::env::var("KLBF_CARD_PREVIEW").expect("KLBF_CARD_PREVIEW=<file.png>");
        std::fs::write(&out, render_png(&card)).unwrap();
        // And with long names, next to it.
        card.sys.gpu = "NVIDIA GeForce RTX 5080 Laptop GPU".into();
        card.sys.cpu = "AMD Ryzen 9 7950X3D 16-Core Processor".into();
        card.sys.os = "Windows 10 Enterprise LTSC 21H2".into();
        card.from = "a-rather-long-organisation-name/an-extremely-long-repository-name @ 0123456789ab (declared)".into();
        std::fs::write(out.replace(".png", "-long.png"), render_png(&card)).unwrap();
    }

    /// A result the game writes, byte for byte the fixture `tools/bench-check.py` accepts
    /// (`python -I tools/bench-check.py crates/victory-gpu/tests/fixtures/KLBF-Bench-BM-v1-3b8db536.json`).
    #[test]
    fn result_matches_the_checker_fixture() {
        let card = sample_card();
        let extra = Extra { black: 0, removed: false, compiled: [3, 7], vram: (9000 << 20, 15000 << 20), ram: 12345 << 20, peaks: (13000 << 20, 9500 << 20), why: "5:00 reached" };
        let mut doc = result_json(&card, &extra, &[60.0, 57.3, 61.2]);
        let J::O(m) = &mut doc else { unreachable!() };
        // The build's own project and commit, replaced by the fixture's.
        m.insert("project", J::S("PastiiJay/klbf-lab".into()));
        m.insert("project_source", J::S("git".into()));
        m.insert("sha", J::S("abc1234".into()));
        let id = serial(&doc);
        assert_eq!(id, "3b8db536");
        let share = format!("[KLBF Lab Bench BM-v1] PastiiJay/klbf-lab@abc1234 | RX 9070 XT | Ryzen 7 7800X3D | avg 60.0 fps, 1% low 58.5 | CERTIFIED | {id}");
        let J::O(m) = &mut doc else { unreachable!() };
        m.insert("serial", J::S(id));
        m.insert("share", J::S(share));
        assert_eq!(doc.canonical(), include_str!("../tests/fixtures/KLBF-Bench-BM-v1-3b8db536.json"));
    }

    #[test]
    fn png_round_trip_and_template() {
        let rgb: Vec<u8> = (0..5 * 3 * 3).map(|i| (i * 37 % 251) as u8).collect();
        let png = encode_png(5, 3, &rgb);
        let (w, h, px) = decode_png(&png).expect("decodes");
        assert_eq!((w, h), (5, 3));
        let back: Vec<u8> = px.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        assert_eq!(back, rgb);
        let (w, h, _) = decode_png(TEMPLATE).expect("template decodes");
        assert_eq!((w, h), (1280, 820));
    }
}
