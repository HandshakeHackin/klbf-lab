//! The console's audio DSP (`chad.dll`): streams that convolve blocks of
//! float samples with an impulse response (reverb), or run other block
//! transforms, on a coprocessor.
//!
//! A title connects once with a memory block for the DSP, activates
//! streams, and submits one command per block (gains, an input and an
//! output buffer). Activation hands back a status block whose second word is
//! the ticket of the last completed command and whose fourth is its result;
//! the title checks it one block later and treats a late command as an
//! overrun. Commands complete when submitted here.
//!
//! `VICTORY_DSP_CONVOLVE=1` (off by default) computes mono 1024-frame
//! convolutions ([`convolve`], from trustno1); `VICTORY_DSP_DRY=1` passes
//! other blocks' input through. Otherwise:
//! the transforms themselves are not computed: the hardware takes impulse
//! responses already in its own partitioned frequency-domain layout, which
//! is not known. A convolution passes its input through unchanged (voices
//! filtered through one, such as a phone line, stay audible; idea from
//! zzz); other transforms output silence. `KLBF_PHONE_SILENT=1` makes
//! convolutions silent again.
// shortcut: no convolution or block transform; convolution is a passthrough,
// the rest are zeros. Needs the coprocessor's impulse response and spectrum
// layouts to do better.
#![allow(non_snake_case)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use super::xasync::E_INVALIDARG;
use super::xgameruntime::{HRESULT, S_OK};
use crate::{HleModule, hle_export};

/// (ported from trustno1 eca45ac, PR #77) The mono convolution on the CPU.
mod convolve;

/// `VICTORY_DSP_CONVOLVE=1` (off by default): mono convolutions of
/// 1024-frame blocks are computed (TVs, phone calls, reverb). Read at each
/// activation (rare).
fn convolve_on() -> bool {
    let on = std::env::var("VICTORY_DSP_CONVOLVE").is_ok_and(|v| v == "1");
    if on {
        static SAID: std::sync::Once = std::sync::Once::new();
        SAID.call_once(|| tracing::info!(gain = gain(), "VICTORY_DSP_CONVOLVE: mono 1024-frame convolutions are computed"));
    }
    on
}

/// What computed convolutions are scaled by: `VICTORY_DSP_GAIN`, else 2
/// (trustno1's by-ear value; the hardware's scaling is not known).
fn gain() -> f32 {
    static GAIN: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *GAIN.get_or_init(|| std::env::var("VICTORY_DSP_GAIN").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(2.0))
}

static CONVOLVED_STREAMS: AtomicU64 = AtomicU64::new(0);
static CONVOLVED_BLOCKS: AtomicU64 = AtomicU64::new(0);
static CONVOLVED_PEAK: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

const E_POINTER: HRESULT = 0x8000_4003_u32 as i32;
const E_OUTOFMEMORY: HRESULT = 0x8007_000E_u32 as i32;

const CONVOLUTION: u32 = 1;
/// Option bits that need a stereo stream, and one convolution refuses.
const STEREO_OPTIONS: u32 = 2 | 8;
const NOT_FOR_CONVOLUTION: u32 = 4;

pub struct Dsp {
    max_streams: u32,
    active: Mutex<u32>,
}

/// What activation describes.
#[repr(C, packed)]
pub struct StreamDesc {
    kind: u32,
    options: u32,
    frames: u32,
    channels: u32,
    ir_floats: u32,
    ir: *const f32,
}

/// The status block a stream's owner polls.
#[repr(C)]
pub struct Status {
    submitted: u32,
    completed: u32,
    reserved: u32,
    result: HRESULT,
}

pub struct Stream {
    dsp: *mut Dsp,
    kind: u32,
    frames: u32,
    channels: u32,
    status: Box<Status>,
    /// (diagnostic) Kind and options, commands so far, whether input with signal came.
    kind: u32,
    options: u32,
    commands: u64,
    heard: bool,
    /// (`VICTORY_DSP_CONVOLVE`) Set for a stream whose convolution is computed.
    convolver: Option<convolve::Convolver>,
}

/// One block of work.
#[repr(C)]
pub struct Command {
    gains: [f32; 2],
    input: *const f32,
    output: *mut f32,
    reserved: u64,
}

pub extern "system" fn XDspConnectWithMaximumStreamLimit(
    base: *mut c_void,
    base_length: u32,
    ir_seconds: u32,
    max_streams: u32,
    max_channels: u32,
    out: *mut *mut Dsp,
) -> HRESULT {
    if base.is_null()
        || !(0x1_0000..=0x1F40_0000).contains(&base_length)
        || ir_seconds > 0x80
        || max_streams > 0x100
        || max_channels > 2
    {
        return E_INVALIDARG;
    }
    if out.is_null() {
        return E_POINTER;
    }
    tracing::info!(
        base_length,
        ir_seconds,
        max_streams,
        max_channels,
        "audio DSP connected"
    );
    let dsp = Box::new(Dsp {
        max_streams,
        active: Mutex::new(0),
    });
    // SAFETY: checked non-null.
    unsafe { out.write(Box::into_raw(dsp)) };
    S_OK
}

pub extern "system" fn XDspDisconnect(dsp: *mut Dsp) -> HRESULT {
    if dsp.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: made by `XDspConnectWithMaximumStreamLimit`; disconnected
    // once, after its streams.
    drop(unsafe { Box::from_raw(dsp) });
    S_OK
}

fn valid(desc: &StreamDesc) -> bool {
    let (kind, options, frames, channels) = (desc.kind, desc.options, desc.frames, desc.channels);
    let shape = if kind == CONVOLUTION {
        (1..=2).contains(&channels)
            && matches!(frames, 0x200 | 0x400)
            && options & NOT_FOR_CONVOLUTION == 0
    } else {
        channels == 1 && matches!(frames, 0x200 | 0x400 | 0x800)
    };
    shape && (options & STEREO_OPTIONS == 0 || channels == 2)
}

pub extern "system" fn XDspActivate(
    dsp: *mut Dsp,
    desc: *const StreamDesc,
    status: *mut *const Status,
    stream: *mut *mut Stream,
) -> HRESULT {
    if status.is_null() || stream.is_null() {
        return E_POINTER;
    }
    if dsp.is_null() || desc.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: checked non-null; the title passes its description.
    let desc = unsafe { &*desc };
    if !valid(desc) {
        return E_INVALIDARG;
    }
    let (kind, options, frames, channels) = (desc.kind, desc.options, desc.frames, desc.channels);
    let (ir, ir_floats) = (desc.ir, desc.ir_floats);
    let convolver = if kind == CONVOLUTION && channels == 1 && frames == 0x400 && !ir.is_null() && convolve_on() {
        // SAFETY: the title's description says the impulse response holds this many floats.
        convolve::Convolver::new(unsafe { std::slice::from_raw_parts(ir, ir_floats as usize) })
    } else {
        None
    };
    if convolver.is_some() {
        CONVOLVED_STREAMS.fetch_add(1, Relaxed);
    }
    tracing::debug!(
        kind,
        options,
        frames,
        channels,
        ir_floats,
        computed = convolver.is_some(),
        "audio DSP stream activated"
    );
    {
        // SAFETY: a connection from `XDspConnectWithMaximumStreamLimit`.
        let dsp = unsafe { &*dsp };
        let mut active = dsp.active.lock().unwrap_or_else(|e| e.into_inner());
        if *active >= dsp.max_streams {
            return E_OUTOFMEMORY;
        }
        *active += 1;
    }
    let new = Box::new(Stream {
        dsp,
        kind: desc.kind,
        frames: desc.frames,
        channels: desc.channels,
        kind,
        options,
        commands: 0,
        heard: false,
        convolver,
        status: Box::new(Status {
            submitted: 0,
            completed: 0,
            reserved: 0,
            result: S_OK,
        }),
    });
    // SAFETY: checked non-null.
    unsafe {
        status.write(&*new.status);
        stream.write(Box::into_raw(new));
    }
    S_OK
}

pub extern "system" fn XDspDeactivate(stream: *mut Stream) -> HRESULT {
    if stream.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: made by `XDspActivate`; deactivated once.
    let stream = unsafe { Box::from_raw(stream) };
    // SAFETY: the connection outlives its streams.
    let dsp = unsafe { &*stream.dsp };
    *dsp.active.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
    S_OK
}

fn phone_silent() -> bool {
    static SILENT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SILENT.get_or_init(|| std::env::var("KLBF_PHONE_SILENT").is_ok_and(|v| v.trim() == "1"))
}

/// Completes the command at once: a convolution's output block is its input
/// block, any other output block is silence.
pub extern "system" fn XDspSubmitCommand(
    stream: *mut Stream,
    command: *const Command,
    ticket: *mut u32,
) -> HRESULT {
    if stream.is_null() || command.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: a stream from `XDspActivate`, the title's command.
    let (stream, command) = unsafe { (&mut *stream, &*command) };
    diagnose(stream, command);
    let samples = (stream.frames * stream.channels) as usize;
    if !command.output.is_null() && !command.input.is_null() && let Some(convolver) = stream.convolver.as_mut() {
        // SAFETY: the title's input and output buffers each hold one block of the stream.
        let (input, output) = unsafe {
            (std::slice::from_raw_parts(command.input, samples), std::slice::from_raw_parts_mut(command.output, samples))
        };
        convolver.process(input, output, command.gains[0] * gain());
        CONVOLVED_BLOCKS.fetch_add(1, Relaxed);
        let peak = output.iter().fold(0f32, |m, &x| m.max(x.abs()));
        CONVOLVED_PEAK.fetch_max(peak.to_bits(), Relaxed);
        if peak > 1e-6 {
            static SAID: std::sync::Once = std::sync::Once::new();
            SAID.call_once(|| tracing::info!(peak, gains = ?command.gains, "VICTORY_DSP_CONVOLVE: first convolved block with signal"));
        }
    } else if dry() && !command.output.is_null() && !command.input.is_null() {
        pass_dry(stream, command);
    } else if !command.output.is_null() {
        let samples = (stream.frames * stream.channels) as usize;
        let pass = stream.kind == CONVOLUTION && !command.input.is_null() && !phone_silent();
        // SAFETY: the title's input and output buffers each hold one block of
        // the stream (same frames and channels); `copy` allows them to overlap.
        unsafe {
            if pass {
                std::ptr::copy(command.input, command.output, samples);
            } else {
                std::ptr::write_bytes(command.output, 0, samples);
            }
        }
    }
    let status = &mut *stream.status;
    let next = status.submitted.wrapping_add(1);
    // SAFETY: the status block is read by title code on other threads.
    unsafe {
        std::ptr::write_volatile(&mut status.submitted, next);
        std::ptr::write_volatile(&mut status.result, S_OK);
        std::ptr::write_volatile(&mut status.completed, next);
    }
    if !ticket.is_null() {
        // SAFETY: optional out parameter.
        unsafe { ticket.write(next) };
    }
    S_OK
}

/// `VICTORY_DSP_DRY=1` (off by default): a block's output is its input
/// times the command's gains (unity when both gains are 0 or not finite)
/// instead of silence: the source without the device-speaker colour.
fn dry() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::env::var("VICTORY_DSP_DRY").is_ok_and(|v| v == "1");
        if on {
            tracing::info!("VICTORY_DSP_DRY: audio DSP blocks pass their input through (times the command's gains)");
        }
        on
    })
}

static DRY_BLOCKS: AtomicU64 = AtomicU64::new(0);
/// Peaks since the last 5 s line, as f32 bits (non-negative floats order as their bits).
static DRY_PEAK_IN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static DRY_PEAK_OUT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static DRY_KINDS: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());
static DRY_FIRST_SIGNAL: std::sync::Once = std::sync::Once::new();

/// One block, output = input times the gain of its channel (interleaved).
fn pass_dry(stream: &Stream, command: &Command) {
    let channels = stream.channels.max(1) as usize;
    let samples = stream.frames as usize * channels;
    let given = command.gains;
    let gains = if given.iter().all(|g| g.is_finite()) && given.iter().any(|&g| g != 0.0) {
        if channels == 1 { [given[0], given[0]] } else { given }
    } else {
        [1.0, 1.0]
    };
    {
        let mut kinds = DRY_KINDS.lock().unwrap_or_else(|e| e.into_inner());
        if !kinds.contains(&(stream.kind, stream.options)) && kinds.len() < 64 {
            kinds.push((stream.kind, stream.options));
            tracing::info!(
                kind = stream.kind,
                options = stream.options,
                frames = stream.frames,
                channels = stream.channels,
                gains = ?given,
                used = ?gains,
                "VICTORY_DSP_DRY: first block of this stream kind/options passed through"
            );
        }
    }
    let (mut peak_in, mut peak_out) = (0f32, 0f32);
    for i in 0..samples {
        // SAFETY: input and output each hold one block of the stream; each
        // sample is read before it is written, so an in-place block is fine.
        unsafe {
            let x = command.input.add(i).read_unaligned();
            let y = x * gains[i % channels.min(2)];
            command.output.add(i).write_unaligned(y);
            peak_in = peak_in.max(x.abs());
            peak_out = peak_out.max(y.abs());
        }
    }
    DRY_BLOCKS.fetch_add(1, Relaxed);
    DRY_PEAK_IN.fetch_max(peak_in.to_bits(), Relaxed);
    DRY_PEAK_OUT.fetch_max(peak_out.to_bits(), Relaxed);
    if peak_in > 1e-6 {
        DRY_FIRST_SIGNAL.call_once(|| {
            tracing::info!(
                kind = stream.kind,
                options = stream.options,
                peak_in,
                peak_out,
                gains = ?given,
                "VICTORY_DSP_DRY: first block with signal: input and output"
            )
        });
    }
}

static COMMANDS: AtomicU64 = AtomicU64::new(0);
static STREAMS_HEARD: AtomicU64 = AtomicU64::new(0);

/// (log only, debug) Per stream: the first command whose input is not
/// silent; every 5 s: commands and streams that had signal.
fn diagnose(stream: &mut Stream, command: &Command) {
    stream.commands += 1;
    COMMANDS.fetch_add(1, Relaxed);
    if !stream.heard && !command.input.is_null() && tracing::enabled!(tracing::Level::DEBUG) {
        let samples = (stream.frames * stream.channels) as usize;
        // SAFETY: the title's input buffer holds one block of the stream.
        let input = unsafe { std::slice::from_raw_parts(command.input, samples) };
        let peak = input.iter().fold(0f32, |m, &x| m.max(x.abs()));
        if peak > 1e-6 {
            stream.heard = true;
            STREAMS_HEARD.fetch_add(1, Relaxed);
            tracing::debug!(
                kind = stream.kind,
                options = stream.options,
                frames = stream.frames,
                channels = stream.channels,
                gains = ?command.gains,
                peak,
                commands = stream.commands,
                "audio DSP: first non-silent input of a stream (its output is zeros)"
            );
        }
    }
    static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let now = std::time::Instant::now();
    if last.is_none_or(|t| now.duration_since(t).as_secs() >= 5) {
        if last.is_some() {
            // SAFETY: the connection outlives its streams.
            let active = *unsafe { &*stream.dsp }.active.lock().unwrap_or_else(|e| e.into_inner());
            tracing::debug!(
                active,
                commands = COMMANDS.load(Relaxed),
                streams_with_signal = STREAMS_HEARD.load(Relaxed),
                "audio DSP: 5 s counters (totals)"
            );
            if CONVOLVED_STREAMS.load(Relaxed) != 0 {
                tracing::info!(
                    active,
                    streams = CONVOLVED_STREAMS.load(Relaxed),
                    blocks = CONVOLVED_BLOCKS.load(Relaxed),
                    peak_out = f32::from_bits(CONVOLVED_PEAK.swap(0, Relaxed)),
                    "VICTORY_DSP_CONVOLVE: 5 s counters (streams, blocks total; peak since the last line)"
                );
            }
            if dry() {
                tracing::info!(
                    active,
                    blocks = DRY_BLOCKS.load(Relaxed),
                    streams_with_signal = STREAMS_HEARD.load(Relaxed),
                    peak_in = f32::from_bits(DRY_PEAK_IN.swap(0, Relaxed)),
                    peak_out = f32::from_bits(DRY_PEAK_OUT.swap(0, Relaxed)),
                    "VICTORY_DSP_DRY: 5 s counters (blocks total; peaks since the last line)"
                );
            }
        }
        *last = Some(now);
    }
}

pub(crate) fn module() -> HleModule {
    HleModule {
        name: "dsp",
        dlls: &["chad.dll"],
        exports: vec![
            hle_export!(XDspConnectWithMaximumStreamLimit),
            hle_export!(XDspDisconnect),
            hle_export!(XDspActivate),
            hle_export!(XDspDeactivate),
            hle_export!(XDspSubmitCommand),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convolution_stream_as_the_title_drives_it() {
        let mut base = vec![0u8; 0x10_0000];
        let mut dsp = std::ptr::null_mut();
        assert_eq!(
            XDspConnectWithMaximumStreamLimit(
                base.as_mut_ptr().cast(),
                0x10_0000,
                24,
                0x80,
                1,
                &mut dsp
            ),
            S_OK
        );
        let ir = vec![0f32; 0x800];
        // Stereo convolution with option 2, as the title asks for.
        let desc = StreamDesc {
            kind: CONVOLUTION,
            options: 2,
            frames: 0x400,
            channels: 2,
            ir_floats: 0x800,
            ir: ir.as_ptr(),
        };
        let (mut status, mut stream) = (std::ptr::null(), std::ptr::null_mut());
        assert_eq!(XDspActivate(dsp, &desc, &mut status, &mut stream), S_OK);
        let mut output = vec![1f32; 0x800];
        let input = vec![0.5f32; 0x800];
        let command = Command {
            gains: [1.0, 1.0],
            input: input.as_ptr(),
            output: output.as_mut_ptr(),
            reserved: 0,
        };
        let mut ticket = 0;
        assert_eq!(XDspSubmitCommand(stream, &command, &mut ticket), S_OK);
        // SAFETY: the stream's status block.
        let (completed, result) = unsafe { ((*status).completed, (*status).result) };
        assert_eq!((completed, result), (ticket, S_OK));
        // A convolution passes its input through.
        assert!(output.iter().all(|&s| s == 0.5));
        // No input: silence.
        let command = Command {
            input: std::ptr::null(),
            ..command
        };
        assert_eq!(XDspSubmitCommand(stream, &command, &mut ticket), S_OK);
        assert!(output.iter().all(|&s| s == 0.0));
        assert_eq!(XDspDeactivate(stream), S_OK);

        // Mono streams refuse stereo options; transforms are mono only.
        let bad = StreamDesc {
            channels: 1,
            ..desc
        };
        assert_eq!(
            XDspActivate(dsp, &bad, &mut status, &mut stream),
            E_INVALIDARG
        );
        let transform = StreamDesc {
            kind: 2,
            options: 0,
            channels: 1,
            ir_floats: 0,
            ir: std::ptr::null(),
            ..desc
        };
        assert_eq!(
            XDspActivate(dsp, &transform, &mut status, &mut stream),
            S_OK
        );
        // Other transforms still output silence.
        let mut output = vec![1f32; 0x400];
        let command = Command {
            gains: [1.0, 1.0],
            input: input.as_ptr(),
            output: output.as_mut_ptr(),
            reserved: 0,
        };
        assert_eq!(XDspSubmitCommand(stream, &command, &mut ticket), S_OK);
        assert!(output.iter().all(|&s| s == 0.0));
        assert_eq!(XDspDeactivate(stream), S_OK);
        assert_eq!(XDspDisconnect(dsp), S_OK);
    }

    /// (ported from trustno1 eca45ac)
    #[test]
    fn a_mono_convolution_is_computed() {
        // SAFETY: tests of this module read this variable only at activation.
        unsafe { std::env::set_var("VICTORY_DSP_CONVOLVE", "1") };
        let mut base = vec![0u8; 0x10_0000];
        let mut dsp = std::ptr::null_mut();
        assert_eq!(XDspConnectWithMaximumStreamLimit(base.as_mut_ptr().cast(), 0x10_0000, 24, 0x80, 1, &mut dsp), S_OK);
        // a response of one sample, 0.5, two samples late
        let ir = convolve::layout(&[0.0, 0.0, 0.5]);
        let desc = StreamDesc { kind: CONVOLUTION, options: 0, frames: 0x400, channels: 1, ir_floats: ir.len() as u32, ir: ir.as_ptr() };
        let (mut status, mut stream) = (std::ptr::null(), std::ptr::null_mut());
        assert_eq!(XDspActivate(dsp, &desc, &mut status, &mut stream), S_OK);
        let input: Vec<f32> = (0..0x400).map(|i| (i % 7) as f32 / 7.0).collect();
        let mut output = vec![0f32; 0x400];
        let command = Command { gains: [1.0, 1.0], input: input.as_ptr(), output: output.as_mut_ptr(), reserved: 0 };
        let mut ticket = 0;
        assert_eq!(XDspSubmitCommand(stream, &command, &mut ticket), S_OK);
        for n in 2..0x400 {
            let want = input[n - 2] * 0.5 * gain();
            assert!((output[n] - want).abs() < 1e-4, "sample {n}: {} vs {want}", output[n]);
        }
        assert_eq!(XDspDeactivate(stream), S_OK);
        assert_eq!(XDspDisconnect(dsp), S_OK);
    }
}
