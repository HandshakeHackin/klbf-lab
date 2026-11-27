//! (27/11) KLBF Lab's in-game keys, read once per presented frame while our window is in front (no
//! global hook): F10 twice within 3 s closes the game for the launcher's settings (exit code 10), F11
//! twice restarts it with the same settings (11), F8 saves a screenshot of the presented image. After a
//! first F10 or F11 the caption says what a second press does. During a bench run F10 and F11 are the
//! bench's (F10 stops it), so only F8 is read.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

const VK_F8: i32 = 0x77;
const VK_F10: i32 = 0x79;
const VK_F11: i32 = 0x7A;
/// The second press must come within this.
const WINDOW: Duration = Duration::from_secs(3);
/// Exit codes play.ps1 reads.
pub const EXIT_SETTINGS: i32 = 10;
pub const EXIT_RESTART: i32 = 11;

/// A key that acts on its second press within [`WINDOW`].
#[derive(Default, Clone, Copy)]
struct DoublePress {
    first: Option<Instant>,
}

impl DoublePress {
    /// A press at `now`: true when it is the second within the window (then it starts over).
    fn press(&mut self, now: Instant) -> bool {
        match self.first {
            Some(t) if now.duration_since(t) <= WINDOW => {
                self.first = None;
                true
            }
            _ => {
                self.first = Some(now);
                false
            }
        }
    }

    /// Whether a first press waits for its second at `now`.
    fn pending(&self, now: Instant) -> bool {
        self.first.is_some_and(|t| now.duration_since(t) <= WINDOW)
    }
}

#[derive(Default)]
struct Keys {
    settings: DoublePress,
    restart: DoublePress,
    /// Whether each of F8, F10, F11 was down at the last frame (a press is an edge).
    down: [bool; 3],
    shot: bool,
}

static KEYS: Mutex<Option<Keys>> = Mutex::new(None);

/// Once per presented frame (`queue.rs`).
pub(crate) fn poll() {
    if !crate::klbf_bench::ours_in_front() {
        return;
    }
    // SAFETY: plain key state queries.
    let now_down = [VK_F8, VK_F10, VK_F11].map(|vk| unsafe { GetAsyncKeyState(vk) } < 0);
    let now = Instant::now();
    let mut keys = KEYS.lock().unwrap_or_else(|e| e.into_inner());
    let k = keys.get_or_insert_with(Keys::default);
    let pressed: Vec<bool> = (0..3).map(|i| now_down[i] && !k.down[i]).collect();
    k.down = now_down;
    if pressed[0] {
        k.shot = true;
    }
    if crate::klbf_bench::active() {
        return;
    }
    let code = if pressed[1] && k.settings.press(now) {
        Some(EXIT_SETTINGS)
    } else if pressed[2] && k.restart.press(now) {
        Some(EXIT_RESTART)
    } else {
        None
    };
    if let Some(code) = code {
        drop(keys);
        tracing::info!(code, "KLBF Lab: the game closes for the launcher ({})", if code == EXIT_SETTINGS { "settings" } else { "restart" });
        std::process::exit(code);
    }
}

/// What the caption says instead of its figures while a first press waits.
pub(crate) fn hint() -> Option<&'static str> {
    let keys = KEYS.lock().unwrap_or_else(|e| e.into_inner());
    let k = keys.as_ref()?;
    let now = Instant::now();
    if k.settings.pending(now) {
        Some("Press F10 again: settings (the game will close)")
    } else if k.restart.pending(now) {
        Some("Press F11 again: restart (the game will close)")
    } else {
        None
    }
}

/// Whether F8 asked for a screenshot since the last call (it is then taken by the present).
#[cfg_attr(not(feature = "diagnostics"), allow(dead_code))]
pub(crate) fn take_shot() -> bool {
    let mut keys = KEYS.lock().unwrap_or_else(|e| e.into_inner());
    keys.as_mut().is_some_and(|k| std::mem::take(&mut k.shot))
}

/// Where screenshots go: `KLBF_SHOTS_DIR`, else %LOCALAPPDATA%\KLBF-Lab\screenshots.
#[cfg_attr(not(feature = "diagnostics"), allow(dead_code))]
pub(crate) fn shots_dir() -> std::path::PathBuf {
    std::env::var_os("KLBF_SHOTS_DIR").map(Into::into).unwrap_or_else(|| {
        std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("KLBF-Lab").join("screenshots")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_acts_on_its_second_press_within_three_seconds() {
        let t0 = Instant::now();
        let mut k = DoublePress::default();
        assert!(!k.press(t0), "the first press only arms it");
        assert!(k.pending(t0 + Duration::from_secs(1)));
        assert!(k.press(t0 + Duration::from_secs(2)), "the second within 3 s acts");
        assert!(!k.pending(t0 + Duration::from_secs(2)), "and it starts over");
        assert!(!k.press(t0 + Duration::from_secs(10)));
        assert!(!k.press(t0 + Duration::from_secs(14)), "4 s later: a new first press");
        assert!(!k.pending(t0 + Duration::from_secs(18)), "the hint goes after 3 s");
        assert!(k.press(t0 + Duration::from_secs(16)));
    }
}
