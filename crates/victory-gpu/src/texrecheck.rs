//! (`VICTORY_TEX_RECHECK=1`, default off) Whole-content rechecks of textures
//! picked by index after their upload.
//!
//! Such a texture is uploaded once and then looked at five times
//! (`draw.rs` `RECHECK_FRAMES`, 4..400 frames, ~19 s at 30 fps) by a 48 KiB
//! sample (start, middle, end). Its memory is written by the title through
//! other views of its pages (every "changed after its upload with nothing to
//! tell of it" warning of runs 401-405 had `blocks_shown_elsewhere` > 0), which
//! no guard or write-watch reports. A level streamed in later than the five
//! looks, or lying between the three sample windows, kept the bytes of the
//! first upload (black or the far LOD) for the rest of the run.
//!
//! Here every texture uploaded whole is kept in a list, and a worker thread
//! hashes ALL of its bytes (read through the kernel: pages not there are
//! zeros) 4, 12, 40, 120 and 400 frames after its upload or its last change,
//! then every `VICTORY_TEX_RECHECK_TAIL` frames (default 400) for as long as
//! the slot shows it. A changed one is made due in the next texture pass,
//! whose own check (`texture::unchanged`) and byte budget then upload it.
//! The worker hashes at most `VICTORY_TEX_RECHECK_MB` (default 64) MB per frame;
//! what is due beyond that waits for the next frame.
//! Counters: the "texture rechecks" line every 5 s.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Condvar, Mutex};

const STEPS: [u64; 5] = [4, 12, 40, 120, 400];
/// Most textures kept at once.
const MOST: usize = 1 << 18;
const CHUNK: usize = 1 << 20;

pub(crate) fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("VICTORY_TEX_RECHECK").is_ok_and(|v| v.trim() == "1"))
}

fn budget() -> u64 {
    static MB: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *MB.get_or_init(|| {
        std::env::var("VICTORY_TEX_RECHECK_MB").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(64u64).max(1)
    }) << 20
}

fn tail() -> u64 {
    static T: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *T.get_or_init(|| {
        std::env::var("VICTORY_TEX_RECHECK_TAIL").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(400u64).max(1)
    })
}

fn wait(step: u8) -> u64 {
    STEPS.get(usize::from(step)).copied().unwrap_or_else(tail)
}

type Key = (usize, u32);

/// What the log says of a texture: format (DXGI number), width, height,
/// levels, first level the view shows.
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct Info {
    pub(crate) format: u32,
    pub(crate) width: u64,
    pub(crate) height: u32,
    pub(crate) mips: u16,
    pub(crate) first: u32,
}

impl Info {
    pub(crate) fn of(resource: crate::object::This, first: u32) -> Info {
        crate::resource::texture_memory(resource).map_or(Info { first, ..Info::default() }, |(_, d)| Info {
            format: d.format,
            width: d.width,
            height: d.height,
            mips: d.mip_levels,
            first,
        })
    }
}

/// Looks in a row that found a change after which a texture counts as one
/// that changes at every look: no longer made due by this recheck (counted),
/// until a look finds it as it was.
const NOISY: u8 = 3;
/// Textures logged of each kind (all zeros at a later look; noisy).
const LOGGED: u64 = 64;
static LOGGED_ZERO: AtomicU64 = AtomicU64::new(0);
static LOGGED_NOISY: AtomicU64 = AtomicU64::new(0);

struct Entry {
    at: u64,
    len: u64,
    info: Info,
    /// Changed looks in a row.
    streak: u8,
    /// Made due by this recheck; its pass has not uploaded it yet.
    pending: bool,
    zero_logged: bool,
    looks: u32,
    /// 0: not hashed yet (the next look takes it as the uploaded content).
    hash: u128,
    step: u8,
    due: u64,
    generation: u64,
}

#[derive(Default)]
struct State {
    entries: HashMap<Key, Entry>,
    order: BTreeSet<(u64, usize, u32)>,
    /// Slots found changed, for their heap's next pass: (heap, slot).
    changed: Vec<Key>,
    frame: u64,
    ticks: u64,
    generation: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static TICK: Condvar = Condvar::new();

/// Since the last line: looks (whole hashes), found changed, bytes hashed,
/// ns hashing, bytes of slots made due, frames whose budget ran out, looks
/// skipped (memory not all there), refused (list full), slots made due;
/// and the untold changes the old recheck found (`draw.rs` warning), by
/// its looks 0..4, and their bytes.
/// Then: [9] looks at memory not all mapped (hashed anyway, missing pages
/// as zeros); [10] made due whose pass found the host copy holding those
/// bytes (no upload: a false alarm); [11] made due and uploaded again,
/// [12] their bytes; [13] changes not made due (noisy textures); [14] looks
/// that found all zeros at look 3 or later.
pub(crate) static COUNTS: [AtomicU64; 15] = [const { AtomicU64::new(0) }; 15];
pub(crate) static UNTOLD: [AtomicU64; 7] = [const { AtomicU64::new(0) }; 7];
static UNTOLD_TOTAL: AtomicU64 = AtomicU64::new(0);

/// The old recheck (`draw.rs`) found texture `bytes` changed at its look `looks`.
pub(crate) fn untold(looks: u8, bytes: u64) {
    UNTOLD[0].fetch_add(1, Relaxed);
    UNTOLD[1 + usize::from(looks).min(4)].fetch_add(1, Relaxed);
    UNTOLD[6].fetch_add(bytes, Relaxed);
    UNTOLD_TOTAL.fetch_add(1, Relaxed);
}

fn key(heap: &crate::descriptor::host::HostHeap, slot: u32) -> Key {
    (heap as *const _ as usize, slot)
}

/// Slot `slot` of `heap` was uploaded whole from title memory `[at, at + len)`
/// in `frame` (`same`: the pass found its host copy holding those bytes and
/// uploaded nothing).
pub(crate) fn track(
    heap: &crate::descriptor::host::HostHeap,
    slot: u32,
    at: u64,
    len: u64,
    frame: u64,
    same: bool,
    info: Info,
) {
    if !on() || len == 0 {
        return;
    }
    let k = key(heap, slot);
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let st = g.get_or_insert_with(State::default);
    st.generation += 1;
    let generation = st.generation;
    match st.entries.get_mut(&k) {
        Some(e) => {
            st.order.remove(&(e.due, k.0, k.1));
            if e.pending {
                e.pending = false;
                if same {
                    COUNTS[10].fetch_add(1, Relaxed);
                } else {
                    COUNTS[11].fetch_add(1, Relaxed);
                    COUNTS[12].fetch_add(len, Relaxed);
                }
            }
            if (e.at, e.len) != (at, len) {
                (e.at, e.len, e.hash, e.streak, e.looks, e.zero_logged) = (at, len, 0, 0, 0, false);
            }
            e.info = info;
            // Same memory: the hash that made it due is about what went up.
            e.step = 0;
            e.due = if e.hash == 0 { frame } else { frame + STEPS[0] };
            e.generation = generation;
            st.order.insert((e.due, k.0, k.1));
        }
        None => {
            if st.entries.len() >= MOST {
                COUNTS[7].fetch_add(1, Relaxed);
                return;
            }
            st.entries.insert(
                k,
                Entry {
                    at,
                    len,
                    info,
                    streak: 0,
                    pending: false,
                    zero_logged: false,
                    looks: 0,
                    hash: 0,
                    step: 0,
                    due: frame,
                    generation,
                },
            );
            st.order.insert((frame, k.0, k.1));
        }
    }
}

/// Slot `slot`'s texture went (streamed out, or the slot shows another).
pub(crate) fn untrack(heap: &crate::descriptor::host::HostHeap, slot: u32) {
    if !on() {
        return;
    }
    let k = key(heap, slot);
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(st) = g.as_mut()
        && let Some(e) = st.entries.remove(&k)
    {
        st.order.remove(&(e.due, k.0, k.1));
    }
}

/// Once a texture pass: wakes the worker for `frame`, makes the slots of
/// `heap` it found changed due, and writes the line every 5 s.
pub(crate) fn tick(heap: &crate::descriptor::host::HostHeap, frame: u64) {
    line();
    if !on() {
        return;
    }
    static WORKER: std::sync::Once = std::sync::Once::new();
    WORKER.call_once(|| {
        let _ = std::thread::Builder::new().name("victory-tex-recheck".into()).spawn(worker);
    });
    let me = heap as *const _ as usize;
    let mut slots = Vec::new();
    {
        let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let st = g.get_or_insert_with(State::default);
        if st.frame != frame {
            st.frame = frame;
            st.ticks += 1;
            TICK.notify_one();
        }
        if !st.changed.is_empty() {
            st.changed.retain(|&(h, slot)| {
                if h == me {
                    slots.push(slot);
                    false
                } else {
                    true
                }
            });
        }
    }
    if !slots.is_empty() {
        let n = heap.texture_slots_rechecked(&mut slots, frame);
        COUNTS[8].fetch_add(n as u64, Relaxed);
    }
}

/// All of `[at, at + len)`, through the kernel.
fn hash(at: u64, len: u64, read: &mut Vec<u8>) -> (u128, bool) {
    read.resize(CHUNK, 0);
    let mut print = 0u128;
    let mut zero = true;
    let mut done = 0u64;
    while done < len {
        let n = (len - done).min(CHUNK as u64) as usize;
        read[..n].fill(0);
        crate::host::copy_mapped(read.as_mut_ptr(), (at + done) as *const u8, n, false);
        print = print.rotate_left(37) ^ crate::texture::digest(&read[..n]).0;
        if zero {
            // SAFETY: plain bytes seen as words.
            let (head, words, tail) = unsafe { read[..n].align_to::<u64>() };
            zero = head.iter().all(|&b| b == 0) && words.iter().all(|&w| w == 0) && tail.iter().all(|&b| b == 0);
        }
        done += n as u64;
    }
    (print | 1, zero)
}

fn worker() {
    let mut read = Vec::new();
    let mut seen = 0u64;
    loop {
        let frame = {
            let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                let st = g.get_or_insert_with(State::default);
                if st.ticks != seen {
                    seen = st.ticks;
                    break st.frame;
                }
                g = TICK.wait(g).unwrap_or_else(|e| e.into_inner());
            }
        };
        let budget = budget();
        let mut spent = 0u64;
        loop {
            let (k, at, len, generation) = {
                let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
                let st = g.get_or_insert_with(State::default);
                let Some(&(due, h, slot)) = st.order.first() else { break };
                if due > frame {
                    break;
                }
                if spent >= budget {
                    COUNTS[5].fetch_add(1, Relaxed);
                    break;
                }
                st.order.pop_first();
                let Some(e) = st.entries.get(&(h, slot)) else { continue };
                ((h, slot), e.at, e.len, e.generation)
            };
            // Not all there (streaming in, or out): hashed all the same, the
            // missing pages as zeros; a change makes it due, and the pass
            // uploads what is there (its partial path).
            let there = crate::texture::mapped_bytes(at as *const u8, len as usize);
            if there < len {
                COUNTS[9].fetch_add(1, Relaxed);
            }
            let began = std::time::Instant::now();
            spent += len;
            COUNTS[0].fetch_add(1, Relaxed);
            COUNTS[2].fetch_add(len, Relaxed);
            let (p, zero) = hash(at, len, &mut read);
            let print = Some(p);
            COUNTS[3].fetch_add(began.elapsed().as_nanos() as u64, Relaxed);
            // Of an all-zero one, what the card writes in or near its memory
            // (asked before the list's lock: other locks of their own).
            let twins = (zero && LOGGED_ZERO.load(Relaxed) < LOGGED).then(|| {
                (
                    crate::texture::host_written_cover(at),
                    crate::resource::target_near(at, len),
                    crate::shadow::extent(at),
                )
            });
            let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
            let st = g.get_or_insert_with(State::default);
            let mut made_due = false;
            if let Some(e) = st.entries.get_mut(&k)
                && e.generation == generation
            {
                e.looks += 1;
                if zero && e.looks >= 3 {
                    COUNTS[14].fetch_add(1, Relaxed);
                    if !e.zero_logged && LOGGED_ZERO.fetch_add(1, Relaxed) < LOGGED {
                        e.zero_logged = true;
                        tracing::info!(
                            slot = k.1,
                            address = format_args!("{at:#x}"),
                            bytes = len,
                            mapped = there,
                            format = e.info.format,
                            width = e.info.width,
                            height = e.info.height,
                            mips = e.info.mips,
                            first_level = e.info.first,
                            looks = e.looks,
                            copied_into = ?twins.and_then(|t| t.0),
                            card_target_near = ?twins.and_then(|t| t.1),
                            in_buffer = ?twins.and_then(|t| t.2),
                            "texture rechecks: a texture picked by index still all zeros in title memory at a later look"
                        );
                    }
                }
                match print {
                    Some(p) if e.hash == 0 => {
                        e.hash = p;
                        e.step = 0;
                    }
                    Some(p) if e.hash != p => {
                        e.hash = p;
                        e.streak = e.streak.saturating_add(1);
                        COUNTS[1].fetch_add(1, Relaxed);
                        if e.streak >= NOISY {
                            // Changes at every look: backed off, not made due.
                            COUNTS[13].fetch_add(1, Relaxed);
                            e.step = e.step.saturating_add(1).min(STEPS.len() as u8);
                            if e.streak == NOISY && LOGGED_NOISY.fetch_add(1, Relaxed) < LOGGED {
                                tracing::info!(
                                    slot = k.1,
                                    address = format_args!("{at:#x}"),
                                    bytes = len,
                                    mapped = there,
                                    format = e.info.format,
                                    width = e.info.width,
                                    height = e.info.height,
                                    mips = e.info.mips,
                                    first_level = e.info.first,
                                    "texture rechecks: a texture picked by index changes at every look; no longer made due until a look finds it unchanged"
                                );
                            }
                        } else {
                            e.step = 0;
                            e.pending = true;
                            made_due = true;
                            COUNTS[4].fetch_add(len, Relaxed);
                        }
                    }
                    Some(_) => {
                        e.streak = 0;
                        e.step = e.step.saturating_add(1).min(STEPS.len() as u8);
                    }
                    None => {}
                }
                e.due = frame + wait(e.step);
                st.order.insert((e.due, k.0, k.1));
            }
            if made_due {
                st.changed.push(k);
            }
        }
    }
}

/// The "texture rechecks" line, every 5 s: while the switch is on, or when
/// the old recheck found untold changes.
fn line() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = crate::host::now_ms();
    let last = LAST.load(Relaxed);
    if now.saturating_sub(last) < 5000 || LAST.compare_exchange(last, now, Relaxed, Relaxed).is_err() {
        return;
    }
    let u: [u64; 7] = std::array::from_fn(|i| UNTOLD[i].swap(0, Relaxed));
    if !on() && u[0] == 0 {
        return;
    }
    let c: [u64; 15] = std::array::from_fn(|i| COUNTS[i].swap(0, Relaxed));
    let (tracked, noisy) = STATE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or((0, 0), |st| {
        (st.entries.len(), st.entries.values().filter(|e| e.streak >= NOISY).count())
    });
    tracing::info!(
        "texture rechecks, last 5 s: untold changes found by the sampled recheck {} ({} MB; at its looks 0-4: {:?}; {} since the start); VICTORY_TEX_RECHECK{}: {} textures tracked, {} hashed whole ({} MB, {:.1} ms), {} found changed ({} MB made due), {} slots made due: {} uploaded again ({} MB), {} false alarms (host copy held the bytes); {} changes not made due (noisy: changed {} looks in a row; {} such textures now); {} looks at memory not all mapped (hashed with zeros); {} all-zero looks 16+ frames after upload; {} frames that ran out of the {} MB/frame budget; {} refused (list full)",
        u[0],
        u[6] >> 20,
        &u[1..6],
        UNTOLD_TOTAL.load(Relaxed),
        if on() { "=1" } else { " off" },
        tracked,
        c[0],
        c[2] >> 20,
        c[3] as f64 / 1e6,
        c[1],
        c[4] >> 20,
        c[8],
        c[11],
        c[12] >> 20,
        c[10],
        c[13],
        NOISY,
        noisy,
        c[9],
        c[14],
        c[5],
        budget() >> 20,
        c[7],
    );
}
