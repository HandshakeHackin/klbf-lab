//! (KLBF Lab, 27/11) Keyboard and mouse as a gamepad: the stand-in pad takes its buttons, sticks and
//! triggers from a keymap (`controls.json` in KLBF Lab's home), and the camera (right stick) from the
//! mouse's raw movement. It is merged with every real pad, so both work at once.
//!
//! The mouse is the player's own until a click in the game picture takes it: the cursor is hidden and
//! kept inside the window. F9 lets it go (and takes it again), and so does the window losing the focus;
//! the cursor is never held otherwise.
//!
//! Letters and digits in the keymap name key positions on a QWERTY keyboard (W A S D is Z Q S D on an
//! AZERTY one): Windows says which key sits there on the active layout.
//!
//! Ideas from the keyboard and mouse controls of LooperSalty's AnyXboxX (click to capture, F9 to
//! release, position-based letters) and eikkapine's victory-amd (raw mouse motion, a PC action-game
//! layout); this is our own code (GPL-3.0, as theirs).

#![cfg_attr(not(windows), allow(dead_code))]

use super::{
    A, B, DOWN, KeyState, LEFT, LEFT_SHOULDER, LEFT_STICK, MENU, PadState, RIGHT, RIGHT_SHOULDER,
    RIGHT_STICK, UP, VIEW, X, Y,
};

/// The file made at the first start (and when it has no `keys`), read at each start.
pub const DEFAULT_JSON: &str = r#"{
  "help": "KLBF Lab keyboard and mouse. Letters and digits are key positions on a QWERTY keyboard (W A S D = Z Q S D on AZERTY). Click in the game to take the mouse for the camera; F9 lets it go. Targets: A B X Y LeftBumper RightBumper LeftTrigger RightTrigger LeftStickPress RightStickPress DpadUp DpadDown DpadLeft DpadRight Menu View LeftStickUp LeftStickDown LeftStickLeft LeftStickRight RightStickUp RightStickDown RightStickLeft RightStickRight. Inputs: A-Z 0-9 Space Enter Esc Tab Backspace LShift RShift LCtrl RCtrl LAlt CapsLock Up Down Left Right Insert Delete Home End PageUp PageDown F1-F7 F12 Numpad0-Numpad9 MouseLeft MouseRight MouseMiddle Mouse4 Mouse5 WheelUp WheelDown. Delete this file to get the defaults back.",
  "enabled": true,
  "mouse_sensitivity": 1.0,
  "invert_y": false,
  "keys": {
    "W": "LeftStickUp",
    "S": "LeftStickDown",
    "A": "LeftStickLeft",
    "D": "LeftStickRight",
    "MouseLeft": "RightTrigger",
    "MouseRight": "LeftTrigger",
    "MouseMiddle": "RightStickPress",
    "Mouse4": "LeftBumper",
    "Mouse5": "RightBumper",
    "WheelUp": "DpadRight",
    "WheelDown": "DpadLeft",
    "LShift": "A",
    "Space": "X",
    "R": "B",
    "F": "Y",
    "E": "DpadRight",
    "C": "LeftStickPress",
    "LCtrl": "LeftStickPress",
    "Q": "RightBumper",
    "Tab": "LeftBumper",
    "H": "RightStickPress",
    "G": "DpadLeft",
    "Esc": "Menu",
    "P": "Menu",
    "M": "View",
    "Enter": "A",
    "Backspace": "B",
    "Up": "DpadUp",
    "Down": "DpadDown",
    "Left": "DpadLeft",
    "Right": "DpadRight",
    "1": "DpadUp",
    "2": "DpadRight",
    "3": "DpadDown",
    "4": "DpadLeft",
    "I": "RightStickUp",
    "K": "RightStickDown",
    "J": "RightStickLeft",
    "L": "RightStickRight"
  }
}
"#;

/// Mouse inputs beside the virtual keys (the buttons are their virtual keys, 1 to 6).
pub const WHEEL_UP: u16 = 0x100;
pub const WHEEL_DOWN: u16 = 0x101;

/// What an input presses: a button bit, or an axis (0 LT, 1 RT, 2 LX, 3 LY, 4 RX, 5 RY) toward a sign.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Button(u32),
    Axis(usize, f32),
}

fn target(name: &str) -> Option<Target> {
    use Target::*;
    Some(match name {
        "A" => Button(A),
        "B" => Button(B),
        "X" => Button(X),
        "Y" => Button(Y),
        "LeftBumper" => Button(LEFT_SHOULDER),
        "RightBumper" => Button(RIGHT_SHOULDER),
        "LeftStickPress" => Button(LEFT_STICK),
        "RightStickPress" => Button(RIGHT_STICK),
        "DpadUp" => Button(UP),
        "DpadDown" => Button(DOWN),
        "DpadLeft" => Button(LEFT),
        "DpadRight" => Button(RIGHT),
        "Menu" => Button(MENU),
        "View" => Button(VIEW),
        "LeftTrigger" => Axis(0, 1.0),
        "RightTrigger" => Axis(1, 1.0),
        "LeftStickLeft" => Axis(2, -1.0),
        "LeftStickRight" => Axis(2, 1.0),
        "LeftStickDown" => Axis(3, -1.0),
        "LeftStickUp" => Axis(3, 1.0),
        "RightStickLeft" => Axis(4, -1.0),
        "RightStickRight" => Axis(4, 1.0),
        "RightStickDown" => Axis(5, -1.0),
        "RightStickUp" => Axis(5, 1.0),
        _ => return None,
    })
}

/// Set-1 scan codes of the QWERTY letter and digit positions.
const POSITIONS: &[u8; 36] = b"QWERTYUIOPASDFGHJKLZXCVBNM1234567890";
const SCANS: [u32; 36] = [
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23,
    0x24, 0x25, 0x26, 0x2C, 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
    0x08, 0x09, 0x0A, 0x0B,
];

/// The input code of a keymap name; `vk_of_scan` is the active layout's key at a scan code.
/// F8 to F11 are KLBF Lab's own keys, so they are not offered.
fn input(name: &str, vk_of_scan: &impl Fn(u32) -> u8) -> Option<u16> {
    let upper = name.to_ascii_uppercase();
    if let [c] = upper.as_bytes()
        && let Some(i) = POSITIONS.iter().position(|p| p == c)
    {
        return Some(u16::from(vk_of_scan(SCANS[i]))).filter(|&vk| vk > 6);
    }
    if let Some(n) = upper.strip_prefix("NUMPAD").and_then(|n| n.parse::<u16>().ok()) {
        return (n <= 9).then_some(0x60 + n);
    }
    if let Some(n) = upper.strip_prefix('F').and_then(|n| n.parse::<u16>().ok()) {
        return ((1..=7).contains(&n) || n == 12).then_some(0x6F + n);
    }
    Some(match upper.as_str() {
        "MOUSELEFT" => 0x01,
        "MOUSERIGHT" => 0x02,
        "MOUSEMIDDLE" => 0x04,
        "MOUSE4" => 0x05,
        "MOUSE5" => 0x06,
        "WHEELUP" => WHEEL_UP,
        "WHEELDOWN" => WHEEL_DOWN,
        "SPACE" => 0x20,
        "ENTER" => 0x0D,
        "ESC" => 0x1B,
        "TAB" => 0x09,
        "BACKSPACE" => 0x08,
        "LSHIFT" => 0xA0,
        "RSHIFT" => 0xA1,
        "LCTRL" => 0xA2,
        "RCTRL" => 0xA3,
        "LALT" => 0xA4,
        "CAPSLOCK" => 0x14,
        "UP" => 0x26,
        "DOWN" => 0x28,
        "LEFT" => 0x25,
        "RIGHT" => 0x27,
        "INSERT" => 0x2D,
        "DELETE" => 0x2E,
        "HOME" => 0x24,
        "END" => 0x23,
        "PAGEUP" => 0x21,
        "PAGEDOWN" => 0x22,
        _ => return None,
    })
}

/// The keymap and the mouse settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Keymap {
    pub enabled: bool,
    pub sensitivity: f32,
    pub invert_y: bool,
    pub binds: Vec<(u16, Target)>,
}

impl Keymap {
    /// The keymap a file says, and whether it has `keys`; what it lacks or gets wrong is the default
    /// (without `keys`, the default keys: the caller writes them in, keeping the settings).
    pub fn parse(text: &str, vk_of_scan: &impl Fn(u32) -> u8) -> (Keymap, bool) {
        let fields = json::object(text).unwrap_or_default();
        let get = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v);
        let keys = match get("keys") {
            Some(json::Value::Object(keys)) if !keys.is_empty() => Some(keys),
            _ => None,
        };
        let binds = match keys {
            Some(keys) => binds(keys, vk_of_scan),
            None => Keymap::defaults(vk_of_scan).binds,
        };
        let keymap = Keymap {
            enabled: !matches!(get("enabled"), Some(json::Value::Bool(false))),
            sensitivity: match get("mouse_sensitivity") {
                Some(&json::Value::Number(n)) if n.is_finite() && n >= 0.0 => n.min(20.0) as f32,
                _ => 1.0,
            },
            invert_y: matches!(get("invert_y"), Some(json::Value::Bool(true))),
            binds,
        };
        (keymap, keys.is_some())
    }

    pub fn defaults(vk_of_scan: &impl Fn(u32) -> u8) -> Keymap {
        let fields = json::object(DEFAULT_JSON).expect("the default keymap is JSON");
        let Some((_, json::Value::Object(keys))) = fields.iter().find(|(n, _)| n == "keys") else {
            unreachable!("the default keymap has keys")
        };
        Keymap { enabled: true, sensitivity: 1.0, invert_y: false, binds: binds(keys, vk_of_scan) }
    }

    /// The pad for the inputs held down and the mouse's stick (x, y; up positive).
    pub fn pad(&self, down: &[u16], look: (f32, f32)) -> PadState {
        let mut axes = [0.0f32; 6];
        let mut buttons = 0;
        for &(code, t) in &self.binds {
            if !down.contains(&code) {
                continue;
            }
            match t {
                Target::Button(bit) => buttons |= bit,
                Target::Axis(i, sign) => axes[i] += sign,
            }
        }
        let axes = axes.map(|v| v.clamp(-1.0, 1.0));
        let further = |a: f32, b: f32| if b.abs() > a.abs() { b } else { a };
        PadState {
            buttons,
            left_trigger: axes[0].max(0.0),
            right_trigger: axes[1].max(0.0),
            left_x: axes[2],
            left_y: axes[3],
            right_x: further(axes[4], look.0),
            right_y: further(axes[5], look.1),
        }
    }
}

fn binds(keys: &[(String, json::Value)], vk_of_scan: &impl Fn(u32) -> u8) -> Vec<(u16, Target)> {
    keys.iter()
        .filter_map(|(name, t)| match t {
            json::Value::String(t) => Some((input(name, vk_of_scan)?, target(t)?)),
            _ => None,
        })
        .collect()
}

/// Mouse counts a second that push the stick to its end at sensitivity 1.
const FULL_SPEED: f32 = 1500.0;
/// How much of the speed is kept each poll (8 ms): bridges the mouse's reports and the title's frames.
const KEEP: f32 = 0.8;
/// The least stick a moving mouse gives: titles drop small stick values (their dead zone).
/// ponytail: one fixed value; a setting if a title's dead zone is wider.
const DEAD_ZONE_JUMP: f32 = 0.12;

/// The mouse's speed, smoothed, as a stick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Look {
    speed: (f32, f32),
}

impl Look {
    /// One poll: `dx`, `dy` counts (screen y downwards) over `seconds`; returns the stick (up positive).
    pub fn step(&mut self, dx: i32, dy: i32, seconds: f32, keymap: &Keymap) -> (f32, f32) {
        let seconds = seconds.max(1e-3);
        let smooth = |old: f32, d: i32| old * KEEP + (1.0 - KEEP) * d as f32 / seconds;
        self.speed = (smooth(self.speed.0, dx), smooth(self.speed.1, dy));
        let stick = |v: f32| {
            let s = v * keymap.sensitivity / FULL_SPEED;
            if s.abs() < 1e-3 {
                return 0.0;
            }
            (s.signum() * (DEAD_ZONE_JUMP + (1.0 - DEAD_ZONE_JUMP) * s.abs())).clamp(-1.0, 1.0)
        };
        let y = stick(self.speed.1);
        (stick(self.speed.0), if keymap.invert_y { y } else { -y })
    }
}

/// Just what the keymap needs of JSON: objects, strings, numbers, booleans (arrays and null skipped).
mod json {
    #[derive(Clone, Debug, PartialEq)]
    pub enum Value {
        Object(Vec<(String, Value)>),
        String(String),
        Number(f64),
        Bool(bool),
        Other,
    }

    pub fn object(text: &str) -> Option<Vec<(String, Value)>> {
        let mut p = Parser(text.trim_start_matches('\u{feff}').as_bytes(), 0);
        match p.value()? {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    struct Parser<'a>(&'a [u8], usize);

    impl Parser<'_> {
        fn peek(&mut self) -> Option<u8> {
            while self.0.get(self.1).is_some_and(u8::is_ascii_whitespace) {
                self.1 += 1;
            }
            self.0.get(self.1).copied()
        }
        fn eat(&mut self, c: u8) -> Option<()> {
            (self.peek()? == c).then(|| self.1 += 1)
        }
        fn value(&mut self) -> Option<Value> {
            match self.peek()? {
                b'{' => {
                    self.1 += 1;
                    let mut fields = Vec::new();
                    if self.eat(b'}').is_some() {
                        return Some(Value::Object(fields));
                    }
                    loop {
                        let Value::String(name) = self.value()? else { return None };
                        self.eat(b':')?;
                        fields.push((name, self.value()?));
                        if self.eat(b',').is_none() {
                            self.eat(b'}')?;
                            return Some(Value::Object(fields));
                        }
                    }
                }
                b'[' => {
                    self.1 += 1;
                    if self.eat(b']').is_some() {
                        return Some(Value::Other);
                    }
                    loop {
                        self.value()?;
                        if self.eat(b',').is_none() {
                            self.eat(b']')?;
                            return Some(Value::Other);
                        }
                    }
                }
                b'"' => {
                    self.1 += 1;
                    let mut s = Vec::new();
                    loop {
                        let c = *self.0.get(self.1)?;
                        self.1 += 1;
                        match c {
                            b'"' => return Some(Value::String(String::from_utf8(s).ok()?)),
                            b'\\' => {
                                let e = *self.0.get(self.1)?;
                                self.1 += 1;
                                match e {
                                    b'n' => s.push(b'\n'),
                                    b't' => s.push(b'\t'),
                                    b'r' | b'b' | b'f' => {}
                                    b'u' => {
                                        let hex = std::str::from_utf8(self.0.get(self.1..self.1 + 4)?).ok()?;
                                        self.1 += 4;
                                        let c = char::from_u32(u32::from_str_radix(hex, 16).ok()?)
                                            .unwrap_or('?');
                                        s.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
                                    }
                                    other => s.push(other),
                                }
                            }
                            c => s.push(c),
                        }
                    }
                }
                _ => {
                    let start = self.1;
                    while self.0.get(self.1).is_some_and(|c| !b",}] \t\r\n".contains(c)) {
                        self.1 += 1;
                    }
                    let word = std::str::from_utf8(&self.0[start..self.1]).ok()?;
                    match word {
                        "true" => Some(Value::Bool(true)),
                        "false" => Some(Value::Bool(false)),
                        "null" => Some(Value::Other),
                        n => n.parse().ok().map(Value::Number),
                    }
                }
            }
        }
    }
}

/// Codes held down: the keys (virtual keys) plus the mouse's.
pub fn down(keys: &[KeyState], mouse: &[u16]) -> Vec<u16> {
    keys.iter().map(|k| u16::from(k.virtual_key)).chain(mouse.iter().copied()).collect()
}

/// The host side: the file, the window's messages, the mouse.
#[cfg(windows)]
pub mod host {
    use std::path::PathBuf;
    use std::sync::LazyLock;
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, Ordering::Relaxed};
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, MAPVK_VSC_TO_VK, MapVirtualKeyW};
    use windows_sys::Win32::UI::Input::{
        GetRawInputData, MOUSE_MOVE_ABSOLUTE, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT,
        RIM_TYPEMOUSE, RegisterRawInputDevices,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging as wm;

    use super::{Keymap, Look, PadState, WHEEL_DOWN, WHEEL_UP};

    /// KLBF Lab's home (`KLBF_HOME`, else `%USERPROFILE%\Saved Games\KLBF Lab`), as the launcher's.
    pub fn path() -> Option<PathBuf> {
        let home = std::env::var_os("KLBF_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| {
            let user = std::env::var_os("USERPROFILE").filter(|v| !v.is_empty())?;
            Some(PathBuf::from(user).join("Saved Games").join("KLBF Lab"))
        })?;
        Some(home.join("controls.json"))
    }

    fn vk_of_scan(scan: u32) -> u8 {
        // SAFETY: plain query.
        unsafe { MapVirtualKeyW(scan, MAPVK_VSC_TO_VK) as u8 }
    }

    /// Read once, at the first poll; the file is made (or its keys filled in) when it has none.
    pub static KEYMAP: LazyLock<Keymap> = LazyLock::new(|| {
        let Some(path) = path() else { return Keymap::defaults(&vk_of_scan) };
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let (keymap, has_keys) = Keymap::parse(&text, &vk_of_scan);
        // Never from tests: the player's own file.
        if !has_keys && !cfg!(test) {
            // Its settings kept (the launcher may have written only them), the default keys added.
            let mut out = super::DEFAULT_JSON
                .replace("\"enabled\": true", &format!("\"enabled\": {}", keymap.enabled))
                .replace("\"invert_y\": false", &format!("\"invert_y\": {}", keymap.invert_y));
            out = out.replace("\"mouse_sensitivity\": 1.0", &format!("\"mouse_sensitivity\": {:?}", keymap.sensitivity));
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
            if let Err(e) = std::fs::write(&path, out) {
                tracing::warn!(%e, "KLBF Lab: controls.json not written");
            }
        }
        tracing::info!(
            enabled = keymap.enabled,
            sensitivity = keymap.sensitivity,
            binds = keymap.binds.len(),
            "KLBF Lab keyboard and mouse"
        );
        keymap
    });

    /// The window the mouse is taken in (0: none).
    static TAKEN: AtomicIsize = AtomicIsize::new(0);
    /// The click that took it is still held: no trigger pull until it is let go.
    static TAKING_CLICK: AtomicBool = AtomicBool::new(false);
    static RAW: AtomicBool = AtomicBool::new(false);
    static DX: AtomicI32 = AtomicI32::new(0);
    static DY: AtomicI32 = AtomicI32::new(0);
    /// Wheel notches up and down since the last poll.
    static WHEEL: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];

    pub fn taken() -> bool {
        TAKEN.load(Relaxed) != 0
    }

    fn take(hwnd: HWND) {
        if !RAW.swap(true, Relaxed) {
            let device = RAWINPUTDEVICE { usUsagePage: 1, usUsage: 2, dwFlags: 0, hwndTarget: hwnd };
            // SAFETY: one valid device.
            if unsafe { RegisterRawInputDevices(&device, 1, size_of::<RAWINPUTDEVICE>() as u32) } == 0 {
                tracing::warn!("KLBF Lab: no raw mouse input");
            }
        }
        TAKEN.store(hwnd as isize, Relaxed);
        clip(hwnd);
        DX.store(0, Relaxed);
        DY.store(0, Relaxed);
        tracing::info!("KLBF Lab: mouse taken for the camera (F9 lets it go)");
    }

    /// Lets the cursor go; nothing when it was not taken.
    pub fn release() {
        if TAKEN.swap(0, Relaxed) != 0 {
            // SAFETY: a null rectangle frees the cursor.
            unsafe { wm::ClipCursor(std::ptr::null()) };
            tracing::info!("KLBF Lab: mouse let go");
        }
    }

    /// Keeps the cursor inside the window's picture.
    fn clip(hwnd: HWND) {
        // SAFETY: plain queries on our own window.
        unsafe {
            let mut r = RECT::default();
            if wm::GetClientRect(hwnd, &mut r) == 0 {
                return;
            }
            let mut a = POINT { x: r.left, y: r.top };
            let mut b = POINT { x: r.right, y: r.bottom };
            ClientToScreen(hwnd, &mut a);
            ClientToScreen(hwnd, &mut b);
            wm::ClipCursor(&RECT { left: a.x, top: a.y, right: b.x, bottom: b.y });
        }
    }

    /// From the title window's procedure: `Some(result)` when the message was ours (not passed on).
    pub fn on_message(hwnd: HWND, message: u32, wparam: usize, lparam: isize) -> Option<isize> {
        if !KEYMAP.enabled || victory_gpu::klbf_bench::input_locked() {
            release();
            return None;
        }
        match message {
            wm::WM_LBUTTONDOWN if !taken() => {
                TAKING_CLICK.store(true, Relaxed);
                take(hwnd);
                Some(0)
            }
            wm::WM_KEYDOWN | wm::WM_SYSKEYDOWN if wparam == 0x78 => {
                if lparam & (1 << 30) == 0 {
                    if taken() { release() } else { take(hwnd) }
                }
                Some(0)
            }
            wm::WM_KILLFOCUS | wm::WM_NCDESTROY => {
                release();
                None
            }
            wm::WM_ACTIVATE if wparam & 0xFFFF == 0 => {
                release();
                None
            }
            wm::WM_ACTIVATEAPP if wparam == 0 => {
                release();
                None
            }
            wm::WM_SIZE | wm::WM_MOVE | wm::WM_EXITSIZEMOVE if taken() => {
                clip(hwnd);
                None
            }
            // No cursor over the picture while the mouse is the camera.
            wm::WM_SETCURSOR if taken() && lparam & 0xFFFF == wm::HTCLIENT as isize => {
                // SAFETY: no cursor is a valid cursor.
                unsafe { wm::SetCursor(std::ptr::null_mut()) };
                Some(1)
            }
            wm::WM_MOUSEWHEEL if taken() => {
                let delta = (wparam >> 16) as u16 as i16;
                WHEEL[usize::from(delta < 0)].fetch_add(1, Relaxed);
                Some(0)
            }
            wm::WM_INPUT if taken() => {
                let mut raw = RAWINPUT::default();
                let mut size = size_of::<RAWINPUT>() as u32;
                // SAFETY: room for one input; the handle is the message's.
                let got = unsafe {
                    GetRawInputData(
                        lparam as _,
                        RID_INPUT,
                        (&raw mut raw).cast(),
                        &mut size,
                        size_of::<RAWINPUTHEADER>() as u32,
                    )
                };
                // SAFETY: a mouse input, read as one.
                // ponytail: absolute motion (remote desktop, tablets) is skipped; deltas from the
                // previous position if someone plays that way.
                if got != u32::MAX && raw.header.dwType == RIM_TYPEMOUSE {
                    let m = unsafe { raw.data.mouse };
                    if m.usFlags & MOUSE_MOVE_ABSOLUTE == 0 {
                        DX.fetch_add(m.lLastX, Relaxed);
                        DY.fetch_add(m.lLastY, Relaxed);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// How long a wheel notch holds its button: the title reads once a frame.
    const NOTCH: Duration = Duration::from_millis(120);

    /// The poll thread's mouse state.
    #[derive(Default)]
    pub struct Mouse {
        look: Look,
        last: Option<Instant>,
        wheel_until: [Option<Instant>; 2],
    }

    impl Mouse {
        /// The stand-in pad from the real keys held (`held`, with focus) and the mouse.
        pub fn pad(&mut self, held: &[super::KeyState], focused: bool) -> PadState {
            let keymap = &*KEYMAP;
            let now = Instant::now();
            let seconds = self.last.map_or(0.008, |l| (now - l).as_secs_f32());
            self.last = Some(now);
            let (dx, dy) = (DX.swap(0, Relaxed), DY.swap(0, Relaxed));
            let mut mouse = Vec::new();
            let look = if focused && taken() {
                // Mouse buttons: physical left and right (swapped buttons kept as the player set them).
                for vk in [1u8, 2, 4, 5, 6] {
                    // SAFETY: plain query.
                    if unsafe { GetAsyncKeyState(i32::from(vk)) } < 0 {
                        mouse.push(u16::from(vk));
                    }
                }
                if !mouse.contains(&1) {
                    TAKING_CLICK.store(false, Relaxed);
                }
                if TAKING_CLICK.load(Relaxed) {
                    mouse.retain(|&c| c != 1);
                }
                for (i, code) in [WHEEL_UP, WHEEL_DOWN].into_iter().enumerate() {
                    // ponytail: notches within one hold merge into one press; a queue if fast
                    // scrolling must count each.
                    if WHEEL[i].swap(0, Relaxed) > 0 {
                        self.wheel_until[i] = Some(now + NOTCH);
                    }
                    if self.wheel_until[i].is_some_and(|u| now < u) {
                        mouse.push(code);
                    }
                }
                self.look.step(dx, dy, seconds, keymap)
            } else {
                self.look = Look::default();
                self.wheel_until = [None, None];
                (0.0, 0.0)
            };
            keymap.pad(&super::down(held, &mouse), look)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The QWERTY layout: the key at each position is the letter printed there.
    fn qwerty(scan: u32) -> u8 {
        SCANS.iter().position(|&s| s == scan).map_or(0, |i| POSITIONS[i])
    }

    /// AZERTY: A/Q and Z/W swapped, M on the semicolon's position (not a letter position here).
    fn azerty(scan: u32) -> u8 {
        match qwerty(scan) {
            b'Q' => b'A',
            b'A' => b'Q',
            b'W' => b'Z',
            b'Z' => b'W',
            other => other,
        }
    }

    fn keymap() -> Keymap {
        Keymap::defaults(&qwerty)
    }

    #[test]
    fn defaults_are_a_pc_action_layout() {
        let k = keymap();
        assert!(k.enabled && k.sensitivity == 1.0 && !k.invert_y);
        let pad = k.pad(&[u16::from(b'W'), u16::from(b'D'), 0x20, 0xA0, 0x01, 0x02, 0x1B], (0.0, 0.0));
        assert_eq!((pad.left_x, pad.left_y), (1.0, 1.0));
        assert_eq!((pad.left_trigger, pad.right_trigger), (1.0, 1.0));
        assert_eq!(pad.buttons, X | A | MENU);
        let pad = k.pad(&[u16::from(b'R'), u16::from(b'F'), u16::from(b'C'), u16::from(b'M'), WHEEL_UP], (0.0, 0.0));
        assert_eq!(pad.buttons, B | Y | LEFT_STICK | VIEW | RIGHT);
        assert_eq!(k.pad(&[], (0.0, 0.0)), PadState::default());
    }

    #[test]
    fn opposite_keys_cancel_and_the_mouse_moves_the_camera() {
        let k = keymap();
        let pad = k.pad(&[u16::from(b'A'), u16::from(b'D')], (0.5, -0.25));
        assert_eq!((pad.left_x, pad.right_x, pad.right_y), (0.0, 0.5, -0.25));
        // A camera key pushed further than the mouse wins.
        let pad = k.pad(&[u16::from(b'L')], (0.5, 0.0));
        assert_eq!(pad.right_x, 1.0);
    }

    #[test]
    fn letters_follow_the_key_positions() {
        let k = Keymap::defaults(&azerty);
        // On AZERTY the forward key (QWERTY W's place) is Z, left (A's place) is Q.
        let pad = k.pad(&[u16::from(b'Z'), u16::from(b'Q')], (0.0, 0.0));
        assert_eq!((pad.left_x, pad.left_y), (-1.0, 1.0));
        assert_eq!(k.pad(&[u16::from(b'W')], (0.0, 0.0)).left_y, 0.0);
    }

    #[test]
    fn a_file_overrides_and_settings_are_read() {
        let text = "\u{feff}{ \"enabled\": false, \"mouse_sensitivity\": 2.5, \"invert_y\": true, \"help\": \"a \\\"b\\\" \\u00e9\",
            \"list\": [1, {\"x\": null}], \"keys\": { \"Space\": \"A\", \"F9\": \"B\", \"X\": \"Nothing\", \"Numpad8\": \"LeftStickUp\" } }";
        let (k, has_keys) = Keymap::parse(text, &qwerty);
        assert!(has_keys && !k.enabled && k.invert_y);
        assert_eq!(k.sensitivity, 2.5);
        assert_eq!(k.binds, vec![(0x20, Target::Button(A)), (0x68, Target::Axis(3, 1.0))]);
    }

    #[test]
    fn a_file_without_keys_gets_the_defaults() {
        let (k, has_keys) = Keymap::parse("{\"mouse_sensitivity\": 0.5}", &qwerty);
        assert!(!has_keys && k.enabled);
        assert_eq!((k.sensitivity, k.binds.clone()), (0.5, keymap().binds));
        let (k, has_keys) = Keymap::parse("not json", &qwerty);
        assert_eq!((k, has_keys), (keymap(), false));
    }

    #[test]
    fn mouse_motion_becomes_a_smoothed_stick() {
        let k = keymap();
        let mut look = Look::default();
        assert_eq!(look.step(0, 0, 0.008, &k), (0.0, 0.0));
        // Moving right and down: stick right and down (up positive), past the dead-zone jump.
        let (x, y) = look.step(6, 6, 0.008, &k);
        assert!(x > DEAD_ZONE_JUMP && y < -DEAD_ZONE_JUMP && x == -y);
        // It builds up while moving, then fades out once the mouse stops.
        let (x2, _) = look.step(6, 6, 0.008, &k);
        assert!(x2 > x);
        let mut fade = x2;
        for _ in 0..60 {
            fade = look.step(0, 0, 0.008, &k).0;
        }
        assert_eq!(fade, 0.0);
        // A fast flick is the stick's end; inverted Y flips only Y.
        let inverted = Keymap { invert_y: true, ..keymap() };
        let mut look = Look::default();
        let (x, y) = look.step(10_000, 10_000, 0.008, &inverted);
        assert_eq!((x, y), (1.0, 1.0));
    }
}
