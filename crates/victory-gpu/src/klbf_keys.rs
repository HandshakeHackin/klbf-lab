//! (27/11) KLBF Lab's in-game keys and menu, one press each while the game window has the focus: F10
//! closes the game for the launcher's settings (exit code 10, play.ps1 opens KLBF Lab Settings, Play
//! starts it again), F11 restarts it with the same settings (11), F8 saves a screenshot of the game
//! image (`klbf_bench::save_screenshot`). With our launcher (`KLBF_LAUNCHER`) the caption says them at
//! its right (`with_hints`). The keys come as window messages (`on_message`, from the title window's
//! procedure), so a short tap is never missed between two frames. During a bench run none of them acts
//! (F10 is the bench's: it stops the run). Started by another launcher (no `KLBF_LAUNCHER`,
//! [`launcher`]), nobody restarts the game: F10 / F11 are the title's (F8 stays).
//!
//! (27/11) F8 says how it went: a short white flash over the game (drawn on the presented image after
//! it was read, so not in the PNG) and "Screenshot saved" in the caption, or "Screenshot failed:
//! <reason>" in the caption and the log. The keys reach us only while the game window has the keyboard
//! focus (`lifecycle::come_forward` takes it at the start).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

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
    if crate::klbf_bench::active() {
        return caption;
    }
    // (27/11) F8's outcome, for a few seconds, in place of the hints (with any launcher).
    if let Some(notice) = notice_now() {
        return format!("{caption}  \u{2014}  {notice}");
    }
    if std::env::var_os("KLBF_LAUNCHER").is_none() {
        return caption;
    }
    match measure(hwnd, &caption).and_then(|([c, h, room], sp)| spaces(c, h, sp, room)) {
        Some(n) => format!("{caption}{}{HINTS}", " ".repeat(n)),
        None => caption,
    }
}

/// How long the caption says F8's outcome.
const NOTICE_FOR: Duration = Duration::from_secs(4);

/// F8's outcome and when it came.
static NOTICE: Mutex<Option<(String, Instant)>> = Mutex::new(None);

/// The outcome still to be shown, if any.
fn notice_now() -> Option<String> {
    let notice = NOTICE.lock().unwrap_or_else(|e| e.into_inner());
    notice.as_ref().filter(|(_, at)| at.elapsed() < NOTICE_FOR).map(|(text, _)| text.clone())
}

/// What the caption says after F8: "Screenshot saved", or "Screenshot failed: <reason>".
fn outcome(result: &Result<String, String>) -> String {
    match result {
        Ok(_) => "Screenshot saved".into(),
        Err(why) => format!("Screenshot failed: {why}"),
    }
}

/// F8's outcome: in the log (a failure as a warning, with its reason) and in the caption.
pub(crate) fn report(result: Result<String, String>) {
    match &result {
        // The file's name only in the log (it is posted).
        Ok(name) => tracing::info!("KLBF Lab: screenshot saved ({name})"),
        Err(why) => tracing::warn!("KLBF Lab: screenshot failed: {why}"),
    }
    *NOTICE.lock().unwrap_or_else(|e| e.into_inner()) = Some((outcome(&result), Instant::now()));
}

/// (27/11) The white flash's strength at its first frame, and how long it fades out after it.
const FLASH_ALPHA: f32 = 0.85;
const FLASH_FOR: Duration = Duration::from_millis(120);

/// The flash asked for: when it was first drawn (`None`: not yet).
static FLASH: Mutex<Option<Option<Instant>>> = Mutex::new(None);

/// A white flash over the next frames (a screenshot was taken).
fn flash() {
    *FLASH.lock().unwrap_or_else(|e| e.into_inner()) = Some(None);
}

/// The flash's opacity for the frame being presented, `None` when there is none. Its first frame is always
/// drawn at full strength (at 15 fps a frame lasts longer than the whole fade), then it fades by time.
pub(crate) fn flash_alpha() -> Option<f32> {
    let mut flash = FLASH.lock().unwrap_or_else(|e| e.into_inner());
    let alpha = fade(flash.as_ref().copied()?.map(|at| at.elapsed()));
    match alpha {
        Some(_) if matches!(*flash, Some(None)) => *flash = Some(Some(Instant::now())),
        None => *flash = None,
        _ => {}
    }
    alpha
}

/// The opacity `since` the flash's first frame (`None`: this is it).
fn fade(since: Option<Duration>) -> Option<f32> {
    match since {
        None => Some(FLASH_ALPHA),
        Some(t) if t < FLASH_FOR => Some(FLASH_ALPHA * (1.0 - t.as_secs_f32() / FLASH_FOR.as_secs_f32())),
        Some(_) => None,
    }
}

const FLASH_SOURCE: &str = r#"
#define RS "RootConstants(num32BitConstants=1, b0)"
cbuffer C : register(b0) { float alpha; };
[RootSignature(RS)]
float4 vs(uint id : SV_VertexID) : SV_Position {
    float2 uv = float2((id << 1) & 2, id & 2);
    return float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
}
[RootSignature(RS)]
float4 ps() : SV_Target { return float4(1, 1, 1, alpha); }
"#;

/// The flash's pipeline for one render target format.
struct FlashPipeline(DXGI_FORMAT, ID3D12RootSignature, ID3D12PipelineState);
// SAFETY: D3D12 objects are free-threaded.
unsafe impl Send for FlashPipeline {}

fn flash_pipeline(device: &ID3D12Device, format: DXGI_FORMAT) -> windows::core::Result<FlashPipeline> {
    use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
    use windows::Win32::Graphics::Direct3D::ID3DBlob;
    use windows::core::{PCSTR, s};
    let compile = |entry: PCSTR, target: PCSTR| -> windows::core::Result<ID3DBlob> {
        let mut code: Option<ID3DBlob> = None;
        // SAFETY: the source and names live through the call; the out pointer is ours.
        unsafe { D3DCompile(FLASH_SOURCE.as_ptr().cast(), FLASH_SOURCE.len(), s!("flash"), None, None, entry, target, 0, 0, &mut code, None)? };
        code.ok_or_else(windows::core::Error::empty)
    };
    let (vs, ps) = (compile(s!("vs"), s!("vs_5_1"))?, compile(s!("ps"), s!("ps_5_1"))?);
    // SAFETY: the blobs' own bytes, alive through the calls.
    unsafe {
        let bytes = |b: &ID3DBlob| std::slice::from_raw_parts(b.GetBufferPointer().cast::<u8>(), b.GetBufferSize());
        let code = |b: &ID3DBlob| D3D12_SHADER_BYTECODE { pShaderBytecode: b.GetBufferPointer(), BytecodeLength: b.GetBufferSize() };
        let root: ID3D12RootSignature = device.CreateRootSignature(0, bytes(&vs))?;
        let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::transmute_copy(&root),
            VS: code(&vs),
            PS: code(&ps),
            SampleMask: u32::MAX,
            RasterizerState: D3D12_RASTERIZER_DESC { FillMode: D3D12_FILL_MODE_SOLID, CullMode: D3D12_CULL_MODE_NONE, ..Default::default() },
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            NumRenderTargets: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            ..Default::default()
        };
        desc.RTVFormats[0] = format;
        desc.BlendState.RenderTarget[0] = D3D12_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(),
            LogicOpEnable: false.into(),
            SrcBlend: D3D12_BLEND_SRC_ALPHA,
            DestBlend: D3D12_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D12_BLEND_OP_ADD,
            SrcBlendAlpha: D3D12_BLEND_ZERO,
            DestBlendAlpha: D3D12_BLEND_ONE,
            BlendOpAlpha: D3D12_BLEND_OP_ADD,
            LogicOp: D3D12_LOGIC_OP_NOOP,
            RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };
        let pso = device.CreateGraphicsPipelineState(&desc)?;
        Ok(FlashPipeline(format, root, pso))
    }
}

/// Records the flash at `alpha` over `back` (`format`, `width` x `height`, in the render target state).
/// A pipeline that cannot be made is said once and the frame shown without the flash.
pub(crate) fn draw_flash(host: &crate::host::Host, list: &ID3D12GraphicsCommandList, back: &ID3D12Resource, format: DXGI_FORMAT, (width, height): (u32, u32), alpha: f32) {
    static PIPELINE: Mutex<Option<FlashPipeline>> = Mutex::new(None);
    let mut pipeline = PIPELINE.lock().unwrap_or_else(|e| e.into_inner());
    if pipeline.as_ref().is_none_or(|p| p.0 != format) {
        match flash_pipeline(&host.device, format) {
            Ok(p) => *pipeline = Some(p),
            Err(e) => {
                static SAID: AtomicBool = AtomicBool::new(false);
                if !SAID.swap(true, Ordering::Relaxed) {
                    tracing::warn!(error = %e, "KLBF Lab: screenshot flash not drawn");
                }
                return;
            }
        }
    }
    let Some(FlashPipeline(_, root, pso)) = pipeline.as_ref() else { return };
    let rtv = host.rtv(Some(back), None);
    // SAFETY: an open list; our live back buffer in the render target state.
    unsafe {
        list.OMSetRenderTargets(1, Some(&rtv), false, None);
        list.SetGraphicsRootSignature(root);
        list.SetPipelineState(pso);
        list.SetGraphicsRoot32BitConstant(0, alpha.to_bits(), 0);
        list.IASetPrimitiveTopology(windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        list.RSSetViewports(&[D3D12_VIEWPORT { Width: width as f32, Height: height as f32, MaxDepth: 1.0, ..Default::default() }]);
        list.RSSetScissorRects(&[windows::Win32::Foundation::RECT { left: 0, top: 0, right: width as i32, bottom: height as i32 }]);
        list.DrawInstanced(3, 1, 0, 0);
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

/// Records a copy of `back` (in the copy-source state) into a new readback buffer; why not, when it cannot.
pub(crate) fn read_back(device: &ID3D12Device, list: &ID3D12GraphicsCommandList, back: &ID3D12Resource) -> Result<Shot, String> {
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
        device
            .CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut buffer)
            .map_err(|e| format!("no memory to read the picture ({e})"))?;
        let buffer = buffer.ok_or("no memory to read the picture")?;
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
        Ok(Shot(buffer, footprint))
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
            if let Err(e) = buffer.Map(0, None, Some(&mut data)) {
                return report(Err(format!("the picture could not be read ({e})")));
            }
            let rows = std::slice::from_raw_parts(data.cast::<u8>(), f.RowPitch as usize * f.Height as usize);
            let rgb = to_rgb(f.Format, f.Width as usize, f.Height as usize, f.RowPitch as usize, rows);
            buffer.Unmap(0, None);
            rgb
        };
        let Some(rgb) = rgb else {
            return report(Err(format!("picture format {} not supported", f.Format.0)));
        };
        flash();
        report(crate::klbf_bench::save_screenshot(f.Width as usize, f.Height as usize, &rgb, &stats));
    });
    if let Err(e) = spawned {
        report(Err(format!("no thread to save it ({e})")));
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

    /// (27/11) F8 always says how it went, and the flash fades out after its first frame.
    #[test]
    fn a_screenshot_says_how_it_went() {
        assert_eq!(outcome(&Ok("KLBF-Lab-x.png".into())), "Screenshot saved");
        assert_eq!(outcome(&Err("disk full".into())), "Screenshot failed: disk full");
        report(Err("disk full".into()));
        assert_eq!(notice_now().as_deref(), Some("Screenshot failed: disk full"));
        assert_eq!(fade(None), Some(FLASH_ALPHA), "the first frame always at full strength");
        let half = fade(Some(FLASH_FOR / 2)).unwrap();
        assert!((half - FLASH_ALPHA / 2.0).abs() < 0.01);
        assert_eq!(fade(Some(FLASH_FOR)), None, "gone after 120 ms");
        assert_eq!(flash_alpha(), None, "no flash asked");
        flash();
        assert_eq!(flash_alpha(), Some(FLASH_ALPHA));
        std::thread::sleep(FLASH_FOR);
        assert_eq!(flash_alpha(), None);
        assert_eq!(flash_alpha(), None, "once");
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
