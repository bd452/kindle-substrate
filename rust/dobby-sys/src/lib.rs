//! Minimal FFI surface for the vendored Dobby engine (see `include/dobby.h`).
//!
//! The engine is C++ but exposes a tiny C ABI, so we hand-declare the functions
//! we use rather than run bindgen. `build.rs` static-links `libdobby.a` for the
//! on-device ARM build; on host this crate is not a dependency.
//!
//! Note: `DobbyImportTableReplace` is intentionally *not* bound. That plugin is
//! Darwin/Mach-O only; Kindle Linux PLT/GOT hooking lives in `ksubstrate::plt`.

use std::os::raw::{c_char, c_void};

extern "C" {
    /// Install an inline hook. Returns 0 on success. `out_origin_func` receives a
    /// callable pointer to the relocated original (may be null to ignore).
    pub fn DobbyHook(
        address: *mut c_void,
        fake_func: *mut c_void,
        out_origin_func: *mut *mut c_void,
    ) -> i32;

    /// Remove a previously installed hook. Returns 0 on success.
    pub fn DobbyDestroy(address: *mut c_void) -> i32;

    /// Resolve a symbol (including many non-exported ones) within a loaded image.
    pub fn DobbySymbolResolver(image_name: *const c_char, symbol_name: *const c_char) -> *mut c_void;
}
