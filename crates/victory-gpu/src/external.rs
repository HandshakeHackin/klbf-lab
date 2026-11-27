//! Host objects for PC libraries the title's calls are redirected to (the
//! FidelityFX upscaler of the PC, `VICTORY_FSR_DLL`). The title holds our
//! console objects; a PC library needs the host device, the host command
//! list the title's list records onto, and the host textures behind the
//! title's. (KLBF Lab: ported from our Neptune base to Oplite's.)
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::ffi::c_void;

use crate::object::This;

/// The host `ID3D12Device` (no reference taken), if there is a host GPU.
pub fn host_device() -> Option<*mut c_void> {
    crate::host::host().map(|h| windows::core::Interface::as_raw(&h.device))
}

/// (KLBF Lab, 27/11) `f` given a throwaway D3D12 device on the card the host will use (the
/// high-performance one, else Windows' default), before the host is made: to try a PC library first.
/// `None` without a D3D12 card.
pub fn with_probe_device<R>(f: impl FnOnce(*mut c_void) -> R) -> Option<R> {
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_12_0;
    use windows::Win32::Graphics::Direct3D12::{D3D12CreateDevice, ID3D12Device};
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: plain object creation.
    unsafe {
        let made = match crate::host::high_performance_adapter() {
            Some(a) => D3D12CreateDevice(&a, D3D_FEATURE_LEVEL_12_0, &mut device),
            None => D3D12CreateDevice(None, D3D_FEATURE_LEVEL_12_0, &mut device),
        };
        if made.is_err() {
            let _ = D3D12CreateDevice(None, D3D_FEATURE_LEVEL_12_0, &mut device);
        }
    }
    let device = device?;
    Some(f(windows::core::Interface::as_raw(&device)))
}

/// Whether `object` is the device the title created.
pub fn is_title_device(object: *const c_void) -> bool {
    std::ptr::eq(object, (&raw const crate::device::DEVICE).cast())
}

/// Whether `object` is a command list of ours.
pub fn is_title_list(object: *const c_void) -> bool {
    crate::list::is_list(object.cast_mut().cast())
}

/// Whether `object` is a live title texture.
pub fn is_title_texture(object: *const c_void) -> bool {
    crate::resource::is_texture(object.cast_mut().cast())
}

/// One title list's host list, open for a PC library to record onto.
pub struct Pass<'a> {
    list: &'a crate::host::HostList,
}

impl Pass<'_> {
    /// The host `ID3D12GraphicsCommandList` (no reference taken).
    pub fn command_list(&self) -> *mut c_void {
        windows::core::Interface::as_raw(&self.list.list)
    }

    /// The host texture behind title texture `texture`, moved to the host
    /// resource state `state` (a D3D12 state, as the library will see it)
    /// on this list; `write` if the library writes it. `None` if the title
    /// has no texture there or the host refuses it.
    pub fn texture(&mut self, texture: *const c_void, state: u32, write: bool) -> Option<*mut c_void> {
        let title: This = texture.cast_mut().cast();
        if !crate::resource::is_texture(title) {
            return None;
        }
        let host = crate::resource::host_resource(title)?;
        // What the title fills itself is uploaded first, as for a copy.
        if !write && crate::texture::title_written(host) {
            crate::texture::upload(self.list, title, host);
        }
        // Depth textures shaders also write: what they wrote, first.
        self.list.twin_use(title, false, write);
        crate::list::ensure_state(self.list, title, state);
        if write && crate::texture::title_written(host) {
            crate::texture::host_written(title);
        }
        Some(windows::core::Interface::as_raw(host))
    }
}

/// Runs `f` with title list `list`'s host list; afterwards the title's
/// bindings are set again on it (the library set its own). `false` if
/// `list` is not a list of ours with a host list (a thin list, `VICTORY_THIN=1`,
/// has none: the upscale is then skipped, said once).
pub fn record(list: *const c_void, f: impl FnOnce(&mut Pass)) -> bool {
    crate::list::with_host(list.cast_mut().cast(), |host, state| {
        f(&mut Pass { list: host });
        state.replay(host);
    })
}
