//! (Fox fill-off-recorders) Descriptor slots of new tables made by threads of
//! ours (`VICTORY_FILL_ASIDE`; off by default).
//!
//! Above 8,000 draws a frame the title's five recording threads set the
//! frame, and most of their time in our calls is building descriptor tables:
//! about 0.4-0.6 us a slot, mostly making views and copying descriptors. The
//! card reads a heap's slots only when the list that names them runs (the
//! heap is indexed directly: what is in a slot at execution is what is read),
//! so the recorder keeps what must be decided at bind time (the view a record
//! names, read now; which slots; what is made readable, in list order) and
//! hands the making and copying of the descriptors to threads of ours. The
//! list's submission waits until its slots are made.
//!
//! Exact: the view each slot shows is the one the record named when the draw
//! was recorded (taken then, under the views' lock); the descriptor made from
//! it a little later shows the host's objects as they are then, which the
//! list holds until it is reset. `=2` (verify): the recorder makes every slot
//! as before, and a thread of ours makes the same slot again beside it and
//! compares the bytes (0 mismatches).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use windows::Win32::Graphics::Direct3D12::*;

use super::View;
use super::host::{HostHeap, Kept};

/// `VICTORY_FILL_ASIDE`: 0 off, 1 on, 2 verify.
pub(crate) fn mode() -> u8 {
    static ON: OnceLock<u8> = OnceLock::new();
    *ON.get_or_init(|| match std::env::var("VICTORY_FILL_ASIDE").as_deref().map(str::trim) {
        Ok("1") => 1,
        Ok("2" | "verify") => 2,
        _ => 0,
    })
}

/// Jobs handed over at once.
const BATCH: usize = 64;

/// What a view made in a thread's list views holds, once made: `None` inside
/// when the host could not make it (the slots then show a null view).
pub(crate) type KeptCell = Arc<OnceLock<Option<Kept>>>;

/// One piece of work.
pub(crate) enum Job {
    /// Make `view` in a recording thread's list views at `at` (its raw twin at
    /// `twin`); what it holds goes to `kept`.
    Make { view: View, at: usize, twin: usize, kept: KeptCell },
    /// Copy that view (`from`, its twin `from_twin`) into slot `index` of
    /// `heap`, with its stride word; the twin when the view has one and the
    /// heap keeps twins.
    Copy { heap: &'static HostHeap, index: u32, from: usize, from_twin: usize, twin: bool, stride: u32, view: View, kept: KeptCell },
    /// Make slot `index` of `heap` from `view` (`HostHeap::make_at`).
    MakeAt { heap: &'static HostHeap, index: u32, view: View },
    /// (Verify) Make `view` beside slot `index` of `heap` and compare.
    Check { heap: &'static HostHeap, index: u32, view: View },
}

// SAFETY: views, heaps of ours (free-threaded D3D12 objects) and addresses
// of descriptors in heaps that live as long as the process.
unsafe impl Send for Job {}

/// A host list's slots to be made.
pub(crate) struct Fills {
    worker: usize,
    pending: Mutex<Vec<Job>>,
    /// Jobs handed over and not done.
    out: AtomicUsize,
    done: (Mutex<()>, Condvar),
    /// What the slots made hold, until the list is reset.
    kept: Mutex<Vec<windows::Win32::Graphics::Direct3D12::ID3D12Resource>>,
}

impl Fills {
    pub(crate) fn new() -> Arc<Fills> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let worker = NEXT.fetch_add(1, Ordering::Relaxed) % threads();
        Arc::new(Fills { worker, pending: Mutex::new(Vec::new()), out: AtomicUsize::new(0), done: (Mutex::new(()), Condvar::new()), kept: Mutex::new(Vec::new()) })
    }

    /// A job; handed over with the ones before it every [`BATCH`].
    pub(crate) fn push(self: &Arc<Self>, job: Job) {
        let full = {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            pending.push(job);
            (pending.len() >= BATCH).then(|| std::mem::take(&mut *pending))
        };
        if let Some(jobs) = full {
            self.send(jobs);
        }
        COUNTS[0].fetch_add(1, Ordering::Relaxed);
    }

    fn send(self: &Arc<Self>, jobs: Vec<Job>) {
        if jobs.is_empty() {
            return;
        }
        self.out.fetch_add(jobs.len(), Ordering::AcqRel);
        let (queue, wake) = &workers()[self.worker];
        queue.lock().unwrap_or_else(|e| e.into_inner()).push_back((self.clone(), jobs));
        wake.notify_one();
    }

    /// What is pending handed over.
    pub(crate) fn flush(self: &Arc<Self>) {
        let jobs = std::mem::take(&mut *self.pending.lock().unwrap_or_else(|e| e.into_inner()));
        self.send(jobs);
    }

    /// Everything handed over and done (`why`: 0 at a submission, 1 at a
    /// recording thread's next list).
    pub(crate) fn wait(self: &Arc<Self>, why: usize) {
        self.flush();
        if self.out.load(Ordering::Acquire) == 0 {
            return;
        }
        let started = std::time::Instant::now();
        let mut guard = self.done.0.lock().unwrap_or_else(|e| e.into_inner());
        while self.out.load(Ordering::Acquire) != 0 {
            guard = self.done.1.wait(guard).unwrap_or_else(|e| e.into_inner());
        }
        COUNTS[3 + why].fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
    }
}

/// Jobs pushed, jobs done, worker ns; ns waited at submissions, at a thread's
/// next list; verify checks, mismatches; views the host could not make.
pub(crate) static COUNTS: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];

fn threads() -> usize {
    static N: OnceLock<usize> = OnceLock::new();
    *N.get_or_init(|| std::env::var("VICTORY_FILL_THREADS").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(6usize).clamp(1, 16))
}

type Queue = (Mutex<VecDeque<(Arc<Fills>, Vec<Job>)>>, Condvar);

fn workers() -> &'static [Queue] {
    static W: OnceLock<Vec<Queue>> = OnceLock::new();
    W.get_or_init(|| {
        let n = threads();
        let queues: Vec<Queue> = (0..n).map(|_| (Mutex::new(VecDeque::new()), Condvar::new())).collect();
        for i in 0..n {
            let _ = std::thread::Builder::new().name(format!("victory-fill-{i}")).spawn(move || work(i));
        }
        queues
    })
}

fn work(i: usize) {
    loop {
        let (fills, jobs) = {
            let (queue, wake) = &workers()[i];
            let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if let Some(item) = q.pop_front() {
                    break item;
                }
                q = wake.wait(q).unwrap_or_else(|e| e.into_inner());
            }
        };
        let started = std::time::Instant::now();
        let n = jobs.len();
        for job in jobs {
            run(&fills, job);
        }
        COUNTS[1].fetch_add(n as u64, Ordering::Relaxed);
        COUNTS[2].fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        if fills.out.fetch_sub(n, Ordering::AcqRel) == n {
            let _g = fills.done.0.lock().unwrap_or_else(|e| e.into_inner());
            fills.done.1.notify_all();
        }
    }
}

fn run(fills: &Fills, job: Job) {
    let handle = |ptr: usize| D3D12_CPU_DESCRIPTOR_HANDLE { ptr };
    match job {
        Job::Make { view, at, twin, kept } => {
            let made = super::host::make_twinned_pub(&view, handle(at), Some(handle(twin)));
            if made.is_none() {
                COUNTS[7].fetch_add(1, Ordering::Relaxed);
                null_made(2, &view);
                super::host::null_for_pub(&view, handle(at));
            }
            let _ = kept.set(made);
        }
        Job::Copy { heap, index, from, from_twin, twin, stride, view, kept } => {
            let made = kept.get().cloned().flatten().flatten();
            let ok = kept.get().is_some_and(Option::is_some);
            heap.fill_copied(index, handle(from), handle(from_twin), twin && ok, if ok { stride } else { 0 }, &view);
            if let Some(resource) = made {
                fills.kept.lock().unwrap_or_else(|e| e.into_inner()).push(resource);
            }
        }
        Job::MakeAt { heap, index, view } => heap.make_at(index, &view),
        Job::Check { heap, index, view } => {
            COUNTS[5].fetch_add(1, Ordering::Relaxed);
            if !heap.fill_check(index, &view) {
                COUNTS[6].fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// Views the host could not make, since the start, by where: `make_at` (any
/// caller, the old path's fallback included), a list view made on the
/// recorder, a list view made by a thread of ours.
pub(crate) static NULLS: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

/// A view the host could not make (`at`: 0, 1 or 2 as [`NULLS`]); the first
/// 40 are named.
pub(crate) fn null_made(at: usize, view: &View) {
    let n = NULLS[at].fetch_add(1, Ordering::Relaxed);
    static SAID: AtomicU64 = AtomicU64::new(0);
    if SAID.fetch_add(1, Ordering::Relaxed) < 40 {
        let thread = std::thread::current().name().unwrap_or("").to_owned();
        tracing::info!(
            at = ["make_at", "list view on the recorder", "list view on a fill thread"][at],
            so_far = n + 1,
            thread,
            ?view,
            "a view the host could not make: its slot shows a null view"
        );
    }
}

/// For the frame-rate line.
pub(crate) fn line(frames: f64) {
    let nulls: [u64; 3] = std::array::from_fn(|i| NULLS[i].load(Ordering::Relaxed));
    if nulls.iter().any(|&n| n > 0) {
        tracing::info!(
            "views the host could not make (null slots), since the start: {} by make_at (the old path's fallback among them), {} list views on the recorder, {} list views on fill threads",
            nulls[0],
            nulls[1],
            nulls[2]
        );
    }
    if mode() == 0 {
        return;
    }
    let f = frames.max(1.0);
    let c: [f64; 8] = std::array::from_fn(|i| COUNTS[i].swap(0, Ordering::Relaxed) as f64 / f);
    tracing::info!(
        "descriptor slots made by threads of ours (VICTORY_FILL_ASIDE={}) per frame: {:.0} jobs handed over, {:.0} done in {:.2} ms (on {} threads), \
         waited {:.2} ms at submissions and {:.2} ms at a recording thread's next list; {:.0} views the host could not make (null); \
         verify {:.0} slots made again and compared, {:.0} mismatches (must be 0)",
        mode(),
        c[0],
        c[1],
        c[2] / 1e6,
        threads(),
        c[3] / 1e6,
        c[4] / 1e6,
        c[7],
        c[5],
        c[6],
    );
}
