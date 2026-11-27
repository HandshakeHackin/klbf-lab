//! (KLBF Lab, 27/11) AMD's frame generation on the host (`KLBF_FRAMEGEN=1`,
//! launcher `-FrameGen`, off by default; needs the FSR redirect,
//! `VICTORY_FSR_DLL`, whose folder must also hold AMD's signed
//! `amd_fidelityfx_loader_dx12.dll` and `amd_fidelityfx_framegeneration_dx12.dll`):
//! the window's swap chain is AMD's frame generation one ([`swap_chain`]), a
//! frame generation context is fed by the title's own upscale (`prepare`: its
//! depth, motion vectors, jitter and camera, at the render size the upscaler
//! was given), and each present is interpolated from the image before it and
//! the one it shows. Series S, or Series X with `KLBF_X_FSR=1`: without a
//! FidelityFX upscale nothing is prepared and frames are presented as they are.
//!
//! Ported from gleaf-6/VIctory-7900XTX- (`framegen.rs`, 252ccc10), itself from
//! MrSkelee/VIctory-MrSkele-AMD's `ffx.rs`, which started from our own
//! lab39-framegen (ef3f4f4). The interface is already in the image
//! (`crate::compose`), so no UI resource is registered: generated frames
//! interpolate it with the rest, and text can shimmer on fast motion.
//!
//! What it is worth: it does not make the title faster. The card shows two
//! frames per real one, the interpolation being work of its own on the same
//! card, and the input is as late as the real frames (a little later: one is
//! held back to interpolate towards). AMD recommends a base of 60 fps; at
//! about 16 real frames a second it adds latency (60+ ms) and artifacts
//! (ghosting, torn edges on fast motion and camera cuts).
//!
//! (27/11) FSR 4's frame generation is RX 9000 only: on any other card the
//! FSR 3.1 frame generation of the same DLL is picked by hand ([`fsr3_providers`]),
//! never none. F7 pauses it and starts it again ([`toggle`]): paused, each
//! present is the real frame only, so nothing (the interface included) is
//! interpolated. Both ideas from computermakemegta6 (BigCheese)'s
//! VIctory-BigCheese v2.1, written again here.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicU64, Ordering};
use std::sync::OnceLock;

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D12::{ID3D12CommandQueue, ID3D12Device};
use windows::Win32::Graphics::Dxgi::{
    DXGI_SWAP_CHAIN_DESC1, IDXGIFactory2, IDXGISwapChain3, IDXGISwapChain4,
};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{Interface, PCSTR, PCWSTR};

/// The API's types and constants (`ffx_api.h`, `ffx_framegeneration.h`, SDK
/// 2.4). The console's own names are the same: the title's DLLs are the same
/// API since FSR 3.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Header {
    kind: u64,
    next: *mut Header,
}

/// `FfxApiResource`: the resource, its description, its state.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Resource {
    pub resource: *mut std::ffi::c_void,
    pub description: [u32; 8],
    pub state: u32,
}
const _: () = assert!(std::mem::size_of::<Resource>() == 0x30);

impl Resource {
    /// A resource that is not there (a slot the title left empty).
    pub const NONE: Resource = Resource {
        resource: std::ptr::null_mut(),
        description: [0; 8],
        state: 0,
    };
}

const OK: u32 = 0;
const GET_VERSIONS: u64 = 4;
const OVERRIDE_VERSION: u64 = 5;
const GET_PROVIDER_VERSION: u64 = 6;
const CREATE_FRAMEGEN: u64 = 0x0002_0001;
const CONFIGURE_FRAMEGEN: u64 = 0x0002_0002;
const DISPATCH_FRAMEGEN_PREPARE_V2: u64 = 0x0002_000c;
const CREATE_SWAP_CHAIN_FOR_HWND: u64 = 0x0003_0006;
const DISPATCH_SWAP_CHAIN_WAIT_FOR_PRESENTS: u64 = 0x0003_0007;
const CREATE_SWAP_CHAIN_VERSION: u64 = 0x0003_000b;
const BACKEND_DX12: u64 = 2;
/// `FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION` of SDK 2.4 (4.0.0).
const SWAP_CHAIN_VERSION: u32 = 4 << 22;

/// `ffxCreateContextDescFrameGenerationSwapChainVersionDX12`.
#[repr(C)]
struct Version {
    header: Header,
    version: u32,
}

/// `ffxCreateContextDescFrameGenerationSwapChainForHwndDX12`.
#[repr(C)]
struct CreateSwapChainForHwnd {
    header: Header,
    swap_chain: *mut *mut std::ffi::c_void,
    hwnd: *mut std::ffi::c_void,
    desc: *const std::ffi::c_void,
    fullscreen: *const std::ffi::c_void,
    factory: *mut std::ffi::c_void,
    queue: *mut std::ffi::c_void,
}

/// `ffxCreateBackendDX12Desc`.
#[repr(C)]
struct BackendDx12 {
    header: Header,
    device: *mut std::ffi::c_void,
}

/// `ffxCreateContextDescFrameGeneration`.
#[repr(C)]
struct CreateFramegen {
    header: Header,
    flags: u32,
    display: [u32; 2],
    max_render: [u32; 2],
    format: u32,
}

type GenerateCallback = unsafe extern "C" fn(*mut Header, *mut std::ffi::c_void) -> u32;

/// `ffxConfigureDescFrameGeneration`.
#[repr(C)]
struct ConfigureFramegen {
    header: Header,
    swap_chain: *mut std::ffi::c_void,
    present_callback: *const std::ffi::c_void,
    present_context: *mut std::ffi::c_void,
    generate_callback: Option<GenerateCallback>,
    generate_context: *mut std::ffi::c_void,
    enabled: bool,
    async_workloads: bool,
    hudless: Resource,
    flags: u32,
    only_generated: bool,
    rect: [i32; 4],
    frame: u64,
}
const _: () = assert!(std::mem::size_of::<ConfigureFramegen>() == 0x90);

/// `ffxDispatchDescFrameGenerationPrepareV2`.
#[repr(C)]
struct PrepareV2 {
    header: Header,
    frame: u64,
    flags: u32,
    command_list: *mut std::ffi::c_void,
    render_size: [u32; 2],
    jitter: [f32; 2],
    motion_vector_scale: [f32; 2],
    frame_time_delta: f32,
    reset: bool,
    camera: [f32; 4],
    depth: Resource,
    motion_vectors: Resource,
    camera_vectors: [f32; 12],
}
const _: () = assert!(std::mem::size_of::<PrepareV2>() == 0xe8);

const NO_RESOURCE: Resource = Resource {
    resource: std::ptr::null_mut(),
    description: [0; 8],
    state: 0,
};

type Create = unsafe extern "C" fn(
    *mut *mut std::ffi::c_void,
    *mut Header,
    *const std::ffi::c_void,
) -> u32;
type Call = unsafe extern "C" fn(*mut *mut std::ffi::c_void, *const Header) -> u32;

struct Api {
    create: Create,
    configure: Call,
    dispatch: Call,
    destroy: Call,
    query: Call,
}

/// What the frame generation context is made with, learnt from the upscale
/// context and the swap chain.
struct Framegen {
    upscale_flags: u32,
    max_render: [u32; 2],
    display: [u32; 2],
    /// An `FfxApiSurfaceFormat`.
    format: u32,
}

static FRAMEGEN: std::sync::Mutex<Framegen> = std::sync::Mutex::new(Framegen {
    upscale_flags: 0,
    max_render: [0; 2],
    display: [0; 2],
    format: 0,
});
static SWAP_CHAIN_CONTEXT: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
static SWAP_CHAIN: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
static CONTEXT: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
/// (27/11) FSR 3.1's providers (swap chain, frame generation) picked by hand on a card without FSR 4's
/// frame generation; 0: the SDK's own pick.
static FSR3: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
/// (27/11) Which frame generation runs: 3 (FSR 3.1) or 4 (FSR 4); 0 before its context is made.
static KIND: AtomicU8 = AtomicU8::new(0);
/// (27/11) F7: paused (each present the real frame only); the first prepare after it starts again resets.
static PAUSED: AtomicBool = AtomicBool::new(false);
static RESUMED: AtomicBool = AtomicBool::new(false);
/// Set on the first failure: frame generation stays off for the run.
static FAILED: AtomicBool = AtomicBool::new(false);
/// The frame identifier of the next present.
static FRAME: AtomicU64 = AtomicU64::new(0);
/// The last frame prepared, plus one.
static PREPARED: AtomicU64 = AtomicU64::new(0);

/// The folder of the PC FSR DLL (`VICTORY_FSR_DLL`), where AMD's loader and
/// frame generation DLL are looked for.
fn dir() -> Option<std::path::PathBuf> {
    let dll = std::path::PathBuf::from(std::env::var_os("VICTORY_FSR_DLL").filter(|p| !p.is_empty())?);
    dll.parent().map(std::path::Path::to_path_buf)
}

/// Whether `KLBF_FRAMEGEN=1` asks for frame generation, with the PC FSR
/// redirect (`VICTORY_FSR_DLL`) to feed it.
pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let asked = std::env::var_os("KLBF_FRAMEGEN").is_some_and(|v| v == "1");
        let on = asked && dir().is_some();
        if asked && !on {
            tracing::warn!("frame generation off: KLBF_FRAMEGEN=1 needs VICTORY_FSR_DLL (AMD's signed FidelityFX DLLs)");
        } else if on {
            tracing::info!("frame generation asked (KLBF_FRAMEGEN=1; AMD recommends a base of 60 fps)");
        }
        on
    })
}

/// AMD's loader, with the upscaler and the frame generation build beside it
/// loaded by full path first (so the loader finds them without a search
/// path change). `None` (logged once) without them.
fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| {
        let dir = dir()?;
        let load = |name: &str| -> Option<windows::Win32::Foundation::HMODULE> {
            let path = std::path::absolute(dir.join(name)).ok()?;
            let mut wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().collect();
            wide.push(0);
            // SAFETY: a NUL-terminated full path; the SDK's own folder and
            // System32 only, never the current folder.
            unsafe {
                LoadLibraryExW(
                    PCWSTR(wide.as_ptr()),
                    None,
                    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
                )
            }
            .ok()
        };
        let _ = load("amd_fidelityfx_upscaler_dx12.dll");
        let _ = load("amd_fidelityfx_framegeneration_dx12.dll");
        let Some(loader) = load("amd_fidelityfx_loader_dx12.dll") else {
            tracing::warn!(dir = %dir.display(), "frame generation off: no amd_fidelityfx_loader_dx12.dll there");
            return None;
        };
        let get = |name: &[u8]| {
            // SAFETY: a loaded module and a NUL-terminated name.
            unsafe { GetProcAddress(loader, PCSTR(name.as_ptr())) }
        };
        // SAFETY: the SDK's exports, of these signatures (`ffx_api.h`).
        unsafe {
            Some(Api {
                create: std::mem::transmute::<_, Create>(get(b"ffxCreateContext\0")?),
                configure: std::mem::transmute::<_, Call>(get(b"ffxConfigure\0")?),
                dispatch: std::mem::transmute::<_, Call>(get(b"ffxDispatch\0")?),
                destroy: std::mem::transmute::<_, Call>(get(b"ffxDestroyContext\0")?),
                query: std::mem::transmute::<_, Call>(get(b"ffxQuery\0")?),
            })
        }
    })
    .as_ref()
}

/// (27/11) The frame generation provider the SDK picked (`ffxQueryGetProviderVersion`, type 5): FSR 3.1
/// frame generation outside RDNA 4, ML frame generation 4.0 on it. Logged once per context.
fn log_provider(api: &Api) {
    #[repr(C)]
    struct ProviderVersion {
        ty: u64,
        next: *mut std::ffi::c_void,
        id: u64,
        name: *const std::ffi::c_char,
    }
    let mut q = ProviderVersion { ty: GET_PROVIDER_VERSION, next: std::ptr::null_mut(), id: 0, name: std::ptr::null() };
    // SAFETY: the SDK's call on a context it made, with a description of ours that outlives it.
    let r = unsafe { (api.query)(CONTEXT.as_ptr(), (&raw mut q).cast::<Header>()) };
    if r == OK && !q.name.is_null() {
        // SAFETY: a NUL-terminated name the SDK keeps.
        let name = unsafe { std::ffi::CStr::from_ptr(q.name) }.to_string_lossy();
        tracing::info!("frame generation: {name}");
        KIND.store(kind_of(&name), Ordering::Relaxed);
    } else {
        tracing::info!(r, "frame generation: no provider version");
        if FSR3[1].load(Ordering::Relaxed) != 0 {
            KIND.store(3, Ordering::Relaxed);
        }
    }
}

/// 3 or 4 from a provider's version name ("3.1.5", "FSR 4.0.2"); 0 for anything else.
fn kind_of(name: &str) -> u8 {
    match name.chars().find(char::is_ascii_digit) {
        Some('3') => 3,
        Some('4') => 4,
        _ => 0,
    }
}

/// The providers the SDK offers for `effect` (a create description type), on `device` (null: its
/// built-in ones only): (id, version name). Empty if it cannot say.
fn versions(api: &Api, effect: u64, device: *mut std::ffi::c_void) -> Vec<(u64, String)> {
    #[repr(C)]
    struct GetVersions {
        header: Header,
        effect: u64,
        device: *mut std::ffi::c_void,
        count: *mut u64,
        ids: *mut u64,
        names: *mut *const std::ffi::c_char,
    }
    let mut count = 0u64;
    let mut q = GetVersions {
        header: header(GET_VERSIONS),
        effect,
        device,
        count: &raw mut count,
        ids: std::ptr::null_mut(),
        names: std::ptr::null_mut(),
    };
    // SAFETY: a null context asks for the versions; the count, then arrays of that many, outlive both calls.
    if unsafe { (api.query)(std::ptr::null_mut(), &raw const q.header) } != OK || !(1..=64).contains(&count) {
        return Vec::new();
    }
    let mut ids = vec![0u64; count as usize];
    let mut names: Vec<*const std::ffi::c_char> = vec![std::ptr::null(); count as usize];
    (q.ids, q.names) = (ids.as_mut_ptr(), names.as_mut_ptr());
    // SAFETY: as above, with room for `count` answers.
    if unsafe { (api.query)(std::ptr::null_mut(), &raw const q.header) } != OK {
        return Vec::new();
    }
    let n = (count as usize).min(ids.len());
    ids.into_iter()
        .zip(names)
        .take(n)
        .filter(|&(id, name)| id != 0 && !name.is_null())
        // SAFETY: NUL-terminated names the SDK keeps.
        .map(|(id, name)| (id, unsafe { std::ffi::CStr::from_ptr(name) }.to_string_lossy().into_owned()))
        .collect()
}

/// The first FSR 3 provider of the lists, when the card's own list (`on_card`) has no FSR 4 one: `None`
/// (the SDK's own pick) on RX 9000, or when the SDK could not say what the card offers.
fn fsr3_of(on_card: &[(u64, String)], built_in: &[(u64, String)]) -> Option<u64> {
    if on_card.is_empty() || on_card.iter().any(|v| kind_of(&v.1) == 4) {
        return None;
    }
    built_in.iter().chain(on_card).find(|v| kind_of(&v.1) == 3).map(|v| v.0)
}

/// (27/11) On a card without FSR 4's frame generation (before RX 9000): FSR 3.1's swap chain and frame
/// generation, picked by hand, so frame generation runs there too; noted in [`FSR3`].
fn fsr3_providers(api: &Api, device: *mut std::ffi::c_void) {
    let null = std::ptr::null_mut();
    let generator = fsr3_of(&versions(api, CREATE_FRAMEGEN, device), &versions(api, CREATE_FRAMEGEN, null));
    let swap_chain = generator.and_then(|_| {
        let built_in = versions(api, CREATE_SWAP_CHAIN_FOR_HWND, null);
        let on_card = versions(api, CREATE_SWAP_CHAIN_FOR_HWND, device);
        built_in.iter().chain(&on_card).find(|v| kind_of(&v.1) == 3).map(|v| v.0)
    });
    if let (Some(g), Some(s)) = (generator, swap_chain) {
        tracing::info!("frame generation: no FSR 4 frame generation on this card (RX 9000 only); FSR 3.1's instead");
        FSR3[0].store(s, Ordering::Relaxed);
        FSR3[1].store(g, Ordering::Relaxed);
    }
}

/// `ffxOverrideVersion`: the provider to use.
#[repr(C)]
struct Override {
    header: Header,
    id: u64,
}

/// (27/11) Which frame generation runs: 3 (FSR 3.1), 4 (FSR 4), 0 not yet known.
pub fn kind() -> u8 {
    KIND.load(Ordering::Relaxed)
}

/// (27/11) Whether F7 paused frame generation (never during a bench run: it runs as set).
pub fn paused() -> bool {
    PAUSED.load(Ordering::Relaxed) && !crate::klbf_bench::active()
}

/// (27/11) F7: frame generation paused, or started again (its history reset). `false` (the key is not
/// ours) when no frame generation swap chain presents.
pub fn toggle() -> bool {
    if SWAP_CHAIN.load(Ordering::Acquire).is_null() || FAILED.load(Ordering::Relaxed) {
        return false;
    }
    let paused = !PAUSED.fetch_xor(true, Ordering::Relaxed);
    if !paused {
        RESUMED.store(true, Ordering::Relaxed);
    }
    tracing::info!(paused, "F7: frame generation {}", if paused { "paused" } else { "on again" });
    true
}

/// The upscale context's flags and largest render size, for the frame
/// generation context (the motion vector and depth bits are at the same
/// positions in both; HDR is the back buffer's, not the upscale input's).
pub fn upscale_context(flags: u32, max_render: [u32; 2]) {
    if !on() {
        return;
    }
    let mut fg = FRAMEGEN.lock().unwrap_or_else(|e| e.into_inner());
    fg.upscale_flags = flags;
    fg.max_render = max_render;
}

/// The FfxApiSurfaceFormat of a swap chain format.
fn surface_format(dxgi: i32) -> u32 {
    match dxgi {
        28 => 10,
        87 => 14,
        24 => 17,
        10 => 4,
        _ => 0,
    }
}

fn header(kind: u64) -> Header {
    Header {
        kind,
        next: std::ptr::null_mut(),
    }
}

/// The frame generation swap chain for `hwnd`, made as `desc` says on
/// `queue`; `None` (one line logged, frame generation off for the run) if
/// the SDK cannot make it, and then the window's own is made as before.
pub fn swap_chain(
    hwnd: HWND,
    desc: &DXGI_SWAP_CHAIN_DESC1,
    factory: &IDXGIFactory2,
    queue: &ID3D12CommandQueue,
) -> Option<IDXGISwapChain3> {
    let api = api()?;
    if FAILED.load(Ordering::Relaxed) {
        return None;
    }
    // SAFETY: the queue's own device.
    if let Ok(device) = unsafe { queue.GetDevice::<ID3D12Device>() } {
        fsr3_providers(api, device.as_raw());
    }
    let mut out = std::ptr::null_mut();
    let mut version = Version {
        header: header(CREATE_SWAP_CHAIN_VERSION),
        version: SWAP_CHAIN_VERSION,
    };
    let mut fsr3 = Override {
        header: Header { kind: OVERRIDE_VERSION, next: (&raw mut version).cast() },
        id: FSR3[0].load(Ordering::Relaxed),
    };
    let mut create = CreateSwapChainForHwnd {
        header: Header {
            kind: CREATE_SWAP_CHAIN_FOR_HWND,
            next: if fsr3.id != 0 { (&raw mut fsr3).cast() } else { (&raw mut version).cast() },
        },
        swap_chain: &raw mut out,
        hwnd: hwnd.0,
        desc: std::ptr::from_ref(desc).cast(),
        fullscreen: std::ptr::null(),
        factory: factory.as_raw(),
        queue: queue.as_raw(),
    };
    // SAFETY: the SDK's call with a chain that outlives it.
    let r = unsafe {
        (api.create)(
            SWAP_CHAIN_CONTEXT.as_ptr(),
            (&raw mut create).cast(),
            std::ptr::null(),
        )
    };
    if r != OK || out.is_null() {
        tracing::warn!(
            r,
            "frame generation off: the SDK made no frame generation swap chain; presenting as before"
        );
        FAILED.store(true, Ordering::Relaxed);
        return None;
    }
    // SAFETY: the swap chain the SDK made, with a reference for us.
    let swap_chain = unsafe { IDXGISwapChain4::from_raw(out) };
    SWAP_CHAIN.store(out, Ordering::Release);
    tracing::info!("presenting through AMD's frame generation swap chain");
    swap_chain.cast().ok()
}

/// The swap chain's size and format, at creation and after each resize; a
/// frame generation context made for another is let go (made again at the
/// next prepare).
pub fn display(width: u32, height: u32, format: i32) {
    let mut fg = FRAMEGEN.lock().unwrap_or_else(|e| e.into_inner());
    let format = surface_format(format);
    if (fg.display, fg.format) == ([width, height], format) {
        return;
    }
    (fg.display, fg.format) = ([width, height], format);
    let Some(api) = api() else { return };
    if CONTEXT.load(Ordering::Acquire).is_null() {
        return;
    }
    configure(api, &fg, FRAME.load(Ordering::Acquire), false);
    let wait = header(DISPATCH_SWAP_CHAIN_WAIT_FOR_PRESENTS);
    // SAFETY: the SDK's calls on the contexts it made.
    unsafe {
        (api.dispatch)(
            SWAP_CHAIN_CONTEXT.as_ptr(),
            (&raw const wait).cast(),
        );
        (api.destroy)(CONTEXT.as_ptr(), std::ptr::null());
    }
    CONTEXT.store(std::ptr::null_mut(), Ordering::Release);
}

/// Records the frame generation prepare for the next present on the
/// upscale's host list, with the upscale's inputs (in the state it left
/// them in); the first upscale after a present only.
#[allow(clippy::too_many_arguments)]
pub fn prepare(
    list: *mut std::ffi::c_void,
    depth: Resource,
    motion_vectors: Resource,
    jitter: [f32; 2],
    motion_vector_scale: [f32; 2],
    render_size: [u32; 2],
    frame_time_delta: f32,
    reset: bool,
    camera: [f32; 4],
) {
    if FAILED.load(Ordering::Relaxed)
        || paused()
        || SWAP_CHAIN_CONTEXT.load(Ordering::Acquire).is_null()
        || depth.resource.is_null()
        || motion_vectors.resource.is_null()
    {
        return;
    }
    let frame = FRAME.load(Ordering::Acquire);
    if PREPARED.swap(frame + 1, Ordering::AcqRel) == frame + 1 {
        return;
    }
    let Some(api) = api() else { return };
    let fg = FRAMEGEN.lock().unwrap_or_else(|e| e.into_inner());
    if CONTEXT.load(Ordering::Acquire).is_null() && !create(api, &fg) {
        PREPARED.store(0, Ordering::Release);
        return;
    }
    let prepare = PrepareV2 {
        header: header(DISPATCH_FRAMEGEN_PREPARE_V2),
        frame,
        flags: 0,
        command_list: list,
        render_size,
        jitter,
        motion_vector_scale,
        frame_time_delta,
        // (27/11) After F7's pause, the frames it holds are stale.
        reset: reset || RESUMED.swap(false, Ordering::Relaxed),
        camera,
        depth,
        motion_vectors,
        // Camera position, up, right and forward: unknown here, left zero.
        camera_vectors: [0.0; 12],
    };
    // SAFETY: the SDK's call on an open host list with live textures.
    let r = unsafe { (api.dispatch)(CONTEXT.as_ptr(), (&raw const prepare).cast()) };
    if r != OK {
        PREPARED.store(0, Ordering::Release);
    }
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| {
        tracing::info!(
            frame,
            render = ?render_size,
            result = r,
            "the first frame generation prepare was recorded"
        );
    });
}

/// Makes the frame generation context; `false` if it cannot be made yet (no
/// swap chain size or upscale context known) or at all (logged once, frame
/// generation off for the run: the swap chain presents as is).
fn create(api: &Api, fg: &Framegen) -> bool {
    let Some(device) = crate::external::host_device() else {
        return false;
    };
    if fg.display.contains(&0) || fg.max_render.contains(&0) {
        return false;
    }
    let mut dx12 = BackendDx12 {
        header: header(BACKEND_DX12),
        device,
    };
    let mut fsr3 = Override {
        header: Header { kind: OVERRIDE_VERSION, next: (&raw mut dx12).cast() },
        id: FSR3[1].load(Ordering::Relaxed),
    };
    let mut create = CreateFramegen {
        header: Header {
            kind: CREATE_FRAMEGEN,
            next: if fsr3.id != 0 { (&raw mut fsr3).cast() } else { (&raw mut dx12).cast() },
        },
        flags: fg.upscale_flags & 0b1_1110,
        display: fg.display,
        max_render: fg.max_render,
        format: fg.format,
    };
    // SAFETY: the SDK's call with a device that outlives it.
    let r = unsafe {
        (api.create)(
            CONTEXT.as_ptr(),
            (&raw mut create).cast(),
            std::ptr::null(),
        )
    };
    if r != OK {
        tracing::warn!(
            r,
            display = ?fg.display,
            render = ?fg.max_render,
            "frame generation off: no frame generation context; presenting as before"
        );
        FAILED.store(true, Ordering::Relaxed);
        CONTEXT.store(std::ptr::null_mut(), Ordering::Release);
        return false;
    }
    log_provider(api);
    tracing::info!(
        display = ?fg.display,
        render = ?fg.max_render,
        flags = create.flags,
        format = fg.format,
        "frame generation context on the host GPU"
    );
    true
}

fn configure(api: &Api, fg: &Framegen, frame: u64, enabled: bool) {
    let c = ConfigureFramegen {
        header: header(CONFIGURE_FRAMEGEN),
        swap_chain: SWAP_CHAIN.load(Ordering::Acquire),
        present_callback: std::ptr::null(),
        present_context: std::ptr::null_mut(),
        generate_callback: Some(generate),
        generate_context: CONTEXT.as_ptr().cast(),
        enabled,
        async_workloads: false,
        hudless: NO_RESOURCE,
        flags: 0,
        only_generated: false,
        rect: [0, 0, fg.display[0] as i32, fg.display[1] as i32],
        frame,
    };
    // SAFETY: the SDK's call on the context it made.
    let r = unsafe { (api.configure)(CONTEXT.as_ptr(), (&raw const c).cast()) };
    if r != OK {
        static REPORTED: std::sync::Once = std::sync::Once::new();
        REPORTED.call_once(|| {
            tracing::warn!(r, "frame generation configuration refused")
        });
    }
}

/// The SDK's call back to generate a frame: its dispatch on our context.
unsafe extern "C" fn generate(params: *mut Header, context: *mut std::ffi::c_void) -> u32 {
    let Some(api) = api() else { return 1 };
    // SAFETY: the SDK's description, for the context it was configured with.
    unsafe { (api.dispatch)(context.cast(), params) }
}

/// Before a present through the frame generation swap chain: frame
/// generation is on for this frame if an upscale prepared it.
pub fn before_present() {
    let Some(api) = api() else { return };
    if CONTEXT.load(Ordering::Acquire).is_null() {
        return;
    }
    let fg = FRAMEGEN.lock().unwrap_or_else(|e| e.into_inner());
    let frame = FRAME.load(Ordering::Acquire);
    configure(api, &fg, frame, !paused() && PREPARED.load(Ordering::Acquire) == frame + 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fsr3_frame_generation_where_fsr4_is_not_offered() {
        let v = |list: &[(u64, &str)]| list.iter().map(|&(i, n)| (i, n.to_owned())).collect::<Vec<_>>();
        let built_in = v(&[(30, "3.1.5"), (31, "3.1.4")]);
        assert_eq!(fsr3_of(&v(&[(40, "4.0.2"), (30, "3.1.5")]), &built_in), None, "RX 9000: FSR 4's, the SDK's pick");
        assert_eq!(fsr3_of(&v(&[(30, "3.1.5")]), &built_in), Some(30), "RX 6000 / 7000: FSR 3.1's");
        assert_eq!(fsr3_of(&v(&[(30, "FSR 3.1.5")]), &[]), Some(30));
        assert_eq!(fsr3_of(&[], &built_in), None, "the SDK could not say: its own pick");
        assert_eq!((kind_of("4.0.2"), kind_of("FSR 3.1.6"), kind_of("")), (4, 3, 0));
    }
}

/// (27/11) Frames shown per second over the last second (real and generated), 0 before a second
/// was measured or when the swap chain gives no present count; the window's caption.
static SHOWN_FPS: AtomicU64 = AtomicU64::new(0);
/// (27/11) Frames shown per real frame since the start, 0 if not known: the bench card's
/// "(FG N)". ponytail: since the start, not since the run's start; the loading screens count too.
static SHOWN_RATIO: AtomicU64 = AtomicU64::new(0);

/// [`SHOWN_FPS`] (0: not known).
pub fn shown_fps() -> f64 {
    f64::from_bits(SHOWN_FPS.load(Ordering::Relaxed))
}

/// [`SHOWN_RATIO`] (0: not known).
pub fn shown_ratio() -> f64 {
    f64::from_bits(SHOWN_RATIO.load(Ordering::Relaxed))
}

/// After a present through the frame generation swap chain, given the swap
/// chain's count of presents (real and generated): once a second, the real
/// frame rate and the one shown.
pub fn after_present(presented: Option<u32>) {
    let real = FRAME.fetch_add(1, Ordering::AcqRel) + 1;
    static SINCE: std::sync::Mutex<Option<(std::time::Instant, u64, u32)>> =
        std::sync::Mutex::new(None);
    let mut since = SINCE.lock().unwrap_or_else(|e| e.into_inner());
    let presented = presented.unwrap_or(0);
    let Some((at, real_then, presented_then)) = *since else {
        *since = Some((std::time::Instant::now(), real, presented));
        return;
    };
    let elapsed = at.elapsed();
    if elapsed < std::time::Duration::from_secs(1) {
        return;
    }
    let seconds = elapsed.as_secs_f64();
    let shown = (presented.saturating_sub(presented_then)) as f64 / seconds;
    SHOWN_FPS.store(shown.to_bits(), Ordering::Relaxed);
    if presented > 0 {
        SHOWN_RATIO.store((f64::from(presented) / real as f64).to_bits(), Ordering::Relaxed);
    }
    tracing::info!(
        real = (real - real_then) as f64 / seconds,
        presented = shown,
        "frames rendered and frames shown (KLBF_FRAMEGEN=1: the extra ones are generated)"
    );
    *since = Some((std::time::Instant::now(), real, presented));
}
