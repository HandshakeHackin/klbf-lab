//! The shader containers behind the programs of the device's precompiled
//! pipeline methods (0x1c8 graphics, 0x218 compute).
//!
//! Those methods hand over console machine code, not containers. Its first
//! 24 bytes carry the program's hash (the console compiler starts every
//! program with it), and every such program is also in a container of the
//! title's shader pack. `victory shaders` (the launcher's `--shaders`)
//! writes, under the cache folder `precompiled`, an index from those 24
//! bytes to the container's hash (`index.txt`, one `<head hex> <stage>
//! <container hash>` line each) and each container once (`<container
//! hash>.cdxil`); a run looks the code up there and makes the pipeline from
//! the container as `CreateGraphicsPipelineState` and
//! `CreateComputePipelineState` would.
//!
//! Ported from LukeFarch's branch of Codec's repo (5611a2f, 93d2aa3,
//! 34fffb3), where it was found by decoding the methods' descriptions, as
//! CGBJ main carries it. Here only the compute method (0x218), behind
//! `VICTORY_PRECOMPILED=1`.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// The bytes of a program's code that name it.
pub const HEAD: usize = 24;

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Whether the precompiled pipeline method `slot` makes pipelines (`VICTORY_PRECOMPILED=1`,
/// default off here, theirs on: 0x218 compute; the graphics method 0x1c8 is not ported yet and
/// keeps failing as before). Off: both fail as they always did (the meter of `crate::device`).
pub(crate) fn on(slot: usize) -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    slot == 0x218
        && *ON.get_or_init(|| matches!(std::env::var("VICTORY_PRECOMPILED").as_deref().map(str::trim), Ok("1" | "compute")))
}

/// Whether [`record`] writes the index (the `victory shaders` pass, [`record_on`]).
static RECORDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The `victory shaders` pass writes the index from here on.
pub fn record_on() {
    RECORDING.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Compute pipelines made by 0x218 and calls that found no container, since the start.
pub(crate) static MADE: [std::sync::atomic::AtomicU64; 2] = [const { std::sync::atomic::AtomicU64::new(0) }; 2];

/// Looks programs up in (or, when translating ahead, writes them to) `dir`.
pub fn use_precompiled(dir: PathBuf) {
    let _ = DIR.set(dir);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// When translating ahead: records the programs of `container` (each by
/// the first [`HEAD`] bytes of its code), so that a run finds the
/// container from the code a precompiled pipeline method hands over.
pub fn record(container: &[u8]) {
    use victory_shader::{Container, FourCC};
    if DIR.get().is_none() || !RECORDING.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let Ok(c) = Container::parse(container) else { return };
    let Some(part) = c.part(FourCC::DXIL).or(c.part(FourCC::ACCC)) else { return };
    let Ok(programs) = victory_shader::native_programs(part.data) else { return };
    for p in &programs {
        record_program(&p.code, &format!("{:?}", p.stage), container);
    }
}

fn record_program(code: &[u8], stage: &str, container: &[u8]) {
    let Some(dir) = DIR.get() else { return };
    if code.len() < HEAD {
        return;
    }
    // A pass writes the index afresh: every container is seen again.
    static INDEX: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    let Some(index) = INDEX.get_or_init(|| {
        if let Err(e) = std::fs::create_dir_all(dir) {
            tracing::warn!(dir = %dir.display(), error = %e, "precompiled pipeline index not written");
            return None;
        }
        match std::fs::File::create(dir.join("index.txt")) {
            Ok(f) => Some(Mutex::new(f)),
            Err(e) => {
                tracing::warn!(dir = %dir.display(), error = %e, "precompiled pipeline index not written");
                None
            }
        }
    }) else {
        return;
    };
    let hash = blake3::hash(container).to_hex();
    let file = dir.join(format!("{hash}.cdxil"));
    if !file.exists() {
        // Written beside, then renamed: a pass cut short leaves no torn
        // container behind.
        let part = dir.join(format!("{hash}.part"));
        if std::fs::write(&part, container).and_then(|()| std::fs::rename(&part, &file)).is_err() {
            let _ = std::fs::remove_file(&part);
            return;
        }
    }
    let line = format!("{} {stage} {hash}\n", hex(&code[..HEAD]));
    let _ = index.lock().unwrap_or_else(|e| e.into_inner()).write_all(line.as_bytes());
}

/// The index, read on first use: code head to (stage, container hash).
fn index() -> &'static HashMap<[u8; HEAD], (String, String)> {
    static INDEX: OnceLock<HashMap<[u8; HEAD], (String, String)>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let text = DIR
            .get()
            .and_then(|d| std::fs::read_to_string(d.join("index.txt")).ok())
            .unwrap_or_default();
        let mut map = HashMap::new();
        for line in text.lines() {
            let mut words = line.split_whitespace();
            let (Some(head), Some(stage), Some(hash)) = (words.next(), words.next(), words.next())
            else {
                continue;
            };
            let Some(key) = unhex(head) else { continue };
            map.entry(key).or_insert((stage.to_owned(), hash.to_owned()));
        }
        tracing::info!(
            programs = map.len(),
            "precompiled pipeline index ({})",
            if map.is_empty() { "none: run the launcher with --shaders" } else { "read" }
        );
        map
    })
}

fn unhex(text: &str) -> Option<[u8; HEAD]> {
    let mut out = [0u8; HEAD];
    if text.len() != 2 * HEAD {
        return None;
    }
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(text.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// The container of the program whose code starts with `head`, and its
/// stage as the index names it (`Vs`, `Ps`, `Cs`, ...).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn container(head: &[u8; HEAD]) -> Option<(String, Vec<u8>)> {
    let (stage, hash) = index().get(head)?;
    let bytes = std::fs::read(DIR.get()?.join(format!("{hash}.cdxil"))).ok()?;
    Some((stage.clone(), bytes))
}

#[cfg(test)]
mod tests {
    #[test]
    fn heads_round_trip_through_hex() {
        let head: [u8; super::HEAD] = std::array::from_fn(|i| (i * 11) as u8);
        assert_eq!(super::unhex(&super::hex(&head)), Some(head));
        assert_eq!(super::unhex("00"), None);
    }
}
