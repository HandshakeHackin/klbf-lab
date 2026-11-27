//! (27/11) KLBF Lab's in-game keys and menu, one press each while the game window has the focus: F10
//! closes the game for the launcher's settings (exit code 10, play.ps1 opens KLBF Lab Settings, Play
//! starts it again), F11 restarts it with the same settings (11), F8 saves a screenshot of the game
//! image (`klbf_bench::save_screenshot`). With our launcher (`KLBF_LAUNCHER`) the caption says them at
//! its right (`with_hints`). The keys come as window messages (`on_message`, from the title window's
//! procedure), so a short tap is never missed between two frames. During a bench run none of them acts
//! (F10 is the bench's: it stops the run). Started by another launcher (no `KLBF_LAUNCHER`,
//! [`launcher`]), nobody restarts the game: F10 / F11 are the title's (F8 stays).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{WM_KEYDOWN, WM_SYSKEYDOWN};

/// Exit codes play.ps1 reads.
pub const EXIT_SETTINGS: i32 = 10;
pub const EXIT_RESTART: i32 = 11;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Screenshot,
    Restart,
    Settings,
}

/// Each action's key.
const ALL: [(Action, usize); 3] = [(Action::Screenshot, 0x77), (Action::Restart, 0x7A), (Action::Settings, 0x79)];

impl Action {
    /// The exit code play.ps1 acts on; `None`: done in the game.
    pub fn exit_code(self) -> Option<i32> {
        match self {
            Action::Screenshot => None,
            Action::Restart => Some(EXIT_RESTART),
            Action::Settings => Some(EXIT_SETTINGS),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::Screenshot => "Screenshot (F8)",
            Action::Restart => "Restart (F11)",
            Action::Settings => "Settings (F10)",
        }
    }

    fn of_key(vk: usize) -> Option<Action> {
        ALL.iter().find(|a| a.1 == vk).map(|a| a.0)
    }
}

/// What a window message asks for: a first press of F8, F10 or F11 (not a key repeat, lparam bit 30).
/// F10 comes as a system key.
fn action_of(message: u32, wparam: usize, lparam: isize) -> Option<Action> {
    match message {
        WM_KEYDOWN | WM_SYSKEYDOWN if lparam & (1 << 30) == 0 => Action::of_key(wparam),
        _ => None,
    }
}

/// Whether our launcher (play.ps1, `KLBF_LAUNCHER`) started the game: it acts on exit codes 10 and 11.
pub fn launcher() -> bool {
    static OURS: OnceLock<bool> = OnceLock::new();
    *OURS.get_or_init(|| std::env::var_os("KLBF_LAUNCHER").is_some_and(|v| !v.is_empty()))
}

/// Whether `action` acts for this start: F10 / F11 only under our launcher.
fn acts(action: Action, launcher: bool) -> bool {
    launcher || action.exit_code().is_none()
}

static EXIT: OnceLock<fn(i32)> = OnceLock::new();
static SHOT: AtomicBool = AtomicBool::new(false);

/// How the game ends with an exit code (the title's own end: suspend, save the pipeline cache, exit).
pub fn on_exit(f: fn(i32)) {
    let _ = EXIT.set(f);
}

/// From a game window's procedure: true when the message was ours (then it is not passed on).
pub fn on_message(message: u32, wparam: usize, lparam: isize) -> bool {
    if crate::klbf_bench::active() {
        return false;
    }
    let Some(action) = action_of(message, wparam, lparam).filter(|&a| acts(a, launcher())) else { return false };
    match action.exit_code() {
        None => SHOT.store(true, Ordering::Relaxed),
        Some(code) => {
            tracing::info!(code, "KLBF Lab: {} asked, the game closes for the launcher", action.label());
            match EXIT.get() {
                Some(f) => f(code),
                None => std::process::exit(code),
            }
        }
    }
    true
}

/// (27/11) The keys, said at the caption's right.
const HINTS: &str = "F8 Screenshot \u{b7} F10 Settings \u{b7} F11 Restart";

/// `caption` with the keys pushed to its right end, just before the window's buttons, by spaces measured
/// in the caption's font; the caption alone when they do not fit, during a bench, or without our
/// launcher (`KLBF_LAUNCHER`: F10 and F11 need it). Measured at each call (the window may have changed size).
pub(crate) fn with_hints(hwnd: usize, caption: String) -> String {
    if std::env::var_os("KLBF_LAUNCHER").is_none() || crate::klbf_bench::active() {
        return caption;
    }
    match measure(hwnd, &caption).and_then(|([c, h, room], sp)| spaces(c, h, sp, room)) {
        Some(n) => format!("{caption}{}{HINTS}", " ".repeat(n)),
        None => caption,
    }
}

/// How many spaces of `space` px put hints `hints` px wide at the end of `room` px after a caption
/// `caption` px wide; `None` when less than two spaces would be left between them.
fn spaces(caption: i32, hints: i32, space: f32, room: i32) -> Option<usize> {
    let gap = (room - caption - hints) as f32;
    (space > 0.0 && gap >= 2.0 * space).then(|| (gap / space) as usize)
}

/// ([caption, hints, room], one space) in px (a space's width from a hundred, so a long run of them adds up): the caption's font (the system's), the room from after the
/// window's icon to its buttons (DWM's bounds, else three caption buttons' widths).
fn measure(hwnd: usize, caption: &str) -> Option<([i32; 3], f32)> {
    use windows_sys::Win32::Foundation::{HWND, RECT, SIZE};
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_CAPTION_BUTTON_BOUNDS, DwmGetWindowAttribute};
    use windows_sys::Win32::Graphics::Gdi::{CreateFontIndirectW, DeleteObject, GetDC, GetTextExtentPoint32W, ReleaseDC, SelectObject};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, GetWindowRect, NONCLIENTMETRICSW, SM_CXFRAME, SM_CXPADDEDBORDER, SM_CXSIZE, SM_CXSMICON, SPI_GETNONCLIENTMETRICS,
        SystemParametersInfoW,
    };
    let hwnd = hwnd as HWND;
    // SAFETY: plain queries; a font and a screen DC made and freed here.
    unsafe {
        let mut ncm: NONCLIENTMETRICSW = std::mem::zeroed();
        ncm.cbSize = size_of::<NONCLIENTMETRICSW>() as u32;
        if SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, ncm.cbSize, (&raw mut ncm).cast(), 0) == 0 {
            return None;
        }
        let mut window = RECT::default();
        if GetWindowRect(hwnd, &mut window) == 0 {
            return None;
        }
        let mut buttons = RECT::default();
        let left_of_buttons = if DwmGetWindowAttribute(hwnd, DWMWA_CAPTION_BUTTON_BOUNDS as u32, (&raw mut buttons).cast(), size_of::<RECT>() as u32) >= 0 && buttons.left > 0 {
            buttons.left
        } else {
            window.right - window.left - 3 * GetSystemMetrics(SM_CXSIZE)
        };
        let start = GetSystemMetrics(SM_CXFRAME) + GetSystemMetrics(SM_CXPADDEDBORDER) + GetSystemMetrics(SM_CXSMICON) + 8;
        let room = left_of_buttons - start - 16;
        let font = CreateFontIndirectW(&ncm.lfCaptionFont);
        let dc = GetDC(std::ptr::null_mut());
        if font.is_null() || dc.is_null() {
            return None;
        }
        let old = SelectObject(dc, font);
        let width = |s: &str| {
            let t: Vec<u16> = s.encode_utf16().collect();
            let mut size = SIZE { cx: 0, cy: 0 };
            GetTextExtentPoint32W(dc, t.as_ptr(), t.len() as i32, &mut size);
            size.cx
        };
        let out = ([width(caption), width(HINTS), room], width(&" ".repeat(100)) as f32 / 100.0);
        SelectObject(dc, old);
        DeleteObject(font);
        ReleaseDC(std::ptr::null_mut(), dc);
        Some(out)
    }
}

/// Whether F8 asked for a screenshot since the last call (`Host::present_planes` takes it).
pub(crate) fn take_shot() -> bool {
    SHOT.swap(false, Ordering::Relaxed)
}

/// A readback buffer and its layout, sent to the saving thread.
pub(crate) struct Shot(ID3D12Resource, D3D12_PLACED_SUBRESOURCE_FOOTPRINT);
// SAFETY: a D3D12 resource is free-threaded; the copy into it has finished before it is sent.
unsafe impl Send for Shot {}

/// Records a copy of `back` (in the copy-source state) into a new readback buffer.
pub(crate) fn read_back(device: &ID3D12Device, list: &ID3D12GraphicsCommandList, back: &ID3D12Resource) -> Option<Shot> {
    let mut footprint = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
    let mut total = 0;
    // SAFETY: a live texture; outputs for one subresource.
    unsafe { device.GetCopyableFootprints(&back.GetDesc(), 0, 1, 0, Some(&mut footprint), None, None, Some(&mut total)) };
    let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_READBACK, ..Default::default() };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: total,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let mut buffer: Option<ID3D12Resource> = None;
    // SAFETY: plain creation; an open list; locations naming live resources.
    unsafe {
        device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut buffer).ok()?;
        let buffer = buffer?;
        let to = D3D12_TEXTURE_COPY_LOCATION {
            pResource: std::mem::transmute_copy(&buffer),
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: footprint },
        };
        let from = D3D12_TEXTURE_COPY_LOCATION {
            pResource: std::mem::transmute_copy(back),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
        };
        list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
        Some(Shot(buffer, footprint))
    }
}

/// After the copy has run: the pixels are read, watermarked and written on a thread of their own, with
/// `stats` (the caption's figures at this frame).
pub(crate) fn save(shot: Shot, stats: String) {
    let spawned = std::thread::Builder::new().name("klbf-screenshot".into()).spawn(move || {
        let Shot(buffer, footprint) = shot;
        let f = footprint.Footprint;
        let mut data = std::ptr::null_mut();
        // SAFETY: the copy has finished; the mapping spans the footprint's rows.
        let rgb = unsafe {
            if buffer.Map(0, None, Some(&mut data)).is_err() {
                return;
            }
            let rows = std::slice::from_raw_parts(data.cast::<u8>(), f.RowPitch as usize * f.Height as usize);
            let rgb = to_rgb(f.Format, f.Width as usize, f.Height as usize, f.RowPitch as usize, rows);
            buffer.Unmap(0, None);
            rgb
        };
        match rgb {
            Some(rgb) => crate::klbf_bench::save_screenshot(f.Width as usize, f.Height as usize, &rgb, &stats),
            None => tracing::warn!(format = f.Format.0, "KLBF Lab: screenshot not saved (the picture's format is not read yet)"),
        }
    });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "KLBF Lab: screenshot not saved");
    }
}

/// 8-bit RGB of rows of 8-bit RGBA or BGRA, or 10-bit RGB (A2). ponytail: no float (HDR) formats; add
/// a tone map when a title presents one.
fn to_rgb(format: DXGI_FORMAT, w: usize, h: usize, pitch: usize, rows: &[u8]) -> Option<Vec<u8>> {
    let px: fn(&[u8]) -> [u8; 3] = match format {
        DXGI_FORMAT_R8G8B8A8_UNORM | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => |p| [p[0], p[1], p[2]],
        DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB => |p| [p[2], p[1], p[0]],
        DXGI_FORMAT_R10G10B10A2_UNORM => |p| {
            let v = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
            [v >> 2, v >> 12, v >> 22].map(|c| c as u8)
        },
        _ => return None,
    };
    let mut out = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        out.extend(rows[y * pitch..][..w * 4].chunks_exact(4).flat_map(px));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_key_does_one_thing_on_one_press() {
        let down = |vk: usize| action_of(WM_KEYDOWN, vk, 1);
        assert_eq!(down(0x77), Some(Action::Screenshot));
        assert_eq!(down(0x7A), Some(Action::Restart));
        assert_eq!(action_of(WM_SYSKEYDOWN, 0x79, 1), Some(Action::Settings), "F10 comes as a system key");
        assert_eq!(down(0x78), None, "F9 is not ours");
        assert_eq!(action_of(WM_KEYDOWN, 0x7A, 1 << 30), None, "a held key's repeats do nothing");
        assert_eq!(Action::Settings.exit_code(), Some(10));
        assert_eq!(Action::Restart.exit_code(), Some(11));
        assert_eq!(Action::Screenshot.exit_code(), None);
    }

    /// The hints end where the caption's room ends; dropped when fewer than two spaces would separate them.
    #[test]
    fn the_hints_are_pushed_to_the_right_or_dropped() {
        // 1600 px of room, a 700 px caption, 300 px of hints, 5 px spaces: 600 px of spaces.
        assert_eq!(spaces(700, 300, 5.0, 1600), Some(120));
        let n = spaces(700, 300, 3.4, 1600).unwrap() as f32;
        assert!(700.0 + n * 3.4 + 300.0 <= 1600.0 && 700.0 + (n + 1.0) * 3.4 + 300.0 > 1600.0, "as far right as it fits");
        assert_eq!(spaces(1295, 300, 5.0, 1600), None, "one space left: no hints");
        assert_eq!(spaces(1290, 300, 5.0, 1600), Some(2));
        assert_eq!(spaces(700, 300, 0.0, 1600), None, "no font measured");
        assert_eq!(spaces(1500, 300, 5.0, 1600), None, "a caption longer than the room");
    }

    #[test]
    fn f10_f11_only_under_our_launcher() {
        assert!(acts(Action::Screenshot, false), "F8 always");
        assert!(!acts(Action::Settings, false) && !acts(Action::Restart, false));
        assert!(acts(Action::Settings, true) && acts(Action::Restart, true));
    }

    #[test]
    fn ten_bit_pixels_keep_their_top_eight_bits() {
        // One pixel R = 1023, G = 0, B = 512, then padding to a pitch of 8.
        let v: u32 = 1023 | (512 << 20);
        let mut rows = v.to_le_bytes().to_vec();
        rows.extend([9; 4]);
        assert_eq!(to_rgb(DXGI_FORMAT_R10G10B10A2_UNORM, 1, 1, 8, &rows), Some(vec![255, 0, 128]));
        assert_eq!(to_rgb(DXGI_FORMAT_B8G8R8A8_UNORM, 1, 1, 8, &[1, 2, 3, 4, 0, 0, 0, 0]), Some(vec![3, 2, 1]));
        assert_eq!(to_rgb(DXGI_FORMAT_R16G16B16A16_FLOAT, 1, 1, 8, &[0; 8]), None);
    }
}
