#[cfg(ksubstrate_dynamic)]
use std::os::raw::{c_char, c_int, c_void};
#[cfg(ksubstrate_dynamic)]
use std::sync::atomic::{AtomicPtr, Ordering};

#[cfg_attr(target_os = "linux", link_section = ".init_array")]
#[used]
static KSUBSTRATE_SAMPLE_TWEAK_INIT: extern "C" fn() = init;

extern "C" fn init() {
    kindle_compat::ensure_linked();
    install_inline_hook();
    install_got_hook();
}

extern "C" fn replacement_compute() -> i32 {
    42
}

/// Resolve the target's exported `compute` symbol and install a real inline hook
/// through the runtime ABI so its callers observe the replacement value (R1).
#[cfg(ksubstrate_dynamic)]
fn install_inline_hook() {
    unsafe {
        let symbol = b"compute\0";
        let target = kh_find_symbol(std::ptr::null(), symbol.as_ptr().cast());
        if target.is_null() {
            log("sample tweak: could not resolve compute symbol");
            return;
        }
        let mut original: *mut c_void = std::ptr::null_mut();
        let status = kh_hook_function(target, replacement_compute as *mut c_void, &mut original);
        if status == 0 {
            log("sample tweak: inline hooked compute -> 42");
        } else {
            log(&format!("sample tweak: kh_hook_function failed with status {status}"));
        }
    }
}

#[cfg(not(ksubstrate_dynamic))]
fn install_inline_hook() {
    let _ = replacement_compute as extern "C" fn() -> i32;
    log("sample tweak: inert host build (inline)");
}

/// Preferred stable path (R2 / A§4.2): rewrite the GOT jump-slot for `write` so
/// the demo target's result-file write goes through our wrapper. No inline
/// prologue patch is involved.
#[cfg(ksubstrate_dynamic)]
static ORIG_WRITE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

#[cfg(ksubstrate_dynamic)]
extern "C" fn replacement_write(fd: c_int, buf: *const c_void, count: usize) -> isize {
    log("sample tweak: GOT-hooked write() fired");
    let orig = ORIG_WRITE.load(Ordering::SeqCst);
    if orig.is_null() {
        return -1;
    }
    let orig_fn: extern "C" fn(c_int, *const c_void, usize) -> isize =
        unsafe { std::mem::transmute(orig) };
    orig_fn(fd, buf, count)
}

#[cfg(ksubstrate_dynamic)]
fn install_got_hook() {
    unsafe {
        let mut original: *mut c_void = std::ptr::null_mut();
        // Import hooks are deliberately scoped to one loaded image.  The demo
        // process is this executable; image_identity canonicalizes this alias
        // before matching it against /proc/self/maps.
        let status = kh_hook_import(
            b"/proc/self/exe\0".as_ptr().cast(),
            b"write\0".as_ptr().cast(),
            replacement_write as *mut c_void,
            &mut original,
        );
        if status == 0 {
            ORIG_WRITE.store(original, Ordering::SeqCst);
            log("sample tweak: GOT hooked write()");
        } else {
            log(&format!("sample tweak: kh_hook_import(write) failed with status {status}"));
        }
    }
}

#[cfg(not(ksubstrate_dynamic))]
fn install_got_hook() {
    log("sample tweak: inert host build (got)");
}

fn log(message: &str) {
    #[cfg(ksubstrate_dynamic)]
    unsafe {
        let mut bytes = Vec::with_capacity(message.len() + 1);
        bytes.extend_from_slice(message.as_bytes());
        bytes.push(0);
        kh_log(bytes.as_ptr().cast());
    }

    #[cfg(not(ksubstrate_dynamic))]
    {
        let _ = message;
    }
}

#[cfg(ksubstrate_dynamic)]
extern "C" {
    fn kh_find_symbol(image: *const c_char, name: *const c_char) -> *mut c_void;
    fn kh_hook_function(
        target: *mut c_void,
        replacement: *mut c_void,
        original: *mut *mut c_void,
    ) -> i32;
    fn kh_hook_import(
        image: *const c_char,
        symbol: *const c_char,
        replacement: *mut c_void,
        original: *mut *mut c_void,
    ) -> i32;
    fn kh_log(message: *const c_char);
}
