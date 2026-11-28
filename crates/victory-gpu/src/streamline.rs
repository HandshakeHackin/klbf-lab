//! (KLBF Lab) NVIDIA DLSS through NVIDIA Streamline (SDK 2.14.16, MIT), loaded at run time from
//! `KLBF_STREAMLINE_DIR` (NVIDIA's release, fetched by the launcher and checked by SHA-256; nothing of
//! NVIDIA's is in this repository). Both off by default, both on top of the FSR redirect
//! (`VICTORY_FSR_DLL`, whose `ffxDispatch` they hook):
//!
//! - `KLBF_DLSS=1`: DLSS Super Resolution does the title's FidelityFX upscale in its place (same
//!   colour, depth, motion vectors, exposure, reactive mask and output, on the same host list). Any
//!   upscale DLSS cannot record goes to AMD FSR as before.
//! - `KLBF_DLSS_FG=2..6`: DLSS Frame Generation, frames shown per rendered one (2: RTX 40 and 50; 3 and
//!   more: Multi Frame Generation, RTX 50, never more than the driver says it makes). The upscale's
//!   depth and motion vectors are tagged for the present; the direct queue and the swap chain are
//!   made through Streamline's proxies, Reflex low latency is on (frame generation needs it).
//!
//! [`check_at_start`] loads the library and asks the card; anything missing (the folder, NVIDIA's
//! signature, the feature on this card or driver, the device hand-over) turns the switch off for the
//! run, said in the log, and the run is as without it.
//!
//! Ported from readmet3xt-lab/foxhound (GPL-3.0): `abi.rs` and `plan.rs` as they are, and here its
//! library loading, signature check, `slInit`, device hand-over, DLSS evaluation and present
//! markers, cut down to our single present thread. The 2x to 6x count bounded by the driver's
//! `numFramesToGenerateMax`: Kemerd/VIctory-NVIDIA-RT and trustno1-coder/VIctory-NVIDIA. Streamline is
//! used with manual hooking: only the objects handed to `slUpgradeInterface` are proxies.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

pub(crate) mod abi;
pub mod plan;

use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::*};
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::Interface;

const INTERPOSER: &str = "sl.interposer.dll";
/// The project id NGX is given (engine "custom", no NVIDIA application id).
const PROJECT_ID: &CStr = c"6f0d7b2e-3a41-4c1e-9b57-2d8e4f1a9c30";
const ENGINE_VERSION: &CStr = c"KLBF Lab";
/// The viewport of the upscale; frame generation wants 0.
const VIEWPORT_DLSS: u32 = 1;
const VIEWPORT_FG: u32 = 0;

/// The upscale's resources in `ffxDispatchDescUpscale`'s order.
pub const COLOUR: usize = 0;
pub const DEPTH: usize = 1;
pub const MOTION: usize = 2;
pub const EXPOSURE: usize = 3;
pub const REACTIVE: usize = 4;
pub const OUTPUT: usize = 6;

struct Sl {
    init: abi::SlInit,
    is_feature_supported: abi::SlIsFeatureSupported,
    evaluate_feature: abi::SlEvaluateFeature,
    set_tag_for_frame: abi::SlSetTagForFrame,
    get_feature_requirements: abi::SlGetFeatureRequirements,
    get_feature_version: abi::SlGetFeatureVersion,
    upgrade_interface: abi::SlUpgradeInterface,
    set_constants: abi::SlSetConstants,
    get_native_interface: abi::SlGetNativeInterface,
    get_feature_function: abi::SlGetFeatureFunction,
    get_new_frame_token: abi::SlGetNewFrameToken,
    set_d3d_device: abi::SlSetD3DDevice,
}

static SL: OnceLock<Sl> = OnceLock::new();

#[derive(Default)]
struct Functions {
    dlss_optimal: Option<abi::SlDlssGetOptimalSettings>,
    dlss_set_options: Option<abi::SlDlssSetOptions>,
    dlssg_set_options: Option<abi::SlDlssgSetOptions>,
    dlssg_get_state: Option<abi::SlDlssgGetState>,
    reflex_set_options: Option<abi::SlReflexSetOptions>,
    pcl_set_marker: Option<abi::SlPclSetMarker>,
}

static FUNCTIONS: OnceLock<Functions> = OnceLock::new();
static DLSS_READY: AtomicBool = AtomicBool::new(false);
static FG_READY: AtomicBool = AtomicBool::new(false);
/// Our swap chain is Streamline's proxy: frame generation can run on its presents.
static SWAP_CHAIN_PROXIED: AtomicBool = AtomicBool::new(false);
/// Frame generation is switched on (the last `slDLSSGSetOptions` that took).
static FG_ON: AtomicBool = AtomicBool::new(false);
/// The present counter: every frame token's index.
static FRAME: AtomicU64 = AtomicU64::new(0);
/// The last frame whose depth and motion vectors were tagged, plus one.
static TAGGED: AtomicU64 = AtomicU64::new(0);
/// The title's upscaler context flags (`FFX_UPSCALE_ENABLE_*`).
static FLAGS: AtomicU32 = AtomicU32::new(0);
/// Frames shown per rendered one, as switched on last (0: off).
static SHOWN: AtomicU32 = AtomicU32::new(0);

/// The direct queue made through Streamline's device proxy, for the swap chain.
struct Proxies {
    _device: ID3D12Device,
    queue: ID3D12CommandQueue,
}
// SAFETY: D3D12 objects are free-threaded.
unsafe impl Send for Proxies {}
// SAFETY: as above.
unsafe impl Sync for Proxies {}
static PROXIES: OnceLock<Proxies> = OnceLock::new();

/// Optimal render sizes by output and mode (the library's call is not thread safe).
static OPTIMAL: Mutex<Vec<([u32; 2], u32, Option<[u32; 2]>)>> = Mutex::new(Vec::new());

fn on(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| v == "1")
}

/// `KLBF_DLSS_FG`: frames shown per rendered one, 2 to 6; 0 (off) otherwise.
fn fg_asked() -> u32 {
    std::env::var("KLBF_DLSS_FG").ok().and_then(|v| v.trim().parse().ok()).filter(|n| (2..=6).contains(n)).unwrap_or(0)
}

/// Frames to generate for `shown` frames per rendered one, no more than the driver's `most`.
fn frames_to_generate(shown: u32, most: u32) -> u32 {
    shown.saturating_sub(1).clamp(1, most.max(1))
}

fn off(names: &[&str], why: &str) {
    tracing::warn!("NVIDIA DLSS off for this run: {why}");
    println!("streamline     NVIDIA DLSS off for this run ({why})");
    // SAFETY: at the run's start, before other threads read the environment.
    unsafe {
        for n in names {
            std::env::remove_var(n);
        }
    }
}

/// Before the title starts and before the host device is made, after the FSR check (DLSS hooks the
/// FSR redirect): Streamline is loaded and asked what this card runs. What cannot run is switched off
/// for the run (`KLBF_DLSS`, `KLBF_DLSS_FG`); with DLSS frame generation on, AMD's (`KLBF_FRAMEGEN`) is.
pub fn check_at_start() {
    let (dlss, fg) = (on("KLBF_DLSS"), fg_asked() > 0);
    if !dlss && !fg {
        return;
    }
    let all = ["KLBF_DLSS", "KLBF_DLSS_FG"];
    if std::env::var_os("VICTORY_FSR_DLL").is_none_or(|v| v.is_empty()) {
        return off(&all, "it hooks the AMD FSR path, which is off");
    }
    let Some(dir) = std::env::var_os("KLBF_STREAMLINE_DIR").filter(|v| !v.is_empty()).map(PathBuf::from) else {
        return off(&all, "no KLBF_STREAMLINE_DIR");
    };
    let sl = match load_and_init(&dir, dlss, fg) {
        Ok(sl) => sl,
        Err(why) => return off(&all, &why),
    };
    // The card the host device is made on (`host::high_performance_adapter`).
    // SAFETY: a plain DXGI query.
    let Some(luid) = crate::host::high_performance_adapter().and_then(|a| unsafe { a.GetDesc1() }.ok()).map(|d| d.AdapterLuid) else {
        return off(&all, "no adapter");
    };
    let supported = |feature: u32| -> Result<(), String> {
        let info = abi::AdapterInfo {
            base: abi::BaseStructure::new(abi::ADAPTER_INFO, 1),
            device_luid: std::ptr::from_ref(&luid).cast(),
            device_luid_size: size_of_val(&luid) as u32,
            vk_physical_device: std::ptr::null_mut(),
        };
        // SAFETY: an adapter description that lives through the call.
        let code = unsafe { (sl.is_feature_supported)(feature, &info) };
        if code == 0 { Ok(()) } else { Err(format!("{} ({code})", abi::result_name(code))) }
    };
    if dlss && !report("DLSS Super Resolution", abi::FEATURE_DLSS, supported(abi::FEATURE_DLSS), sl) {
        off(&["KLBF_DLSS"], "DLSS Super Resolution is not supported on this card or driver");
    }
    if fg {
        let ok = report("DLSS Frame Generation", abi::FEATURE_DLSS_G, supported(abi::FEATURE_DLSS_G), sl)
            && report("Reflex", abi::FEATURE_REFLEX, supported(abi::FEATURE_REFLEX), sl);
        if ok {
            // SAFETY: at the run's start, as above. One frame generation at a time.
            unsafe { std::env::remove_var("KLBF_FRAMEGEN") };
        } else {
            off(&["KLBF_DLSS_FG"], "DLSS Frame Generation is not supported on this card or driver (RTX 40 or later)");
        }
    }
    tracing::info!(folder = %dir.display(), dlss = on("KLBF_DLSS"), frame_generation = fg_asked(), "Streamline 2.14.16 loaded (manual hooking)");
}

fn load_and_init(dir: &Path, dlss: bool, fg: bool) -> Result<&'static Sl, String> {
    let path = std::path::absolute(dir.join(INTERPOSER)).map_err(|e| e.to_string())?;
    if !path.is_file() {
        return Err(format!("no {INTERPOSER} in {}", dir.display()));
    }
    verify_signature(&path)?;
    let sl = match SL.get() {
        Some(sl) => sl,
        None => {
            let loaded = load(&path)?;
            SL.get_or_init(|| loaded)
        }
    };
    let dir_wide = wide(dir);
    let paths = [dir_wide.as_ptr()];
    let mut features = Vec::new();
    if dlss {
        features.push(abi::FEATURE_DLSS);
    }
    if fg {
        features.extend([abi::FEATURE_DLSS_G, abi::FEATURE_REFLEX, abi::FEATURE_PCL]);
    }
    let preferences = abi::Preferences {
        base: abi::BaseStructure::new(abi::PREFERENCES, 1),
        show_console: false,
        log_level: abi::LOG_DEFAULT,
        paths_to_plugins: paths.as_ptr(),
        num_paths_to_plugins: 1,
        path_to_logs_and_data: std::ptr::null(),
        allocate_callback: std::ptr::null(),
        release_callback: std::ptr::null(),
        log_message_callback: Some(log_message),
        // No downloads (OTA): only what is in the folder runs.
        flags: abi::DISABLE_CL_STATE_TRACKING
            | abi::DISABLE_DEBUG_TEXT
            | abi::USE_MANUAL_HOOKING
            | abi::USE_DXGI_FACTORY_PROXY
            | abi::USE_FRAME_BASED_RESOURCE_TAGGING,
        features_to_load: features.as_ptr(),
        num_features_to_load: features.len() as u32,
        application_id: 0,
        engine: abi::ENGINE_CUSTOM,
        engine_version: ENGINE_VERSION.as_ptr(),
        project_id: PROJECT_ID.as_ptr(),
        render_api: abi::RENDER_API_D3D12,
    };
    // SAFETY: a filled preferences structure; the library copies what it keeps.
    let code = unsafe { (sl.init)(&preferences, abi::SDK_VERSION) };
    if code != 0 {
        return Err(format!("slInit failed: {} ({code})", abi::result_name(code)));
    }
    Ok(sl)
}

/// Logs whether a feature is supported, with its requirements; whether it is.
fn report(name: &str, feature: u32, supported: Result<(), String>, sl: &Sl) -> bool {
    // SAFETY: plain data; pointers null, counts zero.
    let mut requirements: abi::FeatureRequirements = unsafe { std::mem::zeroed() };
    requirements.base = abi::BaseStructure::new(abi::FEATURE_REQUIREMENTS, 2);
    // SAFETY: a structure to fill.
    let asked = unsafe { (sl.get_feature_requirements)(feature, &mut requirements) };
    let needs = (asked == 0).then(|| {
        format!(
            "OS {} (needs {}), driver {} (needs {}){}",
            requirements.os_version_detected,
            requirements.os_version_required,
            requirements.driver_version_detected,
            requirements.driver_version_required,
            if requirements.flags & abi::REQUIRES_HARDWARE_SCHEDULING != 0 { ", hardware GPU scheduling" } else { "" },
        )
    });
    match supported {
        Ok(()) => {
            tracing::info!(requirements = needs.as_deref().unwrap_or("?"), "Streamline: {name} is supported on this card");
            true
        }
        Err(why) => {
            tracing::warn!(requirements = needs.as_deref().unwrap_or("?"), "Streamline: {name} is not supported here: {why}");
            false
        }
    }
}

unsafe extern "C" fn log_message(kind: u32, message: *const c_char) {
    if message.is_null() {
        return;
    }
    // SAFETY: a NUL-terminated message for the call.
    let text = unsafe { CStr::from_ptr(message) }.to_string_lossy();
    let text = text.trim_end();
    match kind {
        abi::LOG_ERROR => tracing::error!(target: "streamline", "{text}"),
        abi::LOG_WARN if !text.contains("was not requested by the host") => tracing::warn!(target: "streamline", "{text}"),
        _ => tracing::debug!(target: "streamline", "{text}"),
    }
}

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain([0]).collect()
}

/// The Authenticode signature of `path` (NVIDIA signs its DLLs), checked without going online.
fn verify_signature(path: &Path) -> Result<(), String> {
    use windows_sys::Win32::Security::WinTrust::*;
    let file = wide(path);
    let mut info = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: file.as_ptr(),
        hFile: std::ptr::null_mut(),
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut info },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_NONE,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: filled structures that live through both calls.
    let code = unsafe { WinVerifyTrust(std::ptr::null_mut(), &mut action, (&raw mut data).cast()) };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: as above; closes the state the first call opened.
    unsafe { WinVerifyTrust(std::ptr::null_mut(), &mut action, (&raw mut data).cast()) };
    if code == 0 { Ok(()) } else { Err(format!("{} is not validly signed ({:#x})", path.display(), code as u32)) }
}

fn load(path: &Path) -> Result<Sl, String> {
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
    };
    let file = wide(path);
    // Its own folder and System32 only, never the current folder. Kept loaded for the run.
    // SAFETY: a NUL-terminated full path.
    let module = unsafe { LoadLibraryExW(file.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if module.is_null() {
        return Err(format!("{INTERPOSER} did not load ({})", std::io::Error::last_os_error()));
    }
    let find = |name: &CStr| {
        // SAFETY: a loaded module and a NUL-terminated name.
        unsafe { GetProcAddress(module, name.as_ptr().cast()) }
            .map(|f| f as *const c_void)
            .ok_or_else(|| format!("{INTERPOSER} has no {}", name.to_string_lossy()))
    };
    macro_rules! get {
        ($name:literal, $t:ty) => {
            // SAFETY: the export of that name has the header's signature.
            unsafe { std::mem::transmute::<*const c_void, $t>(find($name)?) }
        };
    }
    Ok(Sl {
        init: get!(c"slInit", abi::SlInit),
        is_feature_supported: get!(c"slIsFeatureSupported", abi::SlIsFeatureSupported),
        evaluate_feature: get!(c"slEvaluateFeature", abi::SlEvaluateFeature),
        set_tag_for_frame: get!(c"slSetTagForFrame", abi::SlSetTagForFrame),
        get_feature_requirements: get!(c"slGetFeatureRequirements", abi::SlGetFeatureRequirements),
        get_feature_version: get!(c"slGetFeatureVersion", abi::SlGetFeatureVersion),
        upgrade_interface: get!(c"slUpgradeInterface", abi::SlUpgradeInterface),
        set_constants: get!(c"slSetConstants", abi::SlSetConstants),
        get_native_interface: get!(c"slGetNativeInterface", abi::SlGetNativeInterface),
        get_feature_function: get!(c"slGetFeatureFunction", abi::SlGetFeatureFunction),
        get_new_frame_token: get!(c"slGetNewFrameToken", abi::SlGetNewFrameToken),
        set_d3d_device: get!(c"slSetD3DDevice", abi::SlSetD3DDevice),
    })
}

/// The library, when [`check_at_start`] left something on.
fn sl() -> Option<&'static Sl> {
    SL.get().filter(|_| on("KLBF_DLSS") || fg_asked() > 0)
}

/// The host device is made: Streamline is given it (`slSetD3DDevice`) and its feature functions are
/// fetched; for frame generation the direct queue is made through Streamline's device proxy. The
/// native queue to use for everything, if it was made here.
pub(crate) fn device_made(device: &ID3D12Device) -> Option<ID3D12CommandQueue> {
    let sl = sl()?;
    // SAFETY: the native device.
    let code = unsafe { (sl.set_d3d_device)(device.as_raw()) };
    if code != 0 {
        tracing::warn!("Streamline: slSetD3DDevice failed: {} ({code}); NVIDIA DLSS stays off", abi::result_name(code));
        return None;
    }
    let function = |feature: u32, name: &CStr| -> Option<*mut c_void> {
        let mut f = std::ptr::null_mut();
        // SAFETY: a NUL-terminated name and an out pointer.
        let code = unsafe { (sl.get_feature_function)(feature, name.as_ptr(), &mut f) };
        (code == 0 && !f.is_null()).then_some(f)
    };
    macro_rules! f {
        ($feature:expr, $name:literal, $t:ty) => {
            // SAFETY: the function of that name has the header's signature.
            function($feature, $name).map(|f| unsafe { std::mem::transmute::<*mut c_void, $t>(f) })
        };
    }
    let mut functions = Functions::default();
    if on("KLBF_DLSS") {
        functions.dlss_optimal = f!(abi::FEATURE_DLSS, c"slDLSSGetOptimalSettings", abi::SlDlssGetOptimalSettings);
        functions.dlss_set_options = f!(abi::FEATURE_DLSS, c"slDLSSSetOptions", abi::SlDlssSetOptions);
        log_version(sl, abi::FEATURE_DLSS, "DLSS Super Resolution");
        DLSS_READY.store(functions.dlss_optimal.is_some() && functions.dlss_set_options.is_some(), Release);
    }
    let mut queue = None;
    if fg_asked() > 0 {
        functions.dlssg_set_options = f!(abi::FEATURE_DLSS_G, c"slDLSSGSetOptions", abi::SlDlssgSetOptions);
        functions.dlssg_get_state = f!(abi::FEATURE_DLSS_G, c"slDLSSGGetState", abi::SlDlssgGetState);
        functions.reflex_set_options = f!(abi::FEATURE_REFLEX, c"slReflexSetOptions", abi::SlReflexSetOptions);
        functions.pcl_set_marker = f!(abi::FEATURE_PCL, c"slPCLSetMarker", abi::SlPclSetMarker);
        log_version(sl, abi::FEATURE_DLSS_G, "DLSS Frame Generation");
        if let (Some(_), Some(_), Some(reflex), Some(_)) =
            (functions.dlssg_set_options, functions.dlssg_get_state, functions.reflex_set_options, functions.pcl_set_marker)
        {
            let options = abi::ReflexOptions {
                base: abi::BaseStructure::new(abi::REFLEX_OPTIONS, 1),
                mode: abi::REFLEX_LOW_LATENCY,
                frame_limit_us: 0,
                use_markers_to_optimize: false,
                virtual_key: 0,
                id_thread: 0,
            };
            // SAFETY: filled options.
            let code = unsafe { reflex(&options) };
            tracing::info!(code, result = abi::result_name(code), "Streamline: Reflex low latency on (frame generation needs it)");
            queue = proxied_queue(sl, device);
        }
        FG_READY.store(queue.is_some(), Release);
        if queue.is_none() {
            tracing::warn!("DLSS Frame Generation stays off: the device hand-over to Streamline did not complete");
        }
    }
    let _ = FUNCTIONS.set(functions);
    queue
}

fn log_version(sl: &Sl, feature: u32, name: &str) {
    let mut version = abi::FeatureVersion {
        base: abi::BaseStructure::new(abi::FEATURE_VERSION, 1),
        version_sl: abi::Version::default(),
        version_ngx: abi::Version::default(),
    };
    // SAFETY: a structure to fill.
    if unsafe { (sl.get_feature_version)(feature, &mut version) } == 0 {
        tracing::info!(streamline = %version.version_sl, ngx = %version.version_ngx, "Streamline: {name} loaded");
    }
}

/// The direct queue, made through Streamline's proxy of `device`; the native queue is returned for
/// our use and the proxy kept for the swap chain.
fn proxied_queue(sl: &Sl, device: &ID3D12Device) -> Option<ID3D12CommandQueue> {
    let mut raw = device.as_raw();
    // SAFETY: an owned reference of ours; on success `raw` is a new proxy with its own reference.
    let code = unsafe { (sl.upgrade_interface)(&mut raw) };
    if code != 0 || raw.is_null() || raw == device.as_raw() {
        tracing::warn!("Streamline: the device was not upgraded: {} ({code})", abi::result_name(code));
        return None;
    }
    // SAFETY: the proxy's one reference, now ours.
    let proxy_device = unsafe { ID3D12Device::from_raw(raw) };
    let desc = D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() };
    // SAFETY: plain queue creation through the proxy.
    let proxy_queue: ID3D12CommandQueue = unsafe { proxy_device.CreateCommandQueue(&desc) }.ok()?;
    let mut native = std::ptr::null_mut();
    // SAFETY: a proxy queue; the native one comes back with a reference.
    let code = unsafe { (sl.get_native_interface)(proxy_queue.as_raw(), &mut native) };
    if code != 0 || native.is_null() {
        tracing::warn!("Streamline: no native queue behind the proxy: {} ({code})", abi::result_name(code));
        return None;
    }
    // SAFETY: the reference slGetNativeInterface added.
    let native = unsafe { ID3D12CommandQueue::from_raw(native) };
    tracing::info!("Streamline: the direct queue was made through the device proxy (frame generation)");
    let _ = PROXIES.set(Proxies { _device: proxy_device, queue: proxy_queue });
    Some(native)
}

/// For frame generation: our swap chain made through Streamline's proxies of `factory` and of the
/// queue; `None` (frame generation off for the run) to make it as before.
pub(crate) fn swap_chain(hwnd: HWND, desc: &DXGI_SWAP_CHAIN_DESC1, factory: &IDXGIFactory2) -> Option<IDXGISwapChain3> {
    if !FG_READY.load(Acquire) {
        return None;
    }
    let (sl, queue) = (sl()?, &PROXIES.get()?.queue);
    let mut raw = factory.as_raw();
    // SAFETY: as for the device: a new proxy with its own reference.
    let code = unsafe { (sl.upgrade_interface)(&mut raw) };
    let made = (code == 0 && !raw.is_null() && raw != factory.as_raw())
        .then(|| {
            // SAFETY: the proxy's reference, now ours.
            let proxy = unsafe { IDXGIFactory2::from_raw(raw) };
            // SAFETY: plain swap chain creation through the proxies.
            unsafe { proxy.CreateSwapChainForHwnd(queue, hwnd, desc, None, None) }.and_then(|s| s.cast::<IDXGISwapChain3>()).ok()
        })
        .flatten();
    SWAP_CHAIN_PROXIED.store(made.is_some(), Release);
    if made.is_some() {
        tracing::info!("Streamline: our swap chain is Streamline's proxy; DLSS Frame Generation can run on its presents");
    } else {
        FG_READY.store(false, Release);
        tracing::warn!(code, "Streamline: no swap chain through the proxies; made as before, DLSS Frame Generation off");
    }
    made
}

/// The title's upscaler context was made with `flags` (`FFX_UPSCALE_ENABLE_*`).
pub fn context_flags(flags: u32) {
    FLAGS.store(flags, Relaxed);
}

/// What an upscale dispatch says of the camera (`ffxDispatchDescUpscale`'s words from +360: jitter,
/// motion vector scale, render and upscale sizes, then near, far and vertical field of view).
pub fn camera(jitter: [f32; 2], motion_scale: [f32; 2], render: [u32; 2], output: [u32; 2], near: f32, far: f32, fov_y: f32, reset: bool) -> plan::Camera {
    plan::Camera { jitter, motion_scale, render, output, near, far, fov_y, reset, flags: FLAGS.load(Relaxed) }
}

fn frame_token(sl: &Sl, index: u32) -> Option<*mut abi::FrameToken> {
    let mut token = std::ptr::null_mut();
    // SAFETY: an out pointer and an index.
    let code = unsafe { (sl.get_new_frame_token)(&mut token, &index) };
    (code == 0 && !token.is_null()).then_some(token)
}

/// Whether DLSS Super Resolution takes the upscales.
pub fn upscaling() -> bool {
    DLSS_READY.load(Acquire)
}

/// `KLBF_DLSS=1`: DLSS records the upscale into the open host list `list`, from `textures` (the host
/// textures of the dispatch, in its order, each with the D3D12 state it is in; null where the title
/// gave none). `false` (said once): nothing recorded, AMD FSR is to do it.
pub fn upscale(list: *mut c_void, textures: &[(*mut c_void, u32); 7], camera: &plan::Camera, pre_exposure: f32) -> bool {
    if !upscaling() {
        return false;
    }
    match record(list, textures, camera, pre_exposure) {
        Ok(mode) => {
            static FIRST: std::sync::Once = std::sync::Once::new();
            FIRST.call_once(|| tracing::info!(render = ?camera.render, output = ?camera.output, mode = plan::sl_mode_name(mode), "DLSS: upscale recorded in place of AMD FSR"));
            true
        }
        Err(why) => {
            static ONCE: std::sync::Once = std::sync::Once::new();
            ONCE.call_once(|| tracing::warn!("DLSS: {why}; AMD FSR does this upscale"));
            false
        }
    }
}

fn record(list: *mut c_void, textures: &[(*mut c_void, u32); 7], camera: &plan::Camera, pre_exposure: f32) -> Result<u32, String> {
    let (Some(sl), Some(functions)) = (sl(), FUNCTIONS.get()) else { return Err("Streamline is not ready".into()) };
    let (Some(optimal_of), Some(set_options)) = (functions.dlss_optimal, functions.dlss_set_options) else {
        return Err("no DLSS functions".into());
    };
    for i in [COLOUR, DEPTH, MOTION, OUTPUT] {
        if textures[i].0.is_null() {
            return Err(format!("the dispatch has no texture {i}"));
        }
    }
    let output = camera.output;
    let preset = 0; // the driver's own model for every mode
    let mode = {
        let mut cache = OPTIMAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut optimal = |mode: u32| -> Option<[u32; 2]> {
            if let Some(&(_, _, size)) = cache.iter().find(|(o, m, _)| *o == output && *m == mode) {
                return size;
            }
            let options = abi::DlssOptions::new(mode, output, preset);
            let mut settings = abi::DlssOptimalSettings::default();
            // SAFETY: filled options and a structure to fill.
            let code = unsafe { optimal_of(&options, &mut settings) };
            let size = (code == 0 && settings.optimal_render_width > 0).then_some([settings.optimal_render_width, settings.optimal_render_height]);
            cache.push((output, mode, size));
            size
        };
        let sizes: Vec<(u32, Option<[u32; 2]>)> = plan::MODES.iter().map(|&m| (m, optimal(m))).collect();
        plan::pick_mode(camera.render, output, |m| sizes.iter().find(|s| s.0 == m).and_then(|s| s.1))
            .ok_or_else(|| format!("no DLSS mode takes a {:?} render for a {output:?} output", camera.render))?
    };
    let auto_exposure = camera.flags & plan::FFX_AUTO_EXPOSURE != 0 || textures[EXPOSURE].0.is_null();
    let mut options = abi::DlssOptions::new(mode, output, preset);
    options.pre_exposure = if pre_exposure > 0.0 { pre_exposure } else { 1.0 };
    options.color_buffers_hdr = abi::boolean(camera.flags & plan::FFX_HIGH_DYNAMIC_RANGE != 0);
    options.use_auto_exposure = abi::boolean(auto_exposure);
    let viewport = abi::ViewportHandle::new(VIEWPORT_DLSS);
    // SAFETY: filled options for our viewport.
    let code = unsafe { set_options(&viewport, &options) };
    if code != 0 {
        return Err(format!("slDLSSSetOptions: {} ({code})", abi::result_name(code)));
    }
    let token = frame_token(sl, FRAME.load(Acquire) as u32).ok_or("no frame token")?;
    let constants = camera.constants([1.0, 1.0], [1.0, 1.0]);
    // SAFETY: constants for this frame and viewport.
    let code = unsafe { (sl.set_constants)(&constants, token, &viewport) };
    if code != 0 {
        return Err(format!("slSetConstants: {} ({code})", abi::result_name(code)));
    }
    let kinds = [
        (COLOUR, abi::BUFFER_SCALING_INPUT_COLOR, camera.render),
        (DEPTH, abi::BUFFER_DEPTH, camera.render),
        (MOTION, abi::BUFFER_MOTION_VECTORS, camera.motion_size()),
        (EXPOSURE, abi::BUFFER_EXPOSURE, [1, 1]),
        (REACTIVE, abi::BUFFER_BIAS_CURRENT_COLOR_HINT, camera.render),
        (OUTPUT, abi::BUFFER_SCALING_OUTPUT_COLOR, output),
    ];
    let mut resources: Vec<(abi::Resource, u32, [u32; 2])> = kinds
        .iter()
        .filter(|&&(i, _, _)| !textures[i].0.is_null() && !(i == EXPOSURE && auto_exposure))
        .map(|&(i, kind, size)| (abi::Resource::texture(textures[i].0, textures[i].1), kind, size))
        .collect();
    let tags: Vec<abi::ResourceTag> = resources
        .iter_mut()
        .map(|(r, kind, size)| abi::ResourceTag::new(r, *kind, abi::VALID_UNTIL_EVALUATE, abi::Extent::sized(*size)))
        .collect();
    let mut inputs: Vec<*const abi::BaseStructure> = vec![std::ptr::from_ref(&viewport.base)];
    inputs.extend(tags.iter().map(|t| std::ptr::from_ref(&t.base)));
    // SAFETY: an open command list; inputs that live through the call.
    let code = unsafe { (sl.evaluate_feature)(abi::FEATURE_DLSS, token, inputs.as_ptr(), inputs.len() as u32, list) };
    if code != 0 {
        return Err(format!("slEvaluateFeature: {} ({code})", abi::result_name(code)));
    }
    Ok(mode)
}

/// `KLBF_DLSS_FG`: the upscale's depth and motion vectors (host textures in the states given) and its
/// camera, for this frame's present; copied by Streamline on the open host list `list`. The first
/// upscale of a frame only.
pub fn tag(list: *mut c_void, depth: (*mut c_void, u32), motion: (*mut c_void, u32), camera: &plan::Camera) {
    if !SWAP_CHAIN_PROXIED.load(Acquire) || depth.0.is_null() || motion.0.is_null() {
        return;
    }
    let Some(sl) = sl() else { return };
    let frame = FRAME.load(Acquire);
    if TAGGED.load(Acquire) == frame + 1 {
        return;
    }
    let Some(token) = frame_token(sl, frame as u32) else { return };
    let viewport = abi::ViewportHandle::new(VIEWPORT_FG);
    // (Frame generation reads the planes: the nearer one is the near one, whichever way they come.)
    let camera = camera.planes_in_order();
    let constants = camera.constants([1.0, 1.0], [1.0, 1.0]);
    let mut resources = [
        (abi::Resource::texture(depth.0, depth.1), abi::BUFFER_DEPTH, camera.render),
        (abi::Resource::texture(motion.0, motion.1), abi::BUFFER_MOTION_VECTORS, camera.motion_size()),
    ];
    let tags = resources.each_mut().map(|(r, kind, size)| abi::ResourceTag::new(r, *kind, abi::ONLY_VALID_NOW, abi::Extent::sized(*size)));
    // SAFETY: constants for this frame and viewport; tags of live textures in the states they are in
    // on the open list, which Streamline copies them on.
    let codes = unsafe {
        [(sl.set_constants)(&constants, token, &viewport), (sl.set_tag_for_frame)(token, &viewport, tags.as_ptr(), tags.len() as u32, list)]
    };
    if codes == [0, 0] {
        TAGGED.store(frame + 1, Release);
    } else {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| tracing::warn!(?codes, "DLSS Frame Generation: depth and motion vectors not tagged"));
    }
}

fn marker(m: u32, token: *const abi::FrameToken) {
    if let Some(set) = FUNCTIONS.get().and_then(|f| f.pcl_set_marker) {
        // SAFETY: a marker for a token of ours.
        unsafe { set(m, token) };
    }
}

/// Before a present through Streamline's swap chain of `size`: the frame's latency markers, and frame
/// generation switched on for a frame whose upscale tagged its inputs (off for menus, loads, videos).
/// Whether the swap chain is Streamline's (the present then takes no do-not-wait flag).
pub(crate) fn before_present(size: [u32; 2]) -> bool {
    if !SWAP_CHAIN_PROXIED.load(Acquire) {
        return false;
    }
    let (Some(sl), Some(functions)) = (sl(), FUNCTIONS.get()) else { return true };
    let frame = FRAME.load(Acquire);
    let Some(token) = frame_token(sl, frame as u32) else { return true };
    for m in [abi::SIMULATION_START, abi::SIMULATION_END, abi::RENDER_SUBMIT_START, abi::RENDER_SUBMIT_END] {
        marker(m, token);
    }
    let on = TAGGED.load(Acquire) == frame + 1;
    if on != FG_ON.load(Relaxed)
        && let (Some(set_options), Some(get_state)) = (functions.dlssg_set_options, functions.dlssg_get_state)
    {
        let viewport = abi::ViewportHandle::new(VIEWPORT_FG);
        let mut options = abi::DlssgOptions::new(if on { abi::DLSSG_ON } else { abi::DLSSG_OFF });
        if on {
            let mut state = abi::DlssgState::default();
            // SAFETY: a structure to fill, no options.
            let most = if unsafe { get_state(&viewport, &mut state, std::ptr::null()) } == 0 { state.num_frames_to_generate_max } else { 1 };
            options.num_frames_to_generate = frames_to_generate(fg_asked(), most);
            if SHOWN.swap(options.num_frames_to_generate + 1, Relaxed) != options.num_frames_to_generate + 1 {
                tracing::info!(asked = fg_asked(), shown = options.num_frames_to_generate + 1, most_generated = most, "DLSS Frame Generation: frames shown per rendered one");
            }
        }
        options.flags = abi::DLSSG_RETAIN_RESOURCES_WHEN_OFF;
        options.num_back_buffers = 2;
        [options.color_width, options.color_height] = size;
        // SAFETY: filled options for viewport 0, on the presenting thread.
        let code = unsafe { set_options(&viewport, &options) };
        tracing::info!(frame, on, code, result = abi::result_name(code), "DLSS Frame Generation switched");
        if code == 0 || code == 39 {
            FG_ON.store(on, Relaxed);
        }
    }
    marker(abi::PRESENT_START, token);
    true
}

/// After each present: the frame's last marker, and the next frame's tokens.
pub(crate) fn after_present() {
    let frame = FRAME.fetch_add(1, AcqRel);
    if SWAP_CHAIN_PROXIED.load(Acquire)
        && let Some(token) = sl().and_then(|sl| frame_token(sl, frame as u32))
    {
        marker(abi::PRESENT_END, token);
    }
}

/// Our swap chain's buffers are about to be made again: frame generation off until the next present
/// with an upscale (Streamline asks so).
pub(crate) fn resizing() {
    if !FG_ON.swap(false, Relaxed) {
        return;
    }
    if let Some(set_options) = FUNCTIONS.get().and_then(|f| f.dlssg_set_options) {
        let options = abi::DlssgOptions::new(abi::DLSSG_OFF);
        // SAFETY: filled options.
        unsafe { set_options(&abi::ViewportHandle::new(VIEWPORT_FG), &options) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_generated_follow_the_ask_and_the_driver() {
        assert_eq!(frames_to_generate(2, 1), 1, "x2 on an RTX 40");
        assert_eq!(frames_to_generate(4, 1), 1, "x4 asked on an RTX 40: x2");
        assert_eq!(frames_to_generate(4, 3), 3, "x4 on an RTX 50");
        assert_eq!(frames_to_generate(6, 3), 3, "x6 where the driver makes 3: x4");
        assert_eq!(frames_to_generate(6, 5), 5, "x6 where the driver makes 5");
        assert_eq!(frames_to_generate(3, 0), 1, "a driver that says nothing: x2");
    }

    /// Without the FSR path DLSS hooks (no VICTORY_FSR_DLL here): the switches go and the run is as without them.
    #[test]
    fn dlss_that_cannot_start_is_switched_off() {
        // SAFETY: this test alone sets these variables.
        unsafe {
            std::env::set_var("KLBF_DLSS", "1");
            std::env::set_var("KLBF_DLSS_FG", "4");
        }
        check_at_start();
        assert!(std::env::var_os("KLBF_DLSS").is_none() && std::env::var_os("KLBF_DLSS_FG").is_none());
        assert!(!upscaling());
    }
}
