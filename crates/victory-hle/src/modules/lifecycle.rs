//! The title's life cycle. A console title is never told it is closing:
//! quitting it or launching a new game first constrains it (its
//! constrained-change routines run), then suspends it (its suspend routines
//! run, and it has a second to save) and then ends the process. Closing the
//! title's window does the same here; a new launch then starts `victory`
//! again, which waits for this process to be gone.
#![allow(non_snake_case)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::too_many_arguments)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
use windows_sys::Win32::UI::WindowsAndMessaging::{self as wm, HMENU};
use windows_sys::core::{PCSTR, PCWSTR, w};

use crate::{HleModule, hle_export};

/// A suspend routine (`quiesced`) or a constrained-change routine
/// (`constrained`): both take a flag and the title's context.
type Routine = extern "system" fn(flag: u8, context: *mut c_void);

/// Registration id, routine and context.
type Registration = (usize, Routine, usize);

/// Each suspend routine.
static ROUTINES: Mutex<Vec<Registration>> = Mutex::new(Vec::new());

/// Each constrained-change routine.
static CONSTRAINED: Mutex<Vec<Registration>> = Mutex::new(Vec::new());

fn routines() -> MutexGuard<'static, Vec<Registration>> {
    ROUTINES.lock().unwrap_or_else(|e| e.into_inner())
}

fn constrained() -> MutexGuard<'static, Vec<Registration>> {
    CONSTRAINED.lock().unwrap_or_else(|e| e.into_inner())
}

/// How long the console lets suspend routines run before it ends the title.
const SUSPEND_DEADLINE: Duration = Duration::from_secs(1);

const ERROR_INVALID_PARAMETER: u32 = 87;

/// Adds `routine` to `list` and writes its registration id.
fn register(
    mut list: MutexGuard<'static, Vec<Registration>>,
    routine: Option<Routine>,
    context: *mut c_void,
    registration: *mut usize,
) -> u32 {
    let Some(routine) = routine else {
        return ERROR_INVALID_PARAMETER;
    };
    if registration.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    list.push((id, routine, context as usize));
    // SAFETY: checked non-null; the title passes room for its registration.
    unsafe { registration.write(id) };
    0
}

pub extern "system" fn RegisterAppStateChangeNotification(
    routine: Option<Routine>,
    context: *mut c_void,
    registration: *mut usize,
) -> u32 {
    register(routines(), routine, context, registration)
}

pub extern "system" fn UnregisterAppStateChangeNotification(registration: usize) {
    routines().retain(|&(id, ..)| id != registration);
}

/// The host's own version never calls the routine (a desktop app is never
/// constrained), so a title waiting for it would never suspend.
pub extern "system" fn RegisterAppConstrainedChangeNotification(
    routine: Option<Routine>,
    context: *mut c_void,
    registration: *mut usize,
) -> u32 {
    register(constrained(), routine, context, registration)
}

pub extern "system" fn UnregisterAppConstrainedChangeNotification(registration: usize) {
    constrained().retain(|&(id, ..)| id != registration);
}

/// The window procedure a title window had before ours.
const TITLE_PROC: PCWSTR = w!("victory.wndproc");

pub extern "system" fn CreateWindowExW(
    ex_style: u32,
    class: PCWSTR,
    name: PCWSTR,
    style: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    parent: HWND,
    menu: HMENU,
    instance: HINSTANCE,
    param: *const c_void,
) -> HWND {
    let main = main_window(parent, style, menu);
    // (KLBF Lab) The game window always has its taskbar button.
    let ex_style = if main { (ex_style | wm::WS_EX_APPWINDOW) & !wm::WS_EX_TOOLWINDOW } else { ex_style };
    let (x, y, width, height) = fit(parent, x, y, width, height);
    // SAFETY: the title's own arguments, forwarded (the size fitted).
    let hwnd = unsafe {
        wm::CreateWindowExW(
            ex_style, class, name, style, x, y, width, height, parent, menu, instance, param,
        )
    };
    hook(hwnd);
    if main {
        borderless_at_start(hwnd);
        come_forward(hwnd);
    }
    hwnd
}

/// See [`CreateWindowExW`].
pub extern "system" fn CreateWindowExA(
    ex_style: u32,
    class: PCSTR,
    name: PCSTR,
    style: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    parent: HWND,
    menu: HMENU,
    instance: HINSTANCE,
    param: *const c_void,
) -> HWND {
    let main = main_window(parent, style, menu);
    // (KLBF Lab) The game window always has its taskbar button.
    let ex_style = if main { (ex_style | wm::WS_EX_APPWINDOW) & !wm::WS_EX_TOOLWINDOW } else { ex_style };
    let (x, y, width, height) = fit(parent, x, y, width, height);
    // SAFETY: the title's own arguments, forwarded (the size fitted).
    let hwnd = unsafe {
        wm::CreateWindowExA(
            ex_style, class, name, style, x, y, width, height, parent, menu, instance, param,
        )
    };
    hook(hwnd);
    if main {
        borderless_at_start(hwnd);
        come_forward(hwnd);
    }
    hwnd
}

/// A console title sizes its window to its render target (
/// the title: 3840x2160 on a Series X), which can exceed the host's screen.
/// A top-level window larger than the work area is shrunk to fit, keeping
/// its proportions, and centred; what it shows is scaled to it (the swap
/// chain stretches). `VICTORY_WINDOW_MAX=WIDTHxHEIGHT` applies a smaller
/// limit without changing the title's render targets.
/// (KLBF Lab, 27/11) `KLBF_WINDOW_MONITOR=<n>` (the launcher's Display, 0 = the first monitor Windows
/// lists): that monitor's work area instead of the primary's, the window always centred on it.
fn fit(parent: HWND, x: i32, y: i32, width: i32, height: i32) -> (i32, i32, i32, i32) {
    let mut area = RECT::default();
    let chosen = chosen_monitor();
    // SAFETY: `area` receives the primary monitor's work area.
    let known = match chosen {
        Some(a) => {
            area = a;
            1
        }
        None => unsafe { wm::SystemParametersInfoW(wm::SPI_GETWORKAREA, 0, (&raw mut area).cast(), 0) },
    };
    let limit = window_limit();
    let fitted = fit_with_limit(&area, width, height, limit);
    if !parent.is_null() || known == 0 || width <= 0 || height <= 0 || (fitted == (width, height) && chosen.is_none()) {
        return (x, y, width, height);
    }
    tracing::info!(width, height, to = ?fitted, ?limit, "title window fitted to the screen");
    centred(&area, fitted)
}

/// A window of `size` centred on `area` (left, top, width, height).
fn centred(area: &RECT, (w, h): (i32, i32)) -> (i32, i32, i32, i32) {
    (area.left + (area.right - area.left - w) / 2, area.top + (area.bottom - area.top - h) / 2, w, h)
}

/// The work area of the monitor `KLBF_WINDOW_MONITOR` names, in the order Windows lists them; `None`
/// when unset or gone.
fn chosen_monitor() -> Option<RECT> {
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
    let n: usize = std::env::var("KLBF_WINDOW_MONITOR").ok()?.trim().parse().ok()?;
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, list: LPARAM) -> i32 {
        // SAFETY: `list` is the vector below, alive during the call.
        unsafe { (*(list as *mut Vec<HMONITOR>)).push(m) };
        1
    }
    let mut list: Vec<HMONITOR> = Vec::new();
    // SAFETY: the callback only pushes into `list`; then a plain query.
    unsafe {
        EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(each), &raw mut list as LPARAM);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..std::mem::zeroed() };
        (GetMonitorInfoW(*list.get(n)?, &mut info) != 0).then_some(info.rcWork)
    }
}

fn parse_window_limit(value: &str) -> Option<(i32, i32)> {
    let (width, height) = value.trim().split_once(['x', 'X'])?;
    let (width, height) = (width.trim().parse().ok()?, height.trim().parse().ok()?);
    (width > 0 && height > 0).then_some((width, height))
}

fn window_limit() -> Option<(i32, i32)> {
    static LIMIT: std::sync::OnceLock<Option<(i32, i32)>> = std::sync::OnceLock::new();
    *LIMIT.get_or_init(|| {
        let value = std::env::var_os("VICTORY_WINDOW_MAX")?;
        let parsed = value.to_str().and_then(parse_window_limit);
        if parsed.is_none() {
            tracing::warn!(
                ?value,
                "ignoring invalid VICTORY_WINDOW_MAX; expected positive WIDTHxHEIGHT"
            );
        }
        parsed
    })
}

fn fit_with_limit(
    area: &RECT,
    width: i32,
    height: i32,
    limit: Option<(i32, i32)>,
) -> (i32, i32) {
    let Some((max_width, max_height)) = limit else {
        return fit_into(area, width, height);
    };
    let bounded = RECT {
        left: 0,
        top: 0,
        right: (area.right - area.left).min(max_width),
        bottom: (area.bottom - area.top).min(max_height),
    };
    fit_into(&bounded, width, height)
}

/// `width` x `height` scaled down to fit `area`, or unchanged if it fits
/// (or is `CW_USEDEFAULT`).
fn fit_into(area: &RECT, width: i32, height: i32) -> (i32, i32) {
    let (aw, ah) = (area.right - area.left, area.bottom - area.top);
    if width <= 0 || height <= 0 || aw <= 0 || ah <= 0 || (width <= aw && height <= ah) {
        return (width, height);
    }
    let scale = f64::min(
        f64::from(aw) / f64::from(width),
        f64::from(ah) / f64::from(height),
    );
    (
        (f64::from(width) * scale) as i32,
        (f64::from(height) * scale) as i32,
    )
}

/// (KLBF Lab, 27/11) The title's main window: the first top-level window with a caption and no menu.
fn main_window(parent: HWND, style: u32, menu: HMENU) -> bool {
    static SEEN: AtomicBool = AtomicBool::new(false);
    let top = parent.is_null() && style & wm::WS_CHILD == 0 && style & wm::WS_CAPTION == wm::WS_CAPTION && menu.is_null();
    top && !SEEN.swap(true, Ordering::Relaxed)
}

/// (KLBF Lab, 27/11) Borderless fullscreen: the window without its frame, on its monitor,
/// keeping the picture's proportions (`letterbox`; black bars from a backdrop window behind it). The
/// swap chain and output keep their size (scaled into the window). Alt+Enter toggles it; the window's
/// place, style and menu come back. `KLBF_BORDERLESS=1` (the launcher's Display mode) starts in it.
struct Borderless {
    style: isize,
    rect: RECT,
    menu: usize,
    backdrop: usize,
}

static BORDERLESS: Mutex<Option<Borderless>> = Mutex::new(None);

fn borderless_at_start(hwnd: HWND) {
    if std::env::var("KLBF_BORDERLESS").is_ok_and(|v| v.trim() == "1") && !victory_gpu::klbf_bench::active() {
        toggle_borderless(hwnd);
    }
}

/// (KLBF Lab, 27/11) Once the game window shows, it comes in front with the focus: started by a hidden
/// launcher, Windows lets a new process take the foreground only through the foreground's own input
/// (attached for the call). Else the desktop kept the focus and Alt+F4 asked to shut Windows down.
fn come_forward(hwnd: HWND) {
    let at = hwnd as usize;
    let _ = std::thread::Builder::new().name("klbf-forward".into()).spawn(move || {
        let hwnd = at as HWND;
        // SAFETY: plain window calls on our window (a stale handle fails).
        unsafe {
            for _ in 0..600 {
                if wm::IsWindow(hwnd) == 0 {
                    return;
                }
                if wm::IsWindowVisible(hwnd) != 0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            to_front(hwnd);
        }
    });
}

/// `hwnd` in front with the focus, through the foreground window's thread input.
///
/// # Safety
/// `hwnd` is a window handle (a stale one fails).
unsafe fn to_front(hwnd: HWND) {
    use windows_sys::Win32::System::Threading::AttachThreadInput;
    // SAFETY: plain window calls; the threads' input is detached again.
    unsafe {
        let front = wm::GetForegroundWindow();
        if front == hwnd {
            return;
        }
        let theirs = wm::GetWindowThreadProcessId(front, std::ptr::null_mut());
        let ours = wm::GetWindowThreadProcessId(hwnd, std::ptr::null_mut());
        let attached = theirs != 0 && theirs != ours && AttachThreadInput(theirs, ours, 1) != 0;
        wm::BringWindowToTop(hwnd);
        wm::SetForegroundWindow(hwnd);
        if attached {
            AttachThreadInput(theirs, ours, 0);
        }
    }
}

/// The game window behind the backdrop: a click on the bars gives it the focus back.
static BACKDROP_FOR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The backdrop never takes the focus; a click on it puts the game in front.
extern "system" fn backdrop_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    const MA_NOACTIVATEANDEAT: LRESULT = 4;
    if message == wm::WM_MOUSEACTIVATE {
        let game = BACKDROP_FOR.load(Ordering::Relaxed) as HWND;
        // SAFETY: our game window (a stale handle fails).
        unsafe { to_front(game) };
        return MA_NOACTIVATEANDEAT;
    }
    // SAFETY: the default handling of our window.
    unsafe { wm::DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// The largest rect of `aspect` (width, height) centred in `area`.
fn letterbox(area: &RECT, (aw, ah): (i32, i32)) -> RECT {
    let (w, h) = (area.right - area.left, area.bottom - area.top);
    if aw <= 0 || ah <= 0 || w <= 0 || h <= 0 {
        return *area;
    }
    let (fw, fh) = if i64::from(w) * i64::from(ah) > i64::from(h) * i64::from(aw) {
        ((i64::from(h) * i64::from(aw) / i64::from(ah)) as i32, h)
    } else {
        (w, (i64::from(w) * i64::from(ah) / i64::from(aw)) as i32)
    };
    let (left, top) = (area.left + (w - fw) / 2, area.top + (h - fh) / 2);
    RECT { left, top, right: left + fw, bottom: top + fh }
}

/// A black, inert window covering `area`, just behind `game`.
fn backdrop(game: HWND, area: &RECT) -> HWND {
    use windows_sys::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject};
    const CLASS: PCWSTR = w!("KLBF Lab backdrop");
    // SAFETY: registers a plain class once (a second time fails harmlessly) and makes a window of it.
    unsafe {
        let instance = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null());
        let class = wm::WNDCLASSW {
            lpfnWndProc: Some(backdrop_proc),
            hInstance: instance,
            lpszClassName: CLASS,
            hbrBackground: GetStockObject(BLACK_BRUSH) as _,
            ..std::mem::zeroed()
        };
        wm::RegisterClassW(&class);
        let b = wm::CreateWindowExW(
            wm::WS_EX_TOOLWINDOW | wm::WS_EX_NOACTIVATE,
            CLASS,
            w!("KLBF Lab"),
            wm::WS_POPUP | wm::WS_VISIBLE,
            area.left,
            area.top,
            area.right - area.left,
            area.bottom - area.top,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if !b.is_null() {
            BACKDROP_FOR.store(game as usize, Ordering::Relaxed);
            wm::SetWindowPos(b, game, 0, 0, 0, 0, wm::SWP_NOMOVE | wm::SWP_NOSIZE | wm::SWP_NOACTIVATE);
        }
        b
    }
}

/// Into borderless fullscreen on the window's monitor, or back to the window it was. On the window's
/// thread (its procedure, or right after it is made).
fn toggle_borderless(hwnd: HWND) {
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
    let mut state = BORDERLESS.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: our title's window, on its own thread; plain window calls.
    unsafe {
        if let Some(was) = state.take() {
            if was.backdrop != 0 {
                wm::DestroyWindow(was.backdrop as HWND);
            }
            wm::SetWindowLongPtrW(hwnd, wm::GWL_STYLE, was.style);
            wm::SetMenu(hwnd, was.menu as HMENU);
            let r = was.rect;
            wm::SetWindowPos(hwnd, std::ptr::null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, wm::SWP_FRAMECHANGED | wm::SWP_NOZORDER | wm::SWP_NOOWNERZORDER);
            to_front(hwnd);
            tracing::info!("KLBF Lab: back to the window");
            return;
        }
        let (mut rect, mut client) = (RECT::default(), RECT::default());
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..std::mem::zeroed() };
        if wm::GetWindowRect(hwnd, &mut rect) == 0
            || wm::GetClientRect(hwnd, &mut client) == 0
            || GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info) == 0
        {
            return;
        }
        let style = wm::GetWindowLongPtrW(hwnd, wm::GWL_STYLE);
        let menu = wm::GetMenu(hwnd);
        let screen = info.rcMonitor;
        let fit = letterbox(&screen, (client.right - client.left, client.bottom - client.top));
        let bars = fit.left != screen.left || fit.top != screen.top || fit.right != screen.right || fit.bottom != screen.bottom;
        wm::SetMenu(hwnd, std::ptr::null_mut());
        let popup = (style as u32 & !(wm::WS_OVERLAPPEDWINDOW)) | wm::WS_POPUP | wm::WS_VISIBLE;
        wm::SetWindowLongPtrW(hwnd, wm::GWL_STYLE, popup as isize);
        wm::SetWindowPos(hwnd, wm::HWND_TOP, fit.left, fit.top, fit.right - fit.left, fit.bottom - fit.top, wm::SWP_FRAMECHANGED | wm::SWP_NOOWNERZORDER);
        let backdrop = if bars { backdrop(hwnd, &screen) as usize } else { 0 };
        to_front(hwnd);
        *state = Some(Borderless { style, rect, menu: menu as usize, backdrop });
        tracing::info!(left = fit.left, top = fit.top, right = fit.right, bottom = fit.bottom, "KLBF Lab: borderless fullscreen");
    }
}

/// (KLBF Lab) The process groups with KLBF Lab Settings under one taskbar button.
pub fn set_app_id() {
    victory_gpu::shell::set_app_id();
}

/// Puts our procedure in front of the title's.
fn hook(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    // SAFETY: a window this thread just created. The title's procedure is
    // stored before ours can run.
    unsafe {
        let title = wm::GetWindowLongPtrW(hwnd, wm::GWLP_WNDPROC);
        wm::SetPropW(hwnd, TITLE_PROC, title as *mut c_void);
        wm::SetWindowLongPtrW(hwnd, wm::GWLP_WNDPROC, window_proc as *const () as isize);
    }
    // KLBF Lab Bench: the card's close ends the title the same way.
    victory_gpu::klbf_bench::on_exit(|| end_title("bench finished", Box::new(|| {})));
    // KLBF Lab's F10 / F11 (and the menu's Settings / Restart): the same end, with the exit code play.ps1 reads.
    victory_gpu::klbf_keys::on_exit(|code| {
        EXIT_CODE.store(code as u32, Ordering::SeqCst);
        end_title(if code == victory_gpu::klbf_keys::EXIT_SETTINGS { "KLBF Lab settings" } else { "KLBF Lab restart" }, Box::new(|| {}));
    });
}

/// The process's exit code at its end ([`end`]): 0, or KLBF Lab's for the launcher.
static EXIT_CODE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // A user close (close button, Alt+F4, window or taskbar menu); a
    // console title never sees one, nor a WM_CLOSE from it.
    if message == wm::WM_SYSCOMMAND && wparam & 0xFFF0 == wm::SC_CLOSE as usize {
        victory_gpu::klbf_bench::window_closed();
        end_title("window closed", Box::new(|| {}));
        return 0;
    }
    // Alt or F10 puts a window into menu mode: the system runs a modal menu
    // loop on the thread that owns the window, which is the title's. The
    // title then stops (no frames, no input) until a key ends the loop, and
    // nothing shows it, since the window has no menu bar;
    // the title aborts itself about half a minute later. A console has no window
    // menu. The mouse still opens it from the title bar.
    if message == wm::WM_SYSCOMMAND && wparam & 0xFFF0 == wm::SC_KEYMENU as usize {
        return 0;
    }
    // KLBF Lab: Alt+Enter toggles borderless fullscreen (the Enter that follows as a character is eaten,
    // or Windows beeps).
    if (message == wm::WM_SYSKEYDOWN && wparam == 0x0D && lparam & (1 << 29) != 0 && !victory_gpu::klbf_bench::active())
        || (message == wm::WM_SYSCHAR && wparam == 0x0D)
    {
        if message == wm::WM_SYSKEYDOWN && lparam & (1 << 30) == 0 {
            toggle_borderless(hwnd);
        }
        return 0;
    }
    // KLBF Lab: F8, F10, F11.
    if victory_gpu::klbf_keys::on_message(message, wparam, lparam) {
        return 0;
    }
    // SAFETY: the procedure `hook` stored, called the way the system would.
    unsafe {
        let title = wm::GetPropW(hwnd, TITLE_PROC);
        if message == wm::WM_NCDESTROY {
            wm::RemovePropW(hwnd, TITLE_PROC);
        }
        let title = std::mem::transmute::<*mut c_void, wm::WNDPROC>(title);
        let result = wm::CallWindowProcW(title, hwnd, message, wparam, lparam);
        // The icons the host gave the window (the game's logos) go with it.
        if message == wm::WM_NCDESTROY {
            victory_gpu::shell::window_destroyed(hwnd as usize);
        }
        result
    }
}

/// Suspends the title as the console does when it ends it, runs `then`
/// (once), then ends the process. Returns at once; later calls do nothing.
pub(crate) fn end_title(why: &'static str, then: Box<dyn FnOnce() + Send>) {
    static ENDING: AtomicBool = AtomicBool::new(false);
    if ENDING.swap(true, Ordering::SeqCst) {
        return;
    }
    // Before the suspend: a pad keeps rumbling after the process ends.
    super::gameinput::stop_rumble();
    let constrain = constrained().clone();
    let suspend = routines().clone();
    tracing::info!(
        constrained = constrain.len(),
        routines = suspend.len(),
        "{why}: suspending the title"
    );
    // Whoever comes first (the routines or the deadline) runs `then`; the
    // other waits for it, so the process never ends halfway through.
    let then = std::sync::Arc::new(Mutex::new(Some(then)));
    let finish = move |how: &str| {
        let mut then = then.lock().unwrap_or_else(|e| e.into_inner());
        tracing::info!("{how}; ending the title");
        if let Some(then) = then.take() {
            then();
        }
        end();
    };
    let late = finish.clone();
    // Not on the calling thread: titles hand the suspend to their window
    // thread and wait for it inside the routine.
    std::thread::spawn(move || {
        constrain_then_suspend(&constrain, &suspend);
        finish("title suspended");
    });
    std::thread::spawn(move || {
        std::thread::sleep(SUSPEND_DEADLINE);
        late("title did not finish suspending in time");
    });
}

/// Runs the constrained-change routines (constrained), then the suspend
/// routines (quiesced), as the console does when it suspends a title.
/// Grand Theft Auto VI's suspend routine waits for a state that only its
/// constrained routine brings about; called alone it never returns.
fn constrain_then_suspend(constrain: &[Registration], suspend: &[Registration]) {
    for &(_, routine, context) in constrain.iter().chain(suspend) {
        routine(1, context as *mut c_void);
    }
}

/// Set in the environment of a relaunched `victory`: an inherited handle
/// of the process it replaces.
const RELAUNCHED_FROM: &str = "VICTORY_RELAUNCHED_FROM";

/// Starts `victory` again with `args` (after the title has suspended); the
/// new process waits until this one has ended.
pub(crate) fn relaunch(args: Vec<std::ffi::OsString>) {
    use windows_sys::Win32::Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE};
    let mut me: HANDLE = std::ptr::null_mut();
    // SAFETY: an inheritable duplicate of our own process handle.
    let duplicated = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            GetCurrentProcess(),
            GetCurrentProcess(),
            &mut me,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        )
    } != 0;
    let spawned = std::env::current_exe().and_then(|exe| {
        let mut command = std::process::Command::new(exe);
        command.args(&args);
        if duplicated {
            command.env(RELAUNCHED_FROM, (me as usize).to_string());
        }
        command.spawn()
    });
    match spawned {
        Ok(child) => tracing::info!(pid = child.id(), ?args, "relaunched"),
        Err(e) => tracing::warn!(%e, "relaunch failed; the title just ends"),
    }
}

/// In a relaunched `victory`: waits (30 s at most) for the process it
/// replaces to be gone, so the two never share the title's files.
pub fn wait_for_previous() {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::Threading::WaitForSingleObject;
    let Some(handle) = std::env::var(RELAUNCHED_FROM)
        .ok()
        .and_then(|h| h.parse::<usize>().ok())
    else {
        return;
    };
    // SAFETY: removing our own variable before any other thread reads it.
    unsafe { std::env::remove_var(RELAUNCHED_FROM) };
    let handle = handle as HANDLE;
    // SAFETY: the inherited process handle the previous process gave us.
    unsafe {
        WaitForSingleObject(handle, 30_000);
        CloseHandle(handle);
    }
}

fn end() {
    // Compiled host pipelines kept for the next run.
    #[cfg(windows)]
    victory_gpu::pipeline::save_pipeline_cache();
    // SAFETY: ends this process, as the console ends a suspended title.
    unsafe { TerminateProcess(GetCurrentProcess(), EXIT_CODE.load(Ordering::SeqCst)) };
}

pub(crate) fn module() -> HleModule {
    HleModule {
        name: "lifecycle",
        dlls: &[
            "api-ms-win-core-psm-appnotify-*",
            "ext-ms-win-rtcore-ntuser-window-*",
        ],
        exports: vec![
            hle_export!(RegisterAppStateChangeNotification),
            hle_export!(UnregisterAppStateChangeNotification),
            hle_export!(RegisterAppConstrainedChangeNotification),
            hle_export!(UnregisterAppConstrainedChangeNotification),
            hle_export!(CreateWindowExW),
            hle_export!(CreateWindowExA),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (KLBF Lab) Only the title's first captioned top-level window is the game's (taskbar button, focus,
    /// borderless); a child, a popup without caption or a later window is not.
    #[test]
    fn the_game_window_is_the_first_captioned_top_level_one() {
        let null = std::ptr::null_mut();
        assert!(!main_window(0x1234 as HWND, wm::WS_OVERLAPPEDWINDOW, null), "a child");
        assert!(!main_window(null, wm::WS_POPUP, null), "no caption");
        assert!(main_window(null, wm::WS_OVERLAPPEDWINDOW, null));
        assert!(!main_window(null, wm::WS_OVERLAPPEDWINDOW, null), "once");
    }

    /// (KLBF Lab) Borderless fullscreen keeps the picture's proportions: bars at the sides of a wide
    /// monitor (one left of the main one too), none on a 16:9 one.
    #[test]
    fn borderless_fills_the_monitor_at_the_pictures_shape() {
        let wide = RECT { left: 0, top: 0, right: 5120, bottom: 1440 };
        let r = letterbox(&wide, (3840, 2160));
        assert_eq!((r.left, r.top, r.right, r.bottom), (1280, 0, 3840, 1440));
        let left = RECT { left: -2560, top: 0, right: 0, bottom: 1440 };
        let r = letterbox(&left, (2560, 1440));
        assert_eq!((r.left, r.top, r.right, r.bottom), (-2560, 0, 0, 1440));
        let tall = RECT { left: 0, top: 0, right: 1920, bottom: 1200 };
        let r = letterbox(&tall, (1920, 1080));
        assert_eq!((r.left, r.top, r.right, r.bottom), (0, 60, 1920, 1140));
        let r = letterbox(&wide, (0, 0));
        assert_eq!(r.right, 5120, "no client size: the whole monitor");
    }

    /// (KLBF Lab) A 3840x2160 window on a 2560x1440 monitor left of the primary (negative x), and on a
    /// 5120x1440 primary: shrunk to fit, the same proportions, centred on it.
    #[test]
    fn a_window_is_centred_on_the_chosen_monitor() {
        let left = RECT { left: -2560, top: 0, right: 0, bottom: 1400 };
        let (w, h) = fit_with_limit(&left, 3840, 2160, None);
        assert!(w <= 2560 && h <= 1400 && (w * 9 - h * 16).abs() <= 16, "{w}x{h}");
        let (x, y, ..) = centred(&left, (w, h));
        assert_eq!((x, y), (-2560 + (2560 - w) / 2, (1400 - h) / 2));
        assert!(x >= -2560 && x + w <= 0, "inside the left monitor");
        let wide = RECT { left: 0, top: 0, right: 5120, bottom: 1400 };
        let (w, h) = fit_with_limit(&wide, 3840, 2160, None);
        let (x, ..) = centred(&wide, (w, h));
        assert_eq!(x, (5120 - w) / 2);
        assert_eq!(centred(&wide, (800, 600)), (2160, 400, 800, 600), "a window that fits: centred, its size kept");
    }

    extern "system" fn noop(_: u8, _: *mut c_void) {}

    #[test]
    fn registrations_come_and_go() {
        let (mut a, mut b) = (0, 0);
        assert_eq!(
            RegisterAppStateChangeNotification(Some(noop), std::ptr::null_mut(), &mut a),
            0
        );
        assert_eq!(
            RegisterAppStateChangeNotification(Some(noop), std::ptr::null_mut(), &mut b),
            0
        );
        UnregisterAppStateChangeNotification(a);
        let left: Vec<_> = routines().iter().map(|r| r.0).collect();
        assert_eq!(left, [b]);
    }

    #[test]
    fn constrained_registrations_come_and_go() {
        let (mut a, mut b) = (0, 0);
        assert_eq!(
            RegisterAppConstrainedChangeNotification(Some(noop), std::ptr::null_mut(), &mut a),
            0
        );
        assert_eq!(
            RegisterAppConstrainedChangeNotification(Some(noop), std::ptr::null_mut(), &mut b),
            0
        );
        assert_eq!(
            RegisterAppConstrainedChangeNotification(None, std::ptr::null_mut(), &mut a),
            ERROR_INVALID_PARAMETER
        );
        UnregisterAppConstrainedChangeNotification(a);
        let left: Vec<_> = constrained().iter().map(|r| r.0).collect();
        assert_eq!(left, [b]);
    }

    /// What `recording_routine` saw: (context, flag) per call, in order.
    static CALLS: Mutex<Vec<(usize, u8)>> = Mutex::new(Vec::new());

    extern "system" fn recording_routine(flag: u8, context: *mut c_void) {
        CALLS.lock().unwrap().push((context as usize, flag));
    }

    #[test]
    fn constrained_routines_run_before_suspend_routines() {
        let r: Routine = recording_routine;
        constrain_then_suspend(&[(1, r, 10), (2, r, 11)], &[(3, r, 20), (4, r, 21)]);
        assert_eq!(*CALLS.lock().unwrap(), [(10, 1), (11, 1), (20, 1), (21, 1)]);
    }

    #[test]
    fn oversized_windows_shrink_to_the_screen() {
        let area = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        assert_eq!(fit_into(&area, 3840, 2160), (1848, 1040));
        assert_eq!(fit_into(&area, 1280, 720), (1280, 720));
        assert_eq!(
            fit_into(&area, wm::CW_USEDEFAULT, 0),
            (wm::CW_USEDEFAULT, 0)
        );
    }

    #[test]
    fn configured_window_limit_fits_4k_into_1080p() {
        let area = RECT {
            left: 100,
            top: 50,
            right: 2660,
            bottom: 1490,
        };
        assert_eq!(fit_with_limit(&area, 3840, 2160, Some((1920, 1080))), (1920, 1080));
        assert_eq!(fit_with_limit(&area, 640, 480, Some((1920, 1080))), (640, 480));
        assert_eq!(fit_with_limit(&area, 3840, 2160, None), (2560, 1440));
    }

    #[test]
    fn configured_window_limit_still_respects_the_work_area() {
        let area = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        assert_eq!(fit_with_limit(&area, 3840, 2160, Some((2560, 1440))), (1848, 1040));
        assert_eq!(
            fit_with_limit(&area, wm::CW_USEDEFAULT, 0, Some((1920, 1080))),
            (wm::CW_USEDEFAULT, 0)
        );
    }

    #[test]
    fn configured_window_limit_preserves_non_widescreen_proportions() {
        let area = RECT {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1440,
        };
        assert_eq!(fit_with_limit(&area, 1600, 1200, Some((1920, 1080))), (1440, 1080));
    }

    #[test]
    fn window_limit_requires_two_positive_dimensions() {
        assert_eq!(parse_window_limit("1920x1080"), Some((1920, 1080)));
        assert_eq!(parse_window_limit(" 1920 X 1080 "), Some((1920, 1080)));
        for bad in [
            "",
            "1920",
            "0x1080",
            "1920x0",
            "-1x1080",
            "1920x-1",
            "1920x1080x2",
            "2147483648x1080",
        ] {
            assert_eq!(parse_window_limit(bad), None, "{bad}");
        }
    }

    #[test]
    fn hooked_windows_still_reach_the_title_procedure() {
        let null = std::ptr::null_mut();
        let hwnd = CreateWindowExW(
            0,
            w!("STATIC"),
            w!("before"),
            0,
            0,
            0,
            10,
            10,
            null,
            null,
            null,
            std::ptr::null(),
        );
        assert!(!hwnd.is_null());
        let mut text = [0u16; 16];
        // SAFETY: our window; the buffer holds 16 characters.
        unsafe {
            wm::SetWindowTextW(hwnd, w!("after"));
            let n = wm::GetWindowTextW(hwnd, text.as_mut_ptr(), 16);
            assert_eq!(String::from_utf16_lossy(&text[..n as usize]), "after");
            assert!(!wm::GetPropW(hwnd, TITLE_PROC).is_null());
            wm::DestroyWindow(hwnd);
        }
    }

    thread_local! {
        /// The system commands `recording` received, low four bits cleared.
        static COMMANDS: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    /// Stands in for a title's procedure: notes system commands and acts
    /// on none of them (no menu loop, no minimising).
    extern "system" fn recording(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == wm::WM_SYSCOMMAND {
            COMMANDS.with(|c| c.borrow_mut().push(wparam & 0xFFF0));
            return 0;
        }
        // SAFETY: the system's default procedure, called as the system would.
        unsafe { wm::DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    #[test]
    fn the_keyboard_menu_never_reaches_the_title() {
        let null = std::ptr::null_mut();
        let hwnd = CreateWindowExW(
            0,
            w!("STATIC"),
            w!("menu"),
            0,
            0,
            0,
            10,
            10,
            null,
            null,
            null,
            std::ptr::null(),
        );
        assert!(!hwnd.is_null());
        // SAFETY: our window, owned by this thread, so each message is
        // handled before SendMessageW returns. `recording` stands in for the
        // title's procedure and the original is put back before the window
        // is destroyed.
        unsafe {
            let title = wm::GetPropW(hwnd, TITLE_PROC);
            wm::SetPropW(hwnd, TITLE_PROC, recording as *const () as *mut c_void);
            let command = |c: u32, key: u8| {
                wm::SendMessageW(hwnd, wm::WM_SYSCOMMAND, c as usize, LPARAM::from(key))
            };
            command(wm::SC_KEYMENU, 0); // Alt or F10
            command(wm::SC_KEYMENU, b' '); // Alt+Space
            command(wm::SC_KEYMENU | 1, 0); // the low bits are the system's
            command(wm::SC_MINIMIZE, 0);
            wm::SetPropW(hwnd, TITLE_PROC, title);
            wm::DestroyWindow(hwnd);
        }
        let seen = COMMANDS.with(|c| c.borrow().clone());
        assert_eq!(seen, [wm::SC_MINIMIZE as usize]);
    }
}
