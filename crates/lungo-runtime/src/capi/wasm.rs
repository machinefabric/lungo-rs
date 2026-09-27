//! The WebAssembly host interface: how the TypeScript support library (`lungo-ts`) exchanges
//! bytes with a program's module and serves as its host.
//!
//! JavaScript cannot give the runtime C function pointers, so the host's entry points are
//! imports of the module (`lungo.dispatch`, `lungo.retain`, `lungo.release`), installed with
//! `lungo_wasm_use_host`. Bytes cross through memory the host allocates with `lungo_wasm_alloc`
//! and buffers it creates with `lungo_wasm_buffer_new`.

use crate::object::lean_internal_panic;
use crate::wire::{self, Buffer, Host};
use std::alloc::Layout;

#[link(wasm_import_module = "lungo")]
unsafe extern "C" {
    #[link_name = "dispatch"]
    fn import_dispatch(callback: u64, input: *const u8, len: usize, out: *mut Buffer) -> i32;
    #[link_name = "retain"]
    fn import_retain(callback: u64);
    #[link_name = "release"]
    fn import_release(callback: u64);
}

unsafe extern "C" fn dispatch(callback: u64, input: *const u8, len: usize, out: *mut Buffer) -> i32 {
    unsafe { import_dispatch(callback, input, len, out) }
}

unsafe extern "C" fn retain(callback: u64) {
    unsafe { import_retain(callback) }
}

unsafe extern "C" fn release(callback: u64) {
    unsafe { import_release(callback) }
}

/// Installs the module's imports as the host, once.
#[unsafe(no_mangle)]
pub extern "C" fn lungo_wasm_use_host() {
    wire::set_host(Host { dispatch, retain, release });
}

fn layout(len: usize) -> Layout {
    Layout::array::<u8>(len.max(1)).unwrap_or_else(|_| lean_internal_panic("a host allocation is too large"))
}

/// `len` bytes of memory for the host to write, freed with `lungo_wasm_free`.
#[unsafe(no_mangle)]
pub extern "C" fn lungo_wasm_alloc(len: usize) -> *mut u8 {
    let p = unsafe { std::alloc::alloc(layout(len)) };
    if p.is_null() {
        crate::object::lean_internal_panic_out_of_memory();
    }
    p
}

/// Frees `len` bytes allocated with `lungo_wasm_alloc`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_wasm_free(p: *mut u8, len: usize) {
    if p.is_null() {
        lean_internal_panic("lungo_wasm_free: a null pointer");
    }
    unsafe { std::alloc::dealloc(p, layout(len)) }
}

/// An empty `lungo_buffer` for the host to pass to entry points; its layout on `wasm32` is three
/// little-endian `u32`s: data, length, capacity.
#[unsafe(no_mangle)]
pub extern "C" fn lungo_wasm_buffer_new() -> *mut Buffer {
    Box::into_raw(Box::new(Buffer::empty()))
}

/// Frees a buffer created with `lungo_wasm_buffer_new`, which must be empty (its bytes freed
/// with `lungo_buffer_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_wasm_buffer_delete(b: *mut Buffer) {
    if b.is_null() {
        lean_internal_panic("lungo_wasm_buffer_delete: a null pointer");
    }
    let b = unsafe { Box::from_raw(b) };
    if !b.data.is_null() {
        lean_internal_panic("lungo_wasm_buffer_delete: the buffer still holds bytes");
    }
}
