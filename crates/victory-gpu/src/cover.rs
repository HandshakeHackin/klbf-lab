//! (Fox draw-cost, `VICTORY_LIST_CALLS=1`, diagnostics) Work that covers less
//! than its target, for the bottom band of old frames: the scene's viewport
//! sizes against the targets drawn to, full-screen dispatches' grids against
//! the textures they write, the passes of either that cover less than their
//! target (by pipeline, both sizes), and the executions dropped with the
//! target they would have written. A line set every 5 s with the list-call
//! lines (`crate::drawmeter::line`).

use std::collections::HashMap;
use std::sync::Mutex;

use crate::descriptor::View;
use crate::object::This;

/// (covered w, h, target w, h) -> count: draws to targets 1024 wide or more.
static DRAWS: Mutex<Option<HashMap<(u32, u32, u32, u32), u64>>> = Mutex::new(None);
/// The same of direct dispatches that write a texture 1024 wide or more
/// (covered: groups times the group size).
static DISPATCHES: Mutex<Option<HashMap<(u32, u32, u32, u32), u64>>> = Mutex::new(None);
/// (pipeline, kind, covered w, h, target w, h) -> count: those covering less.
static SHORT: Mutex<Option<HashMap<(std::sync::Arc<str>, &'static str, u32, u32, u32, u32), u64>>> = Mutex::new(None);
/// (pipeline, why, target w, h) -> count: dropped executions.
static DROPPED: Mutex<Option<HashMap<(std::sync::Arc<str>, &'static str, u32, u32), u64>>> = Mutex::new(None);

fn on() -> bool {
    crate::drawmeter::on()
}

fn size_of_resource(resource: usize) -> Option<(u32, u32)> {
    let t = crate::resource::host_resource(resource as This)?;
    // SAFETY: a live resource.
    let d = unsafe { t.GetDesc() };
    (d.Dimension.0 >= 3).then_some((d.Width as u32, d.Height))
}

fn bump<K: std::hash::Hash + Eq>(map: &Mutex<Option<HashMap<K, u64>>>, key: K) {
    let mut m = map.lock().unwrap_or_else(|e| e.into_inner());
    let m = m.get_or_insert_with(HashMap::new);
    if m.len() < 4096 || m.contains_key(&key) {
        *m.entry(key).or_default() += 1;
    }
}

/// A draw ran to render target 0 `target` (title resource), with the
/// viewport and scissor given (first of each; none: the target's own).
pub(crate) fn draw(pipeline: This, target: Option<usize>, viewport: Option<(f32, f32, f32, f32)>, scissor: Option<(i32, i32, i32, i32)>) {
    if !on() {
        return;
    }
    let Some((tw, th)) = target.and_then(size_of_resource) else { return };
    if tw < 1024 {
        return;
    }
    // What the draw can reach: the viewport cut by the scissor, from the
    // top-left corner.
    let (mut w, mut h) = viewport.map_or((tw, th), |(x, y, vw, vh)| ((x + vw).max(0.0) as u32, (y + vh).max(0.0) as u32));
    if let Some((_, _, r, b)) = scissor {
        w = w.min(r.max(0) as u32);
        h = h.min(b.max(0) as u32);
    }
    let (w, h) = (w.min(tw), h.min(th));
    let vp = viewport.map_or((tw, th), |v| (v.2 as u32, v.3 as u32));
    bump(&DRAWS, (vp.0, vp.1, tw, th));
    if w < tw || h < th {
        bump(&SHORT, (crate::pipeline::label(pipeline), "draw", w, h, tw, th));
    }
}

/// The textures (title resources) a program writes through its tables'
/// unordered views: from what it declares, read now.
fn written(p: &crate::pipeline::host::HostPipeline, tables: &[(u8, u64)]) -> Vec<usize> {
    use victory_shader::native::{ResourceType, RootType};
    let mut out = Vec::new();
    for stage in &p.stages {
        for u in &stage.translated().used {
            if u.kind != ResourceType::Uav || u.root != RootType::Table {
                continue;
            }
            let Some(&(_, table)) = tables.iter().find(|r| r.0 == u.slot) else { continue };
            if let Some(View::Unordered(resource, ..)) = crate::descriptor::view(table + u64::from(u.table_offset_dwords) * 4)
                && !out.contains(&{ resource })
            {
                out.push(resource);
            }
        }
    }
    out
}

/// A direct dispatch of `groups` ran (`threads` a group; none: not known).
pub(crate) fn dispatch(
    pipeline: This,
    p: &crate::pipeline::host::HostPipeline,
    tables: &[(u8, u64)],
    groups: [u32; 3],
    threads: Option<[u32; 3]>,
) {
    if !on() {
        return;
    }
    let Some(threads) = threads else { return };
    let (w, h) = (groups[0].saturating_mul(threads[0]), groups[1].saturating_mul(threads[1]));
    for resource in written(p, tables) {
        let Some((tw, th)) = size_of_resource(resource) else { continue };
        if tw < 1024 {
            continue;
        }
        bump(&DISPATCHES, (w, h, tw, th));
        if w < tw || h < th {
            bump(&SHORT, (crate::pipeline::label(pipeline), "dispatch", w, h, tw, th));
        }
    }
}

/// An execution was dropped (`why`), with what it would have written:
/// render target 0, else the first texture it writes.
pub(crate) fn dropped(pipeline: This, why: &'static str, target: Option<usize>, p: Option<(&crate::pipeline::host::HostPipeline, &[(u8, u64)])>) {
    if !on() {
        return;
    }
    let target = target.or_else(|| p.and_then(|(p, t)| written(p, t).first().copied()));
    let (tw, th) = target.and_then(size_of_resource).unwrap_or((0, 0));
    bump(&DROPPED, (crate::pipeline::label(pipeline), why, tw, th));
}

fn take<K>(map: &Mutex<Option<HashMap<K, u64>>>) -> Vec<(K, u64)> {
    let mut v: Vec<(K, u64)> = map.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or_default().into_iter().collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.1));
    v
}

/// For the list-call lines.
pub(crate) fn line(frames: f64) {
    if !on() {
        return;
    }
    let f = frames.max(1.0);
    let sizes = |v: Vec<((u32, u32, u32, u32), u64)>| -> String {
        let all: u64 = v.iter().map(|e| e.1).sum();
        let text: Vec<String> = v
            .iter()
            .take(12)
            .map(|((w, h, tw, th), n)| format!("{w}x{h} on {tw}x{th} {:.1}", *n as f64 / f))
            .collect();
        format!("{:.0} a frame: {}", all as f64 / f, text.join(", "))
    };
    tracing::info!(
        "cover: draws to targets 1024 wide or more per frame by viewport size (VICTORY_LIST_CALLS): {}",
        sizes(take(&DRAWS))
    );
    tracing::info!(
        "cover: direct dispatches writing textures 1024 wide or more per frame by grid (groups x group size) on texture: {}",
        sizes(take(&DISPATCHES))
    );
    let short = take(&SHORT);
    let all: u64 = short.iter().map(|e| e.1).sum();
    let text: Vec<String> = short
        .iter()
        .take(16)
        .map(|((name, kind, w, h, tw, th), n)| format!("{name} {kind} {w}x{h} of {tw}x{th} {:.1}", *n as f64 / f))
        .collect();
    tracing::info!("cover: work covering less than its target per frame: {:.1}; {}", all as f64 / f, text.join(", "));
    let dropped = take(&DROPPED);
    let all: u64 = dropped.iter().map(|e| e.1).sum();
    let text: Vec<String> = dropped
        .iter()
        .take(16)
        .map(|((name, why, tw, th), n)| format!("{name} [{why}] target {tw}x{th} {:.1}", *n as f64 / f))
        .collect();
    tracing::info!("cover: executions dropped per frame (not the structure builder's): {:.1}; {}", all as f64 / f, text.join(", "));
}
