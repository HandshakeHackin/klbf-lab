//! (KLBF Lab, 27/11) The title's own hang watchdog, turned off at load (`KLBF_TITLE_WATCHDOG=1`
//! keeps it).
//!
//! What the title does (image offsets): +0x2215a20 starts a thread on +0x2215a60 that wakes once a
//! second (`QueryPerformanceCounter`, sleep 1000 ms). While no long task is under way (+0x71b6334 is
//! 0), the main loop's tick (+0x22159f0, the time at +0x71b6338) must move within 30 of those
//! wakes; during long tasks each one must end within 300 s. Otherwise it calls its crash handler
//! (+0x71b6348, +0x23cf620: writes 0x4d8bd421 to address 0) or writes 0 to address 0 itself
//! (+0x2215c35). A console never stalls its main loop that long; here a cold shader cache does
//! (pipelines made one after another for more than 30 s during the opening movies), and the run
//! ended with "write to 0x0 at +0x2215c35" or "+0x23cf683" (issues #69, #116).
//!
//! Its loop starts by reading its stop flag (+0x71b6340): `test al, 1 ; jne` to its own way out
//! (+0x2215dce, which undoes what it registered and returns). The `jne` becomes a `jmp`: the thread
//! ends at once, as if told to stop. Our own hang watch (`VICTORY_HANG_WATCH_S`) still says where a
//! real stall waits.

use crate::resolution::{Patched, Site};

/// +0x2215ab8: `mov al, [+0x71b6340] ; test al, 1 ; jne +0x2215dce`.
pub const STOP_CHECK: Site = Site {
    at: 0x2215ab8,
    what: "the title's hang watchdog",
    bytes: &[0x8a, 0x05, 0x82, 0x08, 0xfa, 0x04, 0xa8, 0x01, 0x0f, 0x85, 0x08, 0x03, 0x00, 0x00],
};

/// The same bytes with the `jne` (6 bytes) made a `jmp` (5) to the same place, and a `nop`.
pub fn stop_now_code() -> Vec<u8> {
    let mut code = STOP_CHECK.bytes[..8].to_vec();
    // jne rel32 ends at +14, jmp rel32 at +13: one more to the same target.
    let rel = i32::from_le_bytes(STOP_CHECK.bytes[10..14].try_into().unwrap()) + 1;
    code.push(0xe9);
    code.extend(rel.to_le_bytes());
    code.push(0x90);
    code
}

/// On unless `KLBF_TITLE_WATCHDOG=1`.
pub fn asked_off() -> bool {
    std::env::var("KLBF_TITLE_WATCHDOG").map_or(true, |v| v.trim() != "1")
}

/// Turns the title's watchdog off unless asked to keep it; says what came of it.
///
/// # Safety
/// The image stays mapped and no thread runs its code yet.
#[cfg(windows)]
pub unsafe fn apply_asked(base: usize, size: usize) {
    if !asked_off() {
        println!("watchdog: the title's own left on (KLBF_TITLE_WATCHDOG=1)");
        return;
    }
    // SAFETY: the caller's promise.
    let done = unsafe { crate::resolution::write(base, size, &STOP_CHECK, &stop_now_code()) };
    match done {
        Patched::Done => tracing::info!("watchdog: the title's own turned off (KLBF_TITLE_WATCHDOG=1 keeps it)"),
        Patched::Differs => tracing::warn!("watchdog: not patched, the title's bytes are not the ones expected (another build?)"),
        Patched::Failed(why) => tracing::warn!(why, "watchdog: not patched"),
    }
    println!("watchdog: the title's own at title+{:#x}: {done:?}", STOP_CHECK.at);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both branches land on the watchdog's way out, +0x2215dce.
    #[test]
    fn the_jump_goes_where_the_branch_went() {
        let target = |end: usize, rel: &[u8]| (end as i64 + i64::from(i32::from_le_bytes(rel.try_into().unwrap()))) as usize;
        let code = stop_now_code();
        assert_eq!(code.len(), STOP_CHECK.bytes.len());
        assert_eq!(code[..8], STOP_CHECK.bytes[..8]);
        assert_eq!(code[8], 0xe9);
        assert_eq!(code[13], 0x90);
        assert_eq!(target(STOP_CHECK.at + 14, &STOP_CHECK.bytes[10..14]), 0x2215dce);
        assert_eq!(target(STOP_CHECK.at + 13, &code[9..13]), 0x2215dce);
    }
}
