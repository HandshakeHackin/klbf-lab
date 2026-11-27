//! (27/11) KLBF Lab's in-game keys and menu, one press each while the game window has the focus: F10
//! closes the game for the launcher's settings (exit code 10, play.ps1 opens KLBF Lab Settings, Play
//! starts it again), F11 restarts it with the same settings (11), F8 saves a screenshot of the game
//! image (`klbf_bench::save_screenshot`). The same three are items of the window's menu bar, at its
//! right. The keys come as window messages (`on_message`, from the title window's procedure), so a short
//! tap is never missed between two frames. During a bench run none of them acts (F10 is the bench's:
//! it stops the run) and the window gets no menu.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateMenu, HMENU, MF_STRING, MFT_RIGHTJUSTIFY, WM_COMMAND, WM_KEYDOWN, WM_SYSKEYDOWN,
};

/// Exit codes play.ps1 reads.
pub const EXIT_SETTINGS: i32 = 10;
pub const EXIT_RESTART: i32 = 11;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Screenshot,
    Restart,
    Settings,
}

/// In the menu bar's order (left to right), with each one's key and menu id.
const ALL: [(Action, usize, u16); 3] =
    [(Action::Screenshot, 0x77, 0x4B08), (Action::Restart, 0x7A, 0x4B0B), (Action::Settings, 0x79, 0x4B0A)];

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

    fn of_command(id: usize) -> Option<Action> {
        ALL.iter().find(|a| usize::from(a.2) == id).map(|a| a.0)
    }
}

/// What a window message asks for: a first press of F8, F10 or F11 (not a key repeat, lparam bit 30), or
/// one of our menu items. F10 comes as a system key.
fn action_of(message: u32, wparam: usize, lparam: isize) -> Option<Action> {
    match message {
        WM_KEYDOWN | WM_SYSKEYDOWN if lparam & (1 << 30) == 0 => Action::of_key(wparam),
        WM_COMMAND if lparam == 0 && wparam >> 16 == 0 => Action::of_command(wparam & 0xFFFF),
        _ => None,
    }
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
    let Some(action) = action_of(message, wparam, lparam) else { return false };
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

/// The game window's menu bar: our three items at its right. Null during a bench run.
pub fn menu() -> HMENU {
    if crate::klbf_bench::active() {
        return std::ptr::null_mut();
    }
    // SAFETY: a new menu and NUL-terminated texts.
    unsafe {
        let m = CreateMenu();
        if m.is_null() {
            return m;
        }
        for (i, (action, _, id)) in ALL.iter().enumerate() {
            let text: Vec<u16> = action.label().encode_utf16().chain([0]).collect();
            // The first item right-justified takes those after it to the right too.
            let flags = if i == 0 { MF_STRING | MFT_RIGHTJUSTIFY } else { MF_STRING };
            AppendMenuW(m, flags, usize::from(*id), text.as_ptr());
        }
        m
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
    fn each_key_and_menu_item_does_one_thing_on_one_press() {
        let down = |vk: usize| action_of(WM_KEYDOWN, vk, 1);
        assert_eq!(down(0x77), Some(Action::Screenshot));
        assert_eq!(down(0x7A), Some(Action::Restart));
        assert_eq!(action_of(WM_SYSKEYDOWN, 0x79, 1), Some(Action::Settings), "F10 comes as a system key");
        assert_eq!(down(0x78), None, "F9 is not ours");
        assert_eq!(action_of(WM_KEYDOWN, 0x7A, 1 << 30), None, "a held key's repeats do nothing");
        assert_eq!(action_of(WM_COMMAND, 0x4B0A, 0), Some(Action::Settings));
        assert_eq!(action_of(WM_COMMAND, 0x4B08, 0), Some(Action::Screenshot));
        assert_eq!(action_of(WM_COMMAND, 0x1_4B0A, 0), None, "an accelerator's command is the title's");
        assert_eq!(action_of(WM_COMMAND, 0x4B0A, 1234), None, "a control's command is the title's");
        assert_eq!(Action::Settings.exit_code(), Some(10));
        assert_eq!(Action::Restart.exit_code(), Some(11));
        assert_eq!(Action::Screenshot.exit_code(), None);
        let labels: Vec<_> = ALL.iter().map(|a| a.0.label()).collect();
        assert_eq!(labels, ["Screenshot (F8)", "Restart (F11)", "Settings (F10)"]);
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
