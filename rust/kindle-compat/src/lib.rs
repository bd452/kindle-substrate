pub fn ensure_linked() {}

/// Kindle firmware ships an old glibc whose `getauxval` is missing or unreliable
/// for the newer libc the cross-toolchain links against. Overriding it to return
/// 0 (as if the requested auxv entry is absent) keeps libc feature probes on the
/// safe/no-op path rather than dereferencing an unavailable auxiliary vector.
/// `ensure_linked()` exists so binaries can force this object to be linked in.
#[cfg(all(target_os = "linux", target_arch = "arm"))]
#[no_mangle]
pub extern "C" fn getauxval(_kind: libc::c_ulong) -> libc::c_ulong {
    0
}
