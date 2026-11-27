//! (KLBF Lab) Our launcher's settings built into the executable, for launchers that start it bare
//! (`victory.exe run <game> --execute`: AmericasLauncher, i95 and others). They then get what
//! `play.ps1` gives, not the base's defaults.
//!
//! Applied only when the start is bare: `play.ps1` sets `VICTORY_GAME_DIR` and its own set, which
//! stays as it is. A setting already in the environment is never replaced. `KLBF_DEFAULTS=0`
//! applies none. Keep this list the same as `play.ps1`'s `$set` (cache and save folders left to
//! the executable's own defaults).

/// `play.ps1`'s standing set, as of op-amd-v12.
const SET: &[(&str, &str)] = &[
    // The standing set of the fork's runs
    ("VICTORY_JOIN_COPIES", "1"),
    ("VICTORY_REFRESH_ONCE", "1"),
    ("VICTORY_CARD_REHASH_RUNS", "1"),
    ("VICTORY_BACK_OWN_VIEW", "1"),
    ("VICTORY_WIDE_MERGE", "1"),
    ("VICTORY_MIRROR_FAST", "1"),
    ("VICTORY_FRAME_LOG", "1"),
    ("VICTORY_CARD_METER", "1"),
    ("VICTORY_PIPELINE_WAIT_MS", "500"),
    ("VICTORY_ASYNC_SUBMIT", "1"),
    ("VICTORY_QUEUE2", "1"),
    ("VICTORY_INDIRECT_WINDOW", "1"),
    ("VICTORY_TEXTURE_NO_WAIT", "1"),
    ("VICTORY_COPY_PLAIN", "1"),
    ("VICTORY_WIDE_TEXTURES", "1"),
    ("VICTORY_INDIRECT_GATHER", "1"),
    ("VICTORY_LATE_WAIT_LISTS", "1024"),
    ("VICTORY_CARD_FILLS_BATCH", "0"),
    ("VICTORY_GATHER_DISPATCH", "0"),
    // cool_fox's launcher set (without Series S, VICTORY_THIN and the stand-in ambient light)
    ("VICTORY_RELAY", "2"),
    ("VICTORY_WIDE_BACK", "1"),
    ("VICTORY_RELAY_COPY", "1"),
    ("VICTORY_CARD_STEPS", "2"),
    ("VICTORY_INDIRECT_BATCH", "2"),
    ("VICTORY_MAP_THREADS", "1"),
    ("VICTORY_CARD_AWAKE", "0"),
    ("VICTORY_SHOW_AFTER", "1"),
    ("VICTORY_JOIN_PARTS", "8"),
    ("VICTORY_UPLOAD_AHEAD", "1"),
    ("VICTORY_CARD_BUFFERS_MIN_KB", "64"),
    ("VICTORY_LEVELS_ON_DEMAND", "0"),
    // Memory and upload switches
    ("VICTORY_POOL_DISCARD", "1"),
    ("VICTORY_FILL_TITLE", "1"),
    ("VICTORY_PARTIAL_DELTA", "1"),
    ("VICTORY_BAKE_DELTA", "1"),
    ("VICTORY_MAP_LOCK_FINE", "1"),
    ("VICTORY_POOL_CAP", "1"),
    // Draw set-up
    ("VICTORY_TABLE_MEMO", "1"),
    ("VICTORY_LIST_ALIAS_EXACT", "1"),
    // v12: every written descriptor slot passed on in the same frame (Yaekai's 8 GB caps: black textures)
    ("VICTORY_HEAP_CHANGED_MAX", "0"),
    ("VICTORY_HEAP_PASS_MAX", "0"),
    // Pipelines written down for `victory warm` (the launcher makes them ahead before the game starts)
    ("VICTORY_PIPELINE_BOOK", "1"),
    // Picture
    ("VICTORY_BAKE_TEXTURES_MS", "10000"),
    ("VICTORY_PLANE_NOW", "1"),
    // v12: LIST_HOLD, WINDOWS_BAR_MB, PORT_CONSTANTS, SOFT_VIEWS, SAMPLERS_MADE, TABLE_NAMED_ONLY and
    // QUEUE_MARKS left off (real play 27/11: T-pose and black textures with them on, clean without)
];

/// Sets the launcher's settings for a bare start. Call first in `main`, before any thread.
pub fn apply() {
    if std::env::var_os("KLBF_DEFAULTS").is_some_and(|v| v == "0") || std::env::var_os("VICTORY_GAME_DIR").is_some() {
        return;
    }
    let mut set = |k: &str, v: &str| {
        if std::env::var_os(k).is_none() {
            // SAFETY: first thing in `main`, before any other thread exists.
            unsafe { std::env::set_var(k, v) };
        }
    };
    for &(k, v) in SET {
        set(k, v);
    }
    // No violet vegetation with ray tracing off (play.ps1: unless -RayTracing or -NoProbeRays).
    if std::env::var_os("VICTORY_BVH_BUILDER").is_none_or(|v| v != "1") {
        set("VICTORY_PROBE_RAYS", "1");
    }
    set("KLBF_DEFAULTS_APPLIED", "1");
}

#[cfg(test)]
mod tests {
    /// The list stays the same as `play.ps1`'s `$set`: every name there with a fixed value is here.
    #[test]
    fn same_as_the_launcher() {
        let script = include_str!("../../../play.ps1");
        let start = script.find("$set = [ordered]@{").unwrap();
        let block = &script[start..start + script[start..].find("\n}").unwrap()];
        for piece in block.split(['\n', ';']) {
            let piece = piece.trim();
            if let Some((k, v)) = piece.split_once('=')
                && k.trim().starts_with("VICTORY_")
                && v.trim().starts_with('\'')
            {
                let (k, v) = (k.trim(), v.trim().trim_matches('\''));
                assert!(super::SET.contains(&(k, v)), "play.ps1 sets {k}={v}, the built-in list does not");
            }
        }
    }
}
