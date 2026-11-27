//! `amd_fidelityfx_loader_xbox.dll`, redirected to a PC FidelityFX DLL.
//!
//! The Series S title upscales with AMD's FSR 4 through the FidelityFX API
//! (`ffxCreateContext`, `ffxDispatch`, ...). The console build and the PC
//! build export the same five functions. With `VICTORY_FSR_DLL=<path>` set
//! (`amd_fidelityfx_upscaler_dx12.dll`, or the `loader_dx12` beside it) the
//! title's loader is not mapped: its `LoadLibrary` gets this module, and
//! each call goes to the PC DLL, so tools that hook that DLL (OptiScaler)
//! see the title's upscaler like any PC game's.
//!
//! The PC DLL needs real D3D12 objects where the title passes its console
//! ones: the device in the context's backend description, and in a
//! dispatch the command list and the textures, which get the host list the
//! title's list records onto and the host textures behind the title's, in
//! the states the title declares. Descriptions are patched in place for the
//! call and restored after it.

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use victory_gpu::external;

use crate::HleModule;

/// The environment variable that turns the redirect on.
const VARIABLE: &str = "VICTORY_FSR_DLL";

/// The title DLLs the redirect answers for.
const DLLS: [&str; 3] = [
    "amd_fidelityfx_loader_xbox.dll",
    "amd_fidelityfx_upscaler_xbox.dll",
    "amd_fidelityfx_framegeneration_xbox.dll",
];

fn path() -> Option<std::path::PathBuf> {
    std::env::var_os(VARIABLE)
        .filter(|p| !p.is_empty())
        .map(Into::into)
}

/// Whether the redirect is on.
pub fn enabled() -> bool {
    path().is_some()
}

/// Whether the title's `file` is answered by the redirect instead of being
/// mapped from the title's folder.
pub fn replaces(file: &str) -> bool {
    enabled() && DLLS.iter().any(|d| d.eq_ignore_ascii_case(file))
}

// ffxReturnCode_t
const OK: u32 = 0;
const ERROR: u32 = 1;
const ERROR_PARAMETER: u32 = 6;

// Description types (`ffx_api_types.h`, `ffx_api_dx12.h`, `ffx_upscale.h`).
const DESC_OVERRIDE_VERSION: u64 = 0x1;
const CREATE_BACKEND_DX12: u64 = 0x2;
const CREATE_UPSCALE: u64 = 0x1_0000;
const DISPATCH_UPSCALE: u64 = 0x1_0001;
const QUERY_JITTER_PHASE_COUNT: u64 = 0x1_0004;
/// (27/11) 0x1_0006 in `ffx_upscale.h` (SDK 2.4); 0x1_0002, read before, is a query's type, so
/// a reactive mask dispatch went to the PC DLL with the title's objects in it.
const DISPATCH_GENERATE_REACTIVE_MASK: u64 = 0x1_0006;

type Create = unsafe extern "system" fn(*mut *mut c_void, *mut u8, *const c_void) -> u32;
type Destroy = unsafe extern "system" fn(*mut *mut c_void, *const c_void) -> u32;
type Header = unsafe extern "system" fn(*mut *mut c_void, *mut u8) -> u32;

struct Api {
    create: Create,
    destroy: Destroy,
    configure: Header,
    query: Header,
    dispatch: Header,
}

/// The PC DLL's functions, loaded on first use.
fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| {
        let path = path()?;
        match load(&path) {
            Ok(api) => {
                println!("fidelityfx     {} (host, for the title's upscaler)", path.display());
                Some(api)
            }
            Err(error) => {
                tracing::error!(path = %path.display(), %error, "the PC FidelityFX DLL");
                None
            }
        }
    })
    .as_ref()
}

fn load(path: &std::path::Path) -> Result<Api, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: a NUL-terminated path. The DLL's own folder is searched for
    // what it needs.
    let module = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) };
    if module.is_null() {
        return Err(format!("LoadLibrary failed: {}", std::io::Error::last_os_error()));
    }
    let export = |name: &'static [u8]| {
        // SAFETY: a loaded module and a NUL-terminated name.
        unsafe { GetProcAddress(module, name.as_ptr()) }
            .map(|f| f as usize)
            .ok_or_else(|| format!("no export {}", String::from_utf8_lossy(&name[..name.len() - 1])))
    };
    // SAFETY: the five exports have these signatures in every FidelityFX
    // API DLL (`ffx_api.h`).
    unsafe {
        Ok(Api {
            create: std::mem::transmute::<usize, Create>(export(b"ffxCreateContext\0")?),
            destroy: std::mem::transmute::<usize, Destroy>(export(b"ffxDestroyContext\0")?),
            configure: std::mem::transmute::<usize, Header>(export(b"ffxConfigure\0")?),
            query: std::mem::transmute::<usize, Header>(export(b"ffxQuery\0")?),
            dispatch: std::mem::transmute::<usize, Header>(export(b"ffxDispatch\0")?),
        })
    }
}

/// Words of title memory changed for one call, put back after it.
#[derive(Default)]
struct Patches(Vec<(*mut usize, usize)>);

impl Patches {
    /// Sets the word at `at` to `new`.
    ///
    /// # Safety
    /// `at` is a writable, aligned word of a description the title passed.
    unsafe fn set(&mut self, at: *mut usize, new: usize) {
        // SAFETY: as the caller says.
        unsafe {
            self.0.push((at, at.read()));
            at.write(new);
        }
    }
}

impl Drop for Patches {
    fn drop(&mut self) {
        for &(at, old) in self.0.iter().rev() {
            // SAFETY: written by `set` on the same call.
            unsafe { at.write(old) };
        }
    }
}

/// Logs the description types a call carries, once per type: what the
/// title sends is not documented for the console build.
fn note(call: &str, head: *const u8) {
    static SEEN: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());
    let mut node = head;
    for _ in 0..16 {
        if node.is_null() {
            break;
        }
        // SAFETY: a title description: type, then the next one.
        let (ty, next) = unsafe { (node.cast::<u64>().read_unaligned(), node.add(8).cast::<*const u8>().read_unaligned()) };
        let fresh = {
            let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
            let key = (call.to_owned(), ty);
            let fresh = !seen.contains(&key);
            if fresh {
                seen.push(key);
            }
            fresh
        };
        if fresh {
            // SAFETY: the first 12 words of a description, as a dump to
            // learn its layout from.
            let words: Vec<u64> = (0..12).map(|i| unsafe { node.cast::<u64>().add(i).read_unaligned() }).collect();
            tracing::info!(call, ty = format_args!("{ty:#x}"), words = ?words.iter().map(|w| format!("{w:#x}")).collect::<Vec<_>>(), "FidelityFX description");
        }
        node = next;
    }
}

/// Replaces the title's device in the words `from..to` (byte offsets) of
/// description `node`.
fn swap_device(patches: &mut Patches, node: *mut u8, from: usize, to: usize, host: *mut c_void) {
    let mut at = from;
    while at + 8 <= to {
        // SAFETY: inside a description the title passed; read unaligned
        // first, patched only where it holds the title's device.
        unsafe {
            let word = node.add(at).cast::<usize>();
            if external::is_title_device(word.read_unaligned() as *const c_void) && word.is_aligned() {
                patches.set(word, host as usize);
            }
        }
        at += 8;
    }
}

/// `ffxCreateContext(context, desc, allocation callbacks)`: the backend
/// description names the host device; one is added if the title sends
/// none. The title's allocation callbacks are not passed on (the host DLL
/// allocates itself), nor is a version override: the title names a
/// provider of the console build.
extern "system" fn ffxCreateContext(context: *mut *mut c_void, desc: *mut u8, _alloc: *const c_void) -> u32 {
    let Some(api) = api() else { return ERROR };
    let Some(device) = external::host_device() else { return ERROR };
    note("create", desc);
    let mut patches = Patches::default();
    let mut backend = [CREATE_BACKEND_DX12, 0, device as u64];
    let mut has_backend = false;
    let mut last = desc;
    let mut previous: *mut u8 = std::ptr::null_mut();
    let mut node = desc;
    for _ in 0..32 {
        if node.is_null() {
            break;
        }
        // SAFETY: a title description: type, then the next one.
        let (ty, next) = unsafe { (node.cast::<u64>().read_unaligned(), node.add(8).cast::<*mut u8>().read_unaligned()) };
        if ty == CREATE_BACKEND_DX12 {
            has_backend = true;
            swap_device(&mut patches, node, 16, 24, device);
        }
        if ty == CREATE_UPSCALE && victory_gpu::framegen::on() {
            // (KLBF Lab) Frame generation is made from the upscaler's flags (depth and motion
            // vector bits) and its largest render size.
            // SAFETY: `ffxCreateContextDescUpscale`: flags at +16, maxRenderSize at +20.
            let (flags, max_render) = unsafe {
                (node.add(16).cast::<u32>().read_unaligned(), [node.add(20).cast::<u32>().read_unaligned(), node.add(24).cast::<u32>().read_unaligned()])
            };
            victory_gpu::framegen::upscale_context(flags, max_render);
        }
        if ty == DESC_OVERRIDE_VERSION && !previous.is_null() {
            tracing::info!("FidelityFX: the title's version override is not passed to the host DLL");
            // SAFETY: the previous description's next-pointer word.
            unsafe { patches.set(previous.add(8).cast(), next as usize) };
        } else {
            previous = node;
        }
        last = node;
        node = next;
    }
    if !has_backend {
        tracing::info!("FidelityFX: no backend description from the title; adding the host device");
        // SAFETY: the last description's next-pointer word; `backend` lives
        // through the call.
        unsafe { patches.set(last.add(8).cast(), backend.as_mut_ptr() as usize) };
    }
    // SAFETY: the PC DLL's export, with a description the title wrote and
    // we patched.
    let code = unsafe { (api.create)(context, desc, std::ptr::null()) };
    drop(patches);
    tracing::info!(code, "ffxCreateContext on the host DLL");
    code
}

extern "system" fn ffxDestroyContext(context: *mut *mut c_void, _alloc: *const c_void) -> u32 {
    let Some(api) = api() else { return ERROR };
    // SAFETY: the PC DLL's export.
    unsafe { (api.destroy)(context, std::ptr::null()) }
}

/// A description with the title's device somewhere in its first words
/// (queries and configurations): the host's in its place.
fn with_host_device(call: &str, context: *mut *mut c_void, desc: *mut u8, f: impl FnOnce(&Api) -> Header) -> u32 {
    let Some(api) = api() else { return ERROR };
    note(call, desc);
    let mut patches = Patches::default();
    if let Some(device) = external::host_device() {
        let mut node = desc;
        for _ in 0..16 {
            if node.is_null() {
                break;
            }
            swap_device(&mut patches, node, 16, 96, device);
            // SAFETY: a title description's next-pointer word.
            node = unsafe { node.add(8).cast::<*mut u8>().read_unaligned() };
        }
    }
    // SAFETY: the PC DLL's export.
    unsafe { f(api)(context, desc) }
}

extern "system" fn ffxConfigure(context: *mut *mut c_void, desc: *mut u8) -> u32 {
    with_host_device("configure", context, desc, |api| api.configure)
}

/// (27/11) `ffxQueryDescUpscaleGetJitterPhaseCount` is answered here, from its render and display
/// widths (words at +16 and +20, the answer's pointer at +24), with AMD's formula: the title divides
/// by it (+0x28f03e8), and the PC DLL's answer came back 0 at a render scale of 0.5 (avatarmanz
/// run res-6; the same `0xc0000094` in justlovett0's Series X runs on Marvisak/VIctory#1793).
extern "system" fn ffxQuery(context: *mut *mut c_void, desc: *mut u8) -> u32 {
    // SAFETY: a description: its type first.
    if !desc.is_null() && unsafe { desc.cast::<u64>().read_unaligned() } == QUERY_JITTER_PHASE_COUNT {
        // SAFETY: the description's words, as `ffx_upscale.h` lays them out.
        unsafe {
            let render = desc.add(16).cast::<u32>().read_unaligned();
            let output = desc.add(20).cast::<u32>().read_unaligned();
            let out = desc.add(24).cast::<*mut i32>().read_unaligned();
            if out.is_null() {
                return ERROR_PARAMETER;
            }
            let count = jitter_phase_count(render, output);
            out.write_unaligned(count);
            static ONCE: std::sync::Once = std::sync::Once::new();
            ONCE.call_once(|| tracing::info!(render, output, count, "FidelityFX: jitter phase count answered here"));
        }
        return OK;
    }
    with_host_device("query", context, desc, |api| api.query)
}

/// AMD's jitter phase count (`ffxFsr3GetJitterPhaseCount`): 8 x (display / render)^2, never 0.
fn jitter_phase_count(render: u32, display: u32) -> i32 {
    if render == 0 {
        return 8;
    }
    let ratio = display as f32 / render as f32;
    ((8.0 * ratio * ratio) as i32).max(1)
}

/// `FfxApiResourceState` to the D3D12 resource state it means.
///
/// In the FidelityFX API's own layout (`ffx_api_types.h`, 1.1.0 to 2.4.0): COMMON 1 << 0,
/// UNORDERED_ACCESS 1 << 1, COMPUTE_READ 1 << 2, PIXEL_READ 1 << 3, COPY_SRC 1 << 4,
/// COPY_DEST 1 << 5, INDIRECT_ARGUMENT 1 << 6, PRESENT 1 << 7, RENDER_TARGET 1 << 8. This read
/// them one bit off, in the older SDK's layout: the title's colour (0xc, read by pixel and
/// compute shaders) went to PIXEL_SHADER_RESOURCE | COPY_SOURCE, a declared UNORDERED_ACCESS
/// to NON_PIXEL_SHADER_RESOURCE, and AMD's DLL recorded its barriers from states the textures
/// were not in (Series S path). `KLBF_VE_FFX_STATES_OLD=1`: the older layout, as before.
/// Based on Bl4Ck0uuT's Neptune-VE 2026-11-26c.
fn d3d12_state(ffx: u32) -> u32 {
    static OLD: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let old = *OLD.get_or_init(|| std::env::var_os("KLBF_VE_FFX_STATES_OLD").is_some_and(|v| v == "1"));
    d3d12_state_in(ffx, old)
}

/// [`d3d12_state`] in the API's layout, or (`old`) in the older SDK's.
fn d3d12_state_in(ffx: u32, old: bool) -> u32 {
    // (bit, D3D12 state); COMMON and PRESENT are D3D12's 0.
    const API: [(u32, u32); 7] =
        [(1 << 1, 0x8), (1 << 2, 0x40), (1 << 3, 0x80), (1 << 4, 0x800), (1 << 5, 0x400), (1 << 6, 0x200), (1 << 8, 0x4)];
    const OLDER: [(u32, u32); 7] =
        [(1 << 0, 0x8), (1 << 1, 0x40), (1 << 2, 0x80), (1 << 3, 0x800), (1 << 4, 0x400), (1 << 5, 0x200), (1 << 7, 0x4)];
    let table = if old { &OLDER } else { &API };
    table.iter().filter(|(bit, _)| ffx & bit != 0).fold(0, |state, (_, d3d)| state | d3d)
}

/// Where a dispatch description keeps its command list, and its resources
/// (each `FfxApiResource`: pointer, 32-byte description, state; 48 bytes)
/// with whether the effect writes them.
const RESOURCE_SIZE: usize = 48;
const RESOURCE_STATE: usize = 40;
const COMMAND_LIST: usize = 16;
const UPSCALE: [(usize, bool); 7] = [
    (24, false),
    (24 + RESOURCE_SIZE, false),
    (24 + 2 * RESOURCE_SIZE, false),
    (24 + 3 * RESOURCE_SIZE, false),
    (24 + 4 * RESOURCE_SIZE, false),
    (24 + 5 * RESOURCE_SIZE, false),
    (24 + 6 * RESOURCE_SIZE, true),
];
const REACTIVE_MASK: [(usize, bool); 3] = [
    (24, false),
    (24 + RESOURCE_SIZE, false),
    (24 + 2 * RESOURCE_SIZE, true),
];

/// `ffxDispatch(context, desc)`: the title's command list and textures
/// become the host's for the call.
extern "system" fn ffxDispatch(context: *mut *mut c_void, desc: *mut u8) -> u32 {
    let Some(api) = api() else { return ERROR };
    if desc.is_null() {
        return ERROR_PARAMETER;
    }
    note("dispatch", desc);
    // SAFETY: a description: its type first.
    let ty = unsafe { desc.cast::<u64>().read_unaligned() };
    let resources: &[(usize, bool)] = match ty {
        DISPATCH_UPSCALE => &UPSCALE,
        DISPATCH_GENERATE_REACTIVE_MASK => &REACTIVE_MASK,
        _ => {
            // SAFETY: the PC DLL's export; nothing here names console objects.
            return unsafe { (api.dispatch)(context, desc) };
        }
    };
    // SAFETY: the command list's word of a dispatch description.
    let list = unsafe { desc.add(COMMAND_LIST).cast::<usize>().read_unaligned() };
    let mut code = ERROR;
    let mut patches = Patches::default();
    let recorded = external::record(list as *const c_void, |pass| {
        // SAFETY: the words of a dispatch description the title passed,
        // aligned as the C structures are.
        unsafe {
            patches.set(desc.add(COMMAND_LIST).cast(), pass.command_list() as usize);
            for &(at, write) in resources {
                let slot = desc.add(at).cast::<usize>();
                let title = slot.read_unaligned();
                if title == 0 {
                    continue;
                }
                let declared = desc.add(at + RESOURCE_STATE).cast::<u32>().read_unaligned();
                match pass.texture(title as *const c_void, d3d12_state(declared), write) {
                    Some(host) => patches.set(slot, host as usize),
                    None => {
                        tracing::warn!(slot = at, resource = title, "FidelityFX dispatch: not a texture of the title");
                        code = ERROR_PARAMETER;
                        return;
                    }
                }
            }
            code = (api.dispatch)(context, desc);
            if code == OK && ty == DISPATCH_UPSCALE {
                // (KLBF Lab, 27/11) Its render and upscaled sizes (`ffxDispatchDescUpscale`'s words 4..8
                // from +360), for the bench card and the window's caption.
                let word = |i: usize| desc.add(360 + 4 * i).cast::<u32>().read_unaligned();
                victory_gpu::klbf_bench::note_upscale([word(4), word(5)], [word(6), word(7)]);
            }
            if code == OK && ty == DISPATCH_UPSCALE && victory_gpu::framegen::on() {
                prepare_framegen(pass.command_list(), desc);
            }
        }
    });
    drop(patches);
    if !recorded {
        tracing::warn!("FidelityFX dispatch: the command list is not one of the title's");
        return ERROR_PARAMETER;
    }
    if code != OK {
        tracing::warn!(code, "ffxDispatch on the host DLL");
    }
    code
}

/// (KLBF Lab, `KLBF_FRAMEGEN=1`) The frame generation prepare, on the upscale's host list right
/// after it: the depth and motion vectors it read (already the host textures, in the states the
/// upscale was told and leaves them in), its jitter, motion vector scale, render size, frame time,
/// reset and camera (`ffxDispatchDescUpscale`'s words from +360). After gleaf-6's.
///
/// # Safety
/// `desc` is the upscale description just dispatched, its resources patched to the host's.
unsafe fn prepare_framegen(list: *mut c_void, desc: *mut u8) {
    // SAFETY: as the caller says; `ffx_upscale.h`'s layout.
    unsafe {
        let resource = |at: usize| victory_gpu::framegen::Resource {
            resource: desc.add(at).cast::<*mut c_void>().read_unaligned(),
            description: desc.add(at + 8).cast::<[u32; 8]>().read_unaligned(),
            state: desc.add(at + RESOURCE_STATE).cast::<u32>().read_unaligned(),
        };
        let word = |i: usize| desc.add(360 + 4 * i).cast::<u32>().read_unaligned();
        let float = |i: usize| f32::from_bits(word(i));
        victory_gpu::framegen::prepare(
            list,
            resource(UPSCALE[1].0),
            resource(UPSCALE[2].0),
            [float(0), float(1)],
            [float(2), float(3)],
            [word(4), word(5)],
            float(10),
            desc.add(408).read() != 0,
            [float(13), float(14), float(15), float(16)],
        );
    }
}

pub(crate) fn module() -> HleModule {
    HleModule {
        name: "fidelityfx",
        dlls: &DLLS,
        exports: vec![
            crate::hle_export!(ffxCreateContext),
            crate::hle_export!(ffxDestroyContext),
            crate::hle_export!(ffxConfigure),
            crate::hle_export!(ffxQuery),
            crate::hle_export!(ffxDispatch),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_map_to_d3d12() {
        // The FidelityFX API's layout (`FfxApiResourceState`).
        assert_eq!(d3d12_state_in(0, false), 0);
        assert_eq!(d3d12_state_in(1, false), 0, "COMMON");
        assert_eq!(d3d12_state_in(1 << 1, false), 0x8, "UNORDERED_ACCESS");
        assert_eq!(d3d12_state_in(0xc, false), 0xc0, "PIXEL_COMPUTE_READ, the title's colour");
        assert_eq!(d3d12_state_in((1 << 4) | (1 << 2), false), 0x840, "GENERIC_READ");
        assert_eq!(d3d12_state_in(1 << 7, false), 0, "PRESENT");
        assert_eq!(d3d12_state_in(1 << 8, false), 0x4, "RENDER_TARGET");
        // The older SDK's layout (`KLBF_VE_FFX_STATES_OLD=1`), as before.
        assert_eq!(d3d12_state_in(1, true), 0x8);
        assert_eq!(d3d12_state_in((1 << 1) | (1 << 2), true), 0xc0);
        assert_eq!(d3d12_state_in((1 << 3) | (1 << 1), true), 0x840);
    }

    #[test]
    fn the_jitter_phase_count_is_never_zero() {
        assert_eq!(jitter_phase_count(1920, 3840), 32, "scale 0.5");
        assert_eq!(jitter_phase_count(960, 1920), 32, "Series S at 0.5, where the PC DLL said 0");
        assert_eq!(jitter_phase_count(1706, 2560), 18, "quality, 1440p");
        assert_eq!(jitter_phase_count(1280, 1920), 18, "Series S, 2/3");
        assert_eq!(jitter_phase_count(2560, 2560), 8, "native");
        assert_eq!(jitter_phase_count(0, 2560), 8);
        assert_eq!(jitter_phase_count(4000, 100), 1);
    }

    #[test]
    fn the_query_is_answered_here() {
        let mut count = 0i32;
        let mut desc = [0u8; 32];
        desc[..8].copy_from_slice(&QUERY_JITTER_PHASE_COUNT.to_le_bytes());
        desc[16..20].copy_from_slice(&1920u32.to_le_bytes());
        desc[20..24].copy_from_slice(&3840u32.to_le_bytes());
        desc[24..].copy_from_slice(&(&raw mut count as usize).to_le_bytes());
        assert_eq!(ffxQuery(std::ptr::null_mut(), desc.as_mut_ptr()), OK);
        assert_eq!(count, 32);
    }

    #[test]
    fn patches_are_put_back() {
        let mut words = [1usize, 2];
        {
            let mut p = Patches::default();
            // SAFETY: aligned words of a local array.
            unsafe {
                p.set(&mut words[0], 9);
                p.set(&mut words[0], 10);
                p.set(&mut words[1], 7);
            }
            assert_eq!(words, [10, 7]);
        }
        assert_eq!(words, [1, 2]);
    }
}
