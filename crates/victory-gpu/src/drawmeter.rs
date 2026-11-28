//! (Fox draw-cost) What a draw's set-up on a recording thread spends besides
//! the parts `copystep`'s splits time: constant copies, hashing by site,
//! locks by site, allocations, and the barriers the host lists get, by who
//! asked (the title's own, translated, or ours). Diagnostics builds with
//! `VICTORY_LIST_CALLS=1` only; otherwise every entry point is one relaxed
//! load of a cached switch (and, for barriers, the call passed on).
//!
//! Also the two draw-cost switches' counters (`VICTORY_WORD_HASH`,
//! `VICTORY_CONST_DEDUP`), which count in every build.
//!
//! Counts are kept per thread and added to the shared ones every
//! [`FLUSH`] events, so the meter itself touches no shared line per event.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};

use windows::Win32::Graphics::Direct3D12::{D3D12_RESOURCE_BARRIER, ID3D12GraphicsCommandList};

/// Whether the meter runs (`VICTORY_LIST_CALLS=1`, diagnostics builds).
#[inline]
pub(crate) fn on() -> bool {
    #[cfg(feature = "diagnostics")]
    {
        crate::copystep::list_meter()
    }
    #[cfg(not(feature = "diagnostics"))]
    {
        false
    }
}

/// Counters, by index.
#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum C {
    /// Binds (`State::bind`): the "per draw" divisor.
    Binds,
    /// Constant buffer views in new tables: copied into the list's upload
    /// memory at submission (`defer_constants`): count, bytes, ns spent
    /// recording them (scratch, step, view).
    TableCbv,
    TableCbvBytes,
    TableCbvNs,
    /// Of them, the same title range (address, size) as one before in the
    /// list with no title write recorded since (exactly what
    /// `VICTORY_CONST_DEDUP` shares), and their bytes.
    TableCbvRepeat,
    TableCbvRepeatBytes,
    /// Of them, at a title address on 256 bytes.
    TableCbvAligned,
    /// Root views (`root_view`): asked, of them constant buffers off 256
    /// bytes in their host copy (a fill of up to 64 KiB), fills new / shared
    /// with an earlier one in the list, new fills' bytes, ns.
    RootViews,
    RootCbvOff,
    RootFillNew,
    RootFillShared,
    RootFillBytes,
    RootNs,
    /// Allocations made inside list calls (recording threads), and bytes.
    Allocs,
    AllocBytes,
    /// `VICTORY_CONST_DEDUP`: constants shared, bytes not copied, ns of the
    /// shared path (the verify checks: `host::CONST_CHECKS`).
    DedupHits,
    DedupBytes,
    DedupNs,
    /// `VICTORY_WORD_HASH`: `make_listed` lookups done once instead of three
    /// times; verify: the one lookup against the three, mismatches.
    WordLookups,
    WordChecks,
    WordMismatches,
    /// New tables: count, slots, constant buffer slots, by constant buffer
    /// slots (0, 1, 2, 3, 4 or more); lists with new tables; of the tables,
    /// the rest (all but the constant views) first seen in the list,
    /// seen before (and its non-constant slots), seen before in the same
    /// epoch, seen before with no constant view at all, the same as the
    /// last new table's at the root slot.
    RestTables,
    RestSlots,
    RestCbvSlots,
    RestCbv0,
    RestCbv1,
    RestCbv2,
    RestCbv3,
    RestCbv4,
    RestLists,
    RestFirst,
    RestRepeat,
    RestRepeatSlots,
    RestRepeatEpoch,
    RestRepeatNoCbv,
    RestSameAsLast,
    /// `make_listed` (a new table's slot copied from a view made before in
    /// the list): calls, sampled calls, hits, misses (the view made in the
    /// thread's heap), twin copies, twins made null, recordings that emptied
    /// the thread's views and the views they held; ns of the sampled calls
    /// by part: thread-local and recording, the map lookup, a miss's view
    /// record read (the shared views lock), a miss's descriptor made, up to
    /// the copy, the D3D copies, after (twins, stride word, holds).
    MlCalls,
    MlSampled,
    MlHits,
    MlMisses,
    MlTwinCopies,
    MlTwinCleared,
    MlClears,
    MlClearedViews,
    MlTls,
    MlLookup,
    MlViewAt,
    MlMake,
    MlBefore,
    MlCopy,
    MlAfter,
    /// `VICTORY_LIST_VIEWS_KEEP`: views taken from an earlier recording;
    /// verify: checked, mismatches.
    MlKeptHits,
    MlKeepChecks,
    MlKeepMismatches,
    /// The shared views lock (`descriptor::views_read`) inside list calls:
    /// takes, of them waited, ns waited.
    ViewsReads,
    ViewsWaited,
    ViewsWaitNs,
    /// New slots copied from a view made before: known ready in this epoch
    /// (no `readable`), made readable (sampled ns of those).
    ReadySkips,
    ReadyRuns,
    /// `make_listed` misses whose view another recording thread made this
    /// frame, before it, or this thread itself (in a recording it let go);
    /// `VICTORY_VIEWS_SHARED`: copied from the shared heap; verify: checked,
    /// mismatches.
    ShOtherFrame,
    ShOtherBefore,
    ShSelf,
    ShHits,
    ShChecks,
    ShMismatches,
    /// Texture shader views made (`texture_srv`, every path): the same host
    /// texture and description made before by another thread this frame, by
    /// this thread this frame, in the frame before; new; and with
    /// `VICTORY_VIEWS_BY_CONTENT` copied instead; verify: checked, mismatches.
    CtOtherFrame,
    CtSelfFrame,
    CtBefore,
    CtNew,
    CtCopied,
    CtChecks,
    CtMismatches,
    /// `VICTORY_READY_BY_VIEW`: views looked up, hits; verify: hits whose
    /// whole path changed the epoch, queued steps, the tested depth.
    RvLooked,
    RvHits,
    RvEpoch,
    RvSteps,
    RvDepth,
    /// `buffer_srv` (buffer shader views, placed ones among them) split, one
    /// call in eight: calls timed, ns finding the host copy, the view's own
    /// copy (`view_home`), the description, the descriptor (made or copied
    /// by content), the raw twin.
    BsSampled,
    BsFind,
    BsHome,
    BsDesc,
    BsCreate,
    BsTwin,
}

const COUNTERS: usize = C::BsTwin as usize + 1;

/// Hash sites: per site calls, bytes, sampled ns and sampled calls.
#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum H {
    /// `State::tables`: a table's key, its records (12 bytes a slot) folded.
    TableKey,
    /// `State::fresh_tables` lookups and inserts ((u64, u32, u64) keys).
    FreshTables,
    /// `State::entry_memo` lookups and inserts (handle, 32 descriptor bytes,
    /// kind, stores, tested depth).
    EntryMemo,
    /// `ListViews::made` lookups (12-byte records), per lookup.
    ListViews,
    /// `State::view_memo` (address, kind).
    ViewMemo,
    /// The list's `refreshed` / `gpu_written` / `filled` sets (3 words).
    ListSets,
}

const HASH_SITES: usize = H::ListSets as usize + 1;
const HASH_NAMES: [&str; HASH_SITES] = [
    "table keys (records folded)",
    "fresh-table map",
    "entry memo map",
    "list-views map",
    "root-view memo map",
    "list refresh/fill sets",
];

/// Lock sites taken per draw.
#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum L {
    /// A title list's draw state (`with_state`), per list call.
    ListState,
    /// A host list's deferred steps (`HostList::deferred`).
    Deferred,
    /// A host list's scratch chunks.
    Scratch,
    /// A heap's dynamic slots (`HostHeap::dynamic`), shared by all threads.
    Dynamic,
    /// A dynamic slot's held resource (`HostHeap::keep`).
    SlotKeep,
    /// The objects let go, shared by all threads (`retire_resource`).
    Retired,
}

const LOCK_SITES: usize = L::Retired as usize + 1;
const LOCK_NAMES: [&str; LOCK_SITES] =
    ["list draw state", "list steps (deferred)", "list scratch", "heap dynamic slots (shared)", "slot holds", "retired objects (shared)"];

/// Per thread: counters, hashes (calls, bytes, sampled ns, sampled calls),
/// locks (takes, contended, ns waited), events since the last flush.
struct Local {
    c: [u64; COUNTERS],
    k: [[u64; KIND_FIELDS]; KINDS],
    h: [[u64; 4]; HASH_SITES],
    l: [[u64; 3]; LOCK_SITES],
    n: u32,
}

const FLUSH: u32 = 512;

static SHARED_C: [AtomicU64; COUNTERS] = [const { AtomicU64::new(0) }; COUNTERS];
static SHARED_K: [[AtomicU64; KIND_FIELDS]; KINDS] = [const { [const { AtomicU64::new(0) }; KIND_FIELDS] }; KINDS];

/// (`make_listed` misses by kind) Kinds of view: texture view, buffer view,
/// texture unordered, buffer unordered, placed texture, placed buffer view,
/// placed buffer unordered, sampler, other.
pub(crate) const KINDS: usize = 9;
const KIND_NAMES: [&str; KINDS] =
    ["texture SRV", "buffer SRV", "texture UAV", "buffer UAV", "placed texture", "placed buffer SRV", "placed buffer UAV", "sampler", "other"];
/// Per kind: misses; the same view (its description, whatever record) made
/// before by another thread this frame, by this thread this frame, in the
/// frame before, or new; descriptor makings timed (one in eight), their ns;
/// and with `VICTORY_VIEWS_BY_CONTENT` copied instead of made.
pub(crate) const KIND_FIELDS: usize = 8;

/// Adds `by` to field `field` of kind `kind` (meter on).
#[inline]
pub(crate) fn kind_add(kind: usize, field: usize, by: u64) {
    if on() {
        with_local(|l| l.k[kind.min(KINDS - 1)][field] += by);
    }
}
static SHARED_H: [[AtomicU64; 4]; HASH_SITES] = [const { [const { AtomicU64::new(0) }; 4] }; HASH_SITES];
static SHARED_L: [[AtomicU64; 3]; LOCK_SITES] = [const { [const { AtomicU64::new(0) }; 3] }; LOCK_SITES];

thread_local! {
    static LOCAL: RefCell<Local> = const {
        RefCell::new(Local { c: [0; COUNTERS], k: [[0; KIND_FIELDS]; KINDS], h: [[0; 4]; HASH_SITES], l: [[0; 3]; LOCK_SITES], n: 0 })
    };
    /// Inside a list call: allocations are counted.
    static IN_CALL: Cell<bool> = const { Cell::new(false) };
    /// Allocations seen in list calls, not yet in `LOCAL` (the allocator
    /// must not borrow `LOCAL`).
    static ALLOCS: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
    /// One in [`SAMPLE`] hashes is timed.
    static TICK: Cell<u32> = const { Cell::new(0) };
}

fn with_local(f: impl FnOnce(&mut Local)) {
    let _ = LOCAL.try_with(|l| {
        let Ok(mut l) = l.try_borrow_mut() else { return };
        f(&mut l);
        l.n += 1;
        if l.n >= FLUSH {
            flush(&mut l);
        }
    });
}

fn flush(l: &mut Local) {
    l.n = 0;
    for (s, v) in SHARED_C.iter().zip(l.c.iter_mut()) {
        if *v != 0 {
            s.fetch_add(std::mem::take(v), Relaxed);
        }
    }
    for (s, v) in SHARED_K.iter().zip(l.k.iter_mut()) {
        for (s, v) in s.iter().zip(v.iter_mut()) {
            if *v != 0 {
                s.fetch_add(std::mem::take(v), Relaxed);
            }
        }
    }
    for (s, v) in SHARED_H.iter().zip(l.h.iter_mut()) {
        for (s, v) in s.iter().zip(v.iter_mut()) {
            if *v != 0 {
                s.fetch_add(std::mem::take(v), Relaxed);
            }
        }
    }
    for (s, v) in SHARED_L.iter().zip(l.l.iter_mut()) {
        for (s, v) in s.iter().zip(v.iter_mut()) {
            if *v != 0 {
                s.fetch_add(std::mem::take(v), Relaxed);
            }
        }
    }
}

/// Adds `by` to counter `c` (meter on only).
#[inline]
pub(crate) fn add(c: C, by: u64) {
    if on() {
        with_local(|l| l.c[c as usize] += by);
    }
}

/// Adds `by` to counter `c` whether the meter runs or not (the switches'
/// own counters).
#[inline]
pub(crate) fn count(c: C, by: u64) {
    with_local(|l| l.c[c as usize] += by);
}

/// (`VICTORY_LIST_CALLS`) One call in [`SPLIT_SAMPLE`] timed in parts: the
/// parts' ns go to their counters, [`C::MlSampled`] counts the calls.
pub(crate) struct Split(Option<std::time::Instant>);

const SPLIT_SAMPLE: u32 = 8;

impl Split {
    /// As [`Split::sampled`], the timed calls counted in `sampled`.
    #[inline]
    pub(crate) fn sampled_as(sampled: C) -> Split {
        if !on() {
            return Split(None);
        }
        let timed = TICK.with(|t| {
            let n = t.get().wrapping_add(1);
            t.set(n);
            n % SPLIT_SAMPLE == 0
        });
        if !timed {
            return Split(None);
        }
        add(sampled, 1);
        Split(Some(std::time::Instant::now()))
    }

    #[inline]
    pub(crate) fn sampled() -> Split {
        if !on() {
            return Split(None);
        }
        let timed = TICK.with(|t| {
            let n = t.get().wrapping_add(1);
            t.set(n);
            n % SPLIT_SAMPLE == 0
        });
        if !timed {
            return Split(None);
        }
        add(C::MlSampled, 1);
        Split(Some(std::time::Instant::now()))
    }

    #[inline]
    pub(crate) fn lap(&mut self, part: C) {
        if let Some(was) = self.0 {
            let now = std::time::Instant::now();
            add(part, (now - was).as_nanos() as u64);
            self.0 = Some(now);
        }
    }
}

/// Inside a list call (allocations and the views lock count there).
#[inline]
pub(crate) fn in_call() -> bool {
    IN_CALL.try_with(Cell::get).unwrap_or(false)
}

/// Hashes timed: one in this many.
const SAMPLE: u32 = 16;

/// A hash of `bytes` at site `site`; `hash` is timed once in [`SAMPLE`]
/// calls (it should compute the site's hash of its key, and nothing else).
#[inline]
pub(crate) fn hashed(site: H, bytes: u64, hash: impl FnOnce() -> u64) {
    if !on() {
        return;
    }
    let timed = TICK.with(|t| {
        let n = t.get().wrapping_add(1);
        t.set(n);
        n % SAMPLE == 0
    });
    let ns = if timed {
        let started = std::time::Instant::now();
        std::hint::black_box(hash());
        started.elapsed().as_nanos() as u64
    } else {
        0
    };
    with_local(|l| {
        let h = &mut l.h[site as usize];
        h[0] += 1;
        h[1] += bytes;
        if timed {
            h[2] += ns;
            h[3] += 1;
        }
    });
}

/// The hash `S` gives `key` (for [`hashed`]).
#[inline]
pub(crate) fn hash_with<S: std::hash::BuildHasher + Default, K: std::hash::Hash>(key: &K) -> u64 {
    S::default().hash_one(key)
}

/// `mutex` locked, the take and any wait counted at `site` (meter on).
#[inline]
pub(crate) fn lock<'a, T>(site: L, mutex: &'a std::sync::Mutex<T>) -> std::sync::MutexGuard<'a, T> {
    if !on() {
        return mutex.lock().unwrap_or_else(|e| e.into_inner());
    }
    let (guard, waited) = match mutex.try_lock() {
        Ok(guard) => (guard, None),
        Err(std::sync::TryLockError::Poisoned(e)) => (e.into_inner(), None),
        Err(std::sync::TryLockError::WouldBlock) => {
            let started = std::time::Instant::now();
            let guard = mutex.lock().unwrap_or_else(|e| e.into_inner());
            (guard, Some(started.elapsed().as_nanos() as u64))
        }
    };
    with_local(|l| {
        let s = &mut l.l[site as usize];
        s[0] += 1;
        if let Some(ns) = waited {
            s[1] += 1;
            s[2] += ns;
        }
    });
    guard
}

/// A list call begins (`list::with_state`): allocations count from here.
#[inline]
pub(crate) fn call_begins() {
    if on() {
        let _ = IN_CALL.try_with(|c| c.set(true));
    }
}

/// The list call ends.
#[inline]
pub(crate) fn call_ends() {
    if !on() {
        return;
    }
    let _ = IN_CALL.try_with(|c| c.set(false));
    let (n, bytes) = ALLOCS.try_with(|a| a.replace((0, 0))).unwrap_or((0, 0));
    if n != 0 {
        with_local(|l| {
            l.c[C::Allocs as usize] += n;
            l.c[C::AllocBytes as usize] += bytes;
        });
    }
}

/// From the counting allocator: a block of `size` bytes was made. Touches
/// thread-local cells only (no allocation, no borrow).
#[inline]
pub(crate) fn allocated(size: usize) {
    if IN_CALL.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCS.try_with(|a| {
            let (n, b) = a.get();
            a.set((n + 1, b + size as u64));
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Barriers by the place that records them.

/// Sites: the caller's location (its address), barrier calls, barriers,
/// transitions, of them with the same state before and after (none should).
const SITES: usize = 256;
static SITE_AT: [AtomicUsize; SITES] = [const { AtomicUsize::new(0) }; SITES];
static SITE_N: [[AtomicU64; 3]; SITES] = [const { [const { AtomicU64::new(0) }; 3] }; SITES];
/// The title's own barriers, translated (`list::barrier`): calls, barriers,
/// of them the "wait for buffer writes" one we add.
static TITLE: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

/// Barriers recorded through [`CountBarriers`] by site.
fn note_barriers(at: &'static std::panic::Location<'static>, barriers: &[D3D12_RESOURCE_BARRIER]) {
    let key = std::ptr::from_ref(at) as usize;
    let start = (key >> 3).wrapping_mul(0x9E37_79B9) % SITES;
    let transitions = barriers.iter().filter(|b| b.Type.0 == 0).count() as u64;
    for i in 0..SITES {
        let s = (start + i) % SITES;
        let was = SITE_AT[s].load(Relaxed);
        if was == key || (was == 0 && SITE_AT[s].compare_exchange(0, key, Relaxed, Relaxed).map_or_else(|v| v == key, |_| true)) {
            SITE_N[s][0].fetch_add(1, Relaxed);
            SITE_N[s][1].fetch_add(barriers.len() as u64, Relaxed);
            SITE_N[s][2].fetch_add(transitions, Relaxed);
            return;
        }
    }
}

/// `ResourceBarrier` with the barriers counted by the caller's place
/// (meter on); the same call either way.
pub(crate) trait CountBarriers {
    /// # Safety
    /// As `ResourceBarrier`.
    unsafe fn barriers_counted(&self, barriers: &[D3D12_RESOURCE_BARRIER]);
}

impl CountBarriers for ID3D12GraphicsCommandList {
    #[track_caller]
    #[inline]
    unsafe fn barriers_counted(&self, barriers: &[D3D12_RESOURCE_BARRIER]) {
        if on() {
            note_barriers(std::panic::Location::caller(), barriers);
        }
        // SAFETY: the caller's contract, passed on.
        unsafe { self.ResourceBarrier(barriers) };
    }
}

/// The title's barrier call (`count` of its own) became `host` barriers,
/// `buffers` of them the wait for buffer writes we add.
pub(crate) fn title_barriers(count: u32, host: usize, buffers: bool) {
    if on() {
        TITLE[0].fetch_add(u64::from(count), Relaxed);
        TITLE[1].fetch_add(host as u64, Relaxed);
        TITLE[2].fetch_add(u64::from(buffers), Relaxed);
    }
}

// ---------------------------------------------------------------------------------------------
// The lines.

/// For the frame lines (with copystep's list-call lines).
pub(crate) fn line(frames: f64) {
    if !on() {
        return;
    }
    let f = frames.max(1.0);
    let c: [u64; COUNTERS] = std::array::from_fn(|i| SHARED_C[i].swap(0, Relaxed));
    let binds = (c[C::Binds as usize] as f64).max(1.0);
    let pf = |i: C| c[i as usize] as f64 / f;
    let us_per_bind = |ns: u64| ns as f64 / 1e3 / binds;
    tracing::info!(
        "constant copies per frame, recording threads (VICTORY_LIST_CALLS): {:.0} binds; table constant views {:.0} ({:.2} MB, {:.2} ms recording them, \
         {:.2} us a bind), of them the same title range again since the list's last title write {:.0} ({:.1} %, {:.2} MB), at a title address on 256 bytes {:.0}; \
         root views {:.0}, of them constant buffers off 256 bytes in their copy {:.0}: fills new {:.0} ({:.2} MB), shared {:.0}; root views {:.2} ms ({:.2} us a bind)",
        binds / f,
        pf(C::TableCbv),
        pf(C::TableCbvBytes) / 1e6,
        pf(C::TableCbvNs) / 1e6,
        us_per_bind(c[C::TableCbvNs as usize]),
        pf(C::TableCbvRepeat),
        100.0 * c[C::TableCbvRepeat as usize] as f64 / (c[C::TableCbv as usize] as f64).max(1.0),
        pf(C::TableCbvRepeatBytes) / 1e6,
        pf(C::TableCbvAligned),
        pf(C::RootViews),
        pf(C::RootCbvOff),
        pf(C::RootFillNew),
        pf(C::RootFillBytes) / 1e6,
        pf(C::RootFillShared),
        pf(C::RootNs) / 1e6,
        us_per_bind(c[C::RootNs as usize]),
    );
    let h: Vec<[u64; 4]> = (0..HASH_SITES).map(|i| std::array::from_fn(|k| SHARED_H[i][k].swap(0, Relaxed))).collect();
    let mut total_ns = 0.0;
    let sites: Vec<String> = h
        .iter()
        .enumerate()
        .map(|(i, v)| {
            // Sampled: the sampled hashes' mean times all of them.
            let ns = if v[3] > 0 { v[2] as f64 / v[3] as f64 * v[0] as f64 } else { 0.0 };
            total_ns += ns;
            format!(
                "{} {:.0} hashes, {:.2} MB, {:.2} ms ({:.0} ns each, {:.2} us a bind)",
                HASH_NAMES[i],
                v[0] as f64 / f,
                v[1] as f64 / 1e6 / f,
                ns / 1e6 / f,
                if v[3] > 0 { v[2] as f64 / v[3] as f64 } else { 0.0 },
                ns / 1e3 / binds
            )
        })
        .collect();
    tracing::info!(
        "hashing per frame, recording threads (VICTORY_LIST_CALLS; one in {SAMPLE} timed, the hash alone, not the map's probe): {:.2} ms ({:.2} us a bind); {}",
        total_ns / 1e6 / f,
        total_ns / 1e3 / binds,
        sites.join("; ")
    );
    let l: Vec<[u64; 3]> = (0..LOCK_SITES).map(|i| std::array::from_fn(|k| SHARED_L[i][k].swap(0, Relaxed))).collect();
    let locks: Vec<String> = l
        .iter()
        .enumerate()
        .map(|(i, v)| {
            format!(
                "{} {:.0} takes ({:.2} a bind), {:.0} waited, {:.3} ms waiting ({:.3} us a bind)",
                LOCK_NAMES[i],
                v[0] as f64 / f,
                v[0] as f64 / binds,
                v[1] as f64 / f,
                v[2] as f64 / 1e6 / f,
                v[2] as f64 / 1e3 / binds
            )
        })
        .collect();
    tracing::info!("locks per frame, all threads that take them (VICTORY_LIST_CALLS): {}", locks.join("; "));
    tracing::info!(
        "allocations in list calls per frame, recording threads (VICTORY_LIST_CALLS): {:.0} ({:.2} a bind), {:.2} MB",
        pf(C::Allocs),
        c[C::Allocs as usize] as f64 / binds,
        pf(C::AllocBytes) / 1e6
    );
    // Barriers.
    let title: [u64; 3] = std::array::from_fn(|i| TITLE[i].swap(0, Relaxed));
    let mut rows: Vec<(String, [u64; 3])> = Vec::new();
    for s in 0..SITES {
        let at = SITE_AT[s].load(Relaxed);
        if at == 0 {
            continue;
        }
        let n: [u64; 3] = std::array::from_fn(|k| SITE_N[s][k].swap(0, Relaxed));
        if n[0] == 0 {
            continue;
        }
        // SAFETY: the key is the address of a `'static` location.
        let at = unsafe { &*(at as *const std::panic::Location<'static>) };
        let file = at.file().rsplit(['/', '\\']).next().unwrap_or(at.file());
        rows.push((format!("{file}:{}", at.line()), n));
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.1[1]));
    let all: u64 = rows.iter().map(|r| r.1[1]).sum();
    let title_site = title[1];
    let top: Vec<String> = rows
        .iter()
        .take(14)
        .map(|(s, n)| format!("{s} {:.0} barriers in {:.0} calls ({:.0} transitions)", n[1] as f64 / f, n[0] as f64 / f, n[2] as f64 / f))
        .collect();
    tracing::info!(
        "barriers recorded per frame, all host lists (VICTORY_LIST_CALLS): {:.0}, the title's own {:.0} (from {:.0} of its barriers; {:.0} calls with the wait for buffer writes we add), ours {:.0}; by site: {}",
        all as f64 / f,
        title_site as f64 / f,
        title[0] as f64 / f,
        title[2] as f64 / f,
        (all - title_site.min(all)) as f64 / f,
        top.join(", ")
    );
    let t = (c[C::RestTables as usize] as f64).max(1.0);
    let share = |i: C| 100.0 * c[i as usize] as f64 / t;
    tracing::info!(
        "new tables' rest per frame (VICTORY_LIST_CALLS; rest = every slot's record but the constant buffer views, and where those are): {:.0} new tables          in {:.0} lists, {:.0} slots, {:.0} of them constant views; by constant views 0: {:.1} %, 1: {:.1} %, 2: {:.1} %, 3: {:.1} %, 4+: {:.1} %;          the rest seen before in the list {:.1} % ({:.0} non-constant slots), of them in the same epoch {:.1} %, with no constant view {:.1} %;          the same rest as the last new table at its root slot {:.1} %; distinct rests {:.0} ({:.1} a list)",
        pf(C::RestTables),
        pf(C::RestLists),
        pf(C::RestSlots),
        pf(C::RestCbvSlots),
        share(C::RestCbv0),
        share(C::RestCbv1),
        share(C::RestCbv2),
        share(C::RestCbv3),
        share(C::RestCbv4),
        share(C::RestRepeat),
        pf(C::RestRepeatSlots),
        share(C::RestRepeatEpoch),
        share(C::RestRepeatNoCbv),
        share(C::RestSameAsLast),
        pf(C::RestFirst),
        c[C::RestFirst as usize] as f64 / (c[C::RestLists as usize] as f64).max(1.0),
    );
    {
        let calls = (c[C::MlCalls as usize] as f64).max(1.0);
        let sampled = (c[C::MlSampled as usize] as f64).max(1.0);
        let ns = |i: C| c[i as usize] as f64 / sampled;
        let parts = [
            ("thread-local and recording", C::MlTls),
            ("map lookup (hits)", C::MlLookup),
            ("a miss: the view's record (shared views lock)", C::MlViewAt),
            ("a miss: its descriptor made", C::MlMake),
            ("up to the copy", C::MlBefore),
            ("the D3D copies", C::MlCopy),
            ("after (twins, stride, holds)", C::MlAfter),
        ];
        let total: f64 = parts.iter().map(|p| ns(p.1)).sum();
        let text: Vec<String> = parts.iter().map(|(n, i)| format!("{n} {:.0} ns", ns(*i))).collect();
        tracing::info!(
            "make_listed per slot (VICTORY_LIST_CALLS; one call in {SPLIT_SAMPLE} timed, ns a call): {:.0} calls a frame, {:.0} ns a call ({:.2} ms a frame); \
             hits {:.1} %, misses {:.1} %; twin copies {:.0}, twins made null {:.0}; {:.0} recordings emptied the thread's views ({:.0} views); {}; \
             views taken from an earlier recording (VICTORY_LIST_VIEWS_KEEP) {:.0}, verify {:.0} checked, {:.0} mismatches (must be 0)",
            calls / f,
            total,
            total * calls / 1e6 / f,
            100.0 * c[C::MlHits as usize] as f64 / calls,
            100.0 * c[C::MlMisses as usize] as f64 / calls,
            pf(C::MlTwinCopies),
            pf(C::MlTwinCleared),
            pf(C::MlClears),
            pf(C::MlClearedViews),
            text.join(", "),
            pf(C::MlKeptHits),
            pf(C::MlKeepChecks),
            pf(C::MlKeepMismatches),
        );
        tracing::info!(
            "make_listed misses per frame by who made the view before (VICTORY_LIST_CALLS): {:.0} misses, another recording thread this frame {:.0} ({:.1} %), \
             another thread a frame before {:.0} ({:.1} %), this thread (a recording let go) {:.0}; copied from the shared heap (VICTORY_VIEWS_SHARED) {:.0}, \
             verify {:.0} checked, {:.0} mismatches (must be 0)",
            pf(C::MlMisses),
            pf(C::ShOtherFrame),
            100.0 * c[C::ShOtherFrame as usize] as f64 / (c[C::MlMisses as usize] as f64).max(1.0),
            pf(C::ShOtherBefore),
            100.0 * c[C::ShOtherBefore as usize] as f64 / (c[C::MlMisses as usize] as f64).max(1.0),
            pf(C::ShSelf),
            pf(C::ShHits),
            pf(C::ShChecks),
            pf(C::ShMismatches),
        );
        let made = (c[C::CtOtherFrame as usize] + c[C::CtSelfFrame as usize] + c[C::CtBefore as usize] + c[C::CtNew as usize]) as f64;
        let pc = |i: C| 100.0 * c[i as usize] as f64 / made.max(1.0);
        tracing::info!(
            "texture views made per frame, all threads, by what they are made from (host texture and view description; VICTORY_LIST_CALLS): {:.0}; \
             the same made before by another thread this frame {:.0} ({:.1} %), by the same thread this frame {:.0} ({:.1} %), a frame before {:.0} ({:.1} %), \
             new {:.0} ({:.1} %); copied instead (VICTORY_VIEWS_BY_CONTENT) {:.0}, verify {:.0} checked, {:.0} mismatches (must be 0)",
            made / f,
            pf(C::CtOtherFrame),
            pc(C::CtOtherFrame),
            pf(C::CtSelfFrame),
            pc(C::CtSelfFrame),
            pf(C::CtBefore),
            pc(C::CtBefore),
            pf(C::CtNew),
            pc(C::CtNew),
            pf(C::CtCopied),
            pf(C::CtChecks),
            pf(C::CtMismatches),
        );
        {
            let n = (c[C::BsSampled as usize] as f64).max(1.0);
            let ns = |i: C| c[i as usize] as f64 / n;
            tracing::info!(
                "buffer shader views made per call (VICTORY_LIST_CALLS; one in {SPLIT_SAMPLE} timed): finding the host copy {:.0} ns, the view's own copy {:.0} ns,                  the description {:.0} ns, the descriptor (made or copied by content) {:.0} ns, the raw twin {:.0} ns",
                ns(C::BsFind),
                ns(C::BsHome),
                ns(C::BsDesc),
                ns(C::BsCreate),
                ns(C::BsTwin),
            );
        }
        tracing::info!(
            "shared views lock inside list calls per frame (VICTORY_LIST_CALLS): {:.0} takes ({:.2} a bind), {:.0} waited, {:.3} ms waiting ({:.3} us a bind); \
             new slots from views made before: {:.0} known ready in the epoch, {:.0} made readable",
            pf(C::ViewsReads),
            c[C::ViewsReads as usize] as f64 / binds,
            pf(C::ViewsWaited),
            pf(C::ViewsWaitNs) / 1e6,
            c[C::ViewsWaitNs as usize] as f64 / 1e3 / binds,
            pf(C::ReadySkips),
            pf(C::ReadyRuns),
        );
    }
    #[cfg(feature = "diagnostics")]
    crate::cover::line(frames);
    #[cfg(feature = "diagnostics")]
    crate::draw::tex_pass_line(frames);
    {
        let k: Vec<[u64; KIND_FIELDS]> = (0..KINDS).map(|i| std::array::from_fn(|j| SHARED_K[i][j].swap(0, Relaxed))).collect();
        let all: u64 = k.iter().map(|v| v[0]).sum();
        let rows: Vec<String> = k
            .iter()
            .enumerate()
            .filter(|(_, v)| v[0] > 0)
            .map(|(i, v)| {
                let n = (v[0] as f64).max(1.0);
                let pc = |j: usize| 100.0 * v[j] as f64 / n;
                format!(
                    "{} {:.0} ({:.1} %): same view made before by another thread this frame {:.1} %, by this thread {:.1} %, a frame before {:.1} %, new {:.1} %; \
                     making {:.0} ns; copied by content {:.0}",
                    KIND_NAMES[i],
                    v[0] as f64 / f,
                    100.0 * v[0] as f64 / (all as f64).max(1.0),
                    pc(1),
                    pc(2),
                    pc(3),
                    pc(4),
                    if v[5] > 0 { v[6] as f64 / v[5] as f64 } else { 0.0 },
                    v[7] as f64 / f,
                )
            })
            .collect();
        tracing::info!("make_listed misses per frame by kind of view (VICTORY_LIST_CALLS): {:.0}; {}", all as f64 / f, rows.join("; "));
    }
    let sw = |i: C| c[i as usize] as f64 / f;
    if crate::draw::const_dedup() != 0 {
        tracing::info!(
            "constants shared in lists (VICTORY_CONST_DEDUP={}) per frame: {:.0} table constant views took an earlier fill's bytes, {:.2} MB not copied, \
             {:.2} ms on the shared path; verify: {:.0} checked at submission, {:.0} mismatches (must be 0)",
            crate::draw::const_dedup(),
            sw(C::DedupHits),
            sw(C::DedupBytes) / 1e6,
            sw(C::DedupNs) / 1e6,
            crate::host::CONST_CHECKS[0].swap(0, Relaxed) as f64 / f,
            crate::host::CONST_CHECKS[1].swap(0, Relaxed) as f64 / f
        );
    }
    if crate::draw::ready_by_view() != 0 {
        tracing::info!(
            "ready by view (VICTORY_READY_BY_VIEW={}) per frame: {:.0} views looked up, {:.0} done earlier in the list with nothing changed ({:.1} %); \
             verify, hits the whole path changed something on: epoch {:.1}, steps {:.1}, tested depth {:.1} (all must be 0)",
            crate::draw::ready_by_view(),
            sw(C::RvLooked),
            sw(C::RvHits),
            100.0 * c[C::RvHits as usize] as f64 / (c[C::RvLooked as usize] as f64).max(1.0),
            sw(C::RvEpoch),
            sw(C::RvSteps),
            sw(C::RvDepth),
        );
    }
    if crate::host::word_hash() != 0 {
        tracing::info!(
            "word hashing (VICTORY_WORD_HASH={}) per frame: {:.0} list-view lookups done once instead of three times; verify: {:.0} checked, {:.0} mismatches (must be 0)",
            crate::host::word_hash(),
            sw(C::WordLookups),
            sw(C::WordChecks),
            sw(C::WordMismatches)
        );
    }
}
