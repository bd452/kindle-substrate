mod backend;
mod chain;
mod plt;
mod thunk;

use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::raw::{c_char, c_int, c_void};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::sync::{Mutex, OnceLock};

pub type I32Hook = extern "C" fn() -> i32;

static NAMED_I32_HOOKS: OnceLock<Mutex<BTreeMap<String, I32Hook>>> = OnceLock::new();

fn named_hooks() -> &'static Mutex<BTreeMap<String, I32Hook>> {
    NAMED_I32_HOOKS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub fn register_named_i32_hook(name: &str, replacement: I32Hook) -> Result<(), HookError> {
    kindle_compat::ensure_linked();
    if name.trim().is_empty() {
        return Err(HookError::InvalidArgument);
    }
    let mut hooks = named_hooks().lock().map_err(|_| HookError::Poisoned)?;
    hooks.insert(name.to_owned(), replacement);
    Ok(())
}

pub fn clear_named_hook(name: &str) -> Result<(), HookError> {
    let mut hooks = named_hooks().lock().map_err(|_| HookError::Poisoned)?;
    hooks.remove(name);
    Ok(())
}

pub fn call_named_i32(name: &str, original: I32Hook) -> i32 {
    let replacement = named_hooks()
        .lock()
        .ok()
        .and_then(|hooks| hooks.get(name).copied());
    replacement.unwrap_or(original)()
}

pub fn find_symbol(image: Option<&str>, name: &str) -> *mut c_void {
    kindle_compat::ensure_linked();
    if name.is_empty() {
        return ptr::null_mut();
    }

    let cname = match CString::new(name) {
        Ok(value) => value,
        Err(_) => return ptr::null_mut(),
    };

    let via_dlsym = unsafe {
        let handle = match image {
            Some(path) if !path.is_empty() => {
                let cpath = match CString::new(path) {
                    Ok(value) => value,
                    Err(_) => return ptr::null_mut(),
                };
                libc::dlopen(cpath.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL)
            }
            _ => libc::RTLD_DEFAULT,
        };

        if handle.is_null() {
            ptr::null_mut()
        } else {
            libc::dlsym(handle, cname.as_ptr()).cast::<c_void>()
        }
    };
    if !via_dlsym.is_null() {
        return via_dlsym;
    }

    // On-device, fall back to Dobby's resolver, which finds many symbols the
    // dynamic linker won't (non-exported firmware internals).
    #[cfg(all(target_os = "linux", target_arch = "arm"))]
    {
        return dobby_symbol(image, name);
    }

    #[allow(unreachable_code)]
    ptr::null_mut()
}

#[cfg(all(target_os = "linux", target_arch = "arm"))]
fn dobby_symbol(image: Option<&str>, name: &str) -> *mut c_void {
    let Ok(cname) = CString::new(name) else {
        return ptr::null_mut();
    };
    let cimage = image.and_then(|value| CString::new(value).ok());
    let image_ptr = cimage.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null());
    unsafe { dobby_sys::DobbySymbolResolver(image_ptr, cname.as_ptr()) }
}

/// Resolve a runtime code address for a firmware-private function that is not an
/// exported symbol: find the module's load base in `/proc/self/maps` and add the
/// RVA recorded in the symbol DB. Callers should follow up with the checked hook
/// so a wrong base/RVA (e.g. after a firmware update) is refused by the prologue
/// signature rather than silently patched.
pub fn resolve_rva(image: Option<&str>, rva: usize) -> *mut c_void {
    let Some(image) = image.filter(|value| !value.is_empty()) else {
        return ptr::null_mut();
    };
    match module_base(image) {
        Some(base) => (base + rva) as *mut c_void,
        None => ptr::null_mut(),
    }
}

fn module_base(image: &str) -> Option<usize> {
    let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
    module_base_from_maps(&maps, image)
}

/// Lowest mapped address of the object whose path is (or ends in) `image`.
fn module_base_from_maps(maps: &str, image: &str) -> Option<usize> {
    let mut lowest: Option<usize> = None;
    for line in maps.lines() {
        let range = match line.split_whitespace().next() {
            Some(range) => range,
            None => continue,
        };
        let Some(path) = line.split_whitespace().nth(5) else {
            continue;
        };
        let matches = path == image
            || std::path::Path::new(path)
                .file_name()
                .map(|name| name == image)
                .unwrap_or(false);
        if !matches {
            continue;
        }
        if let Some(start) = range
            .split('-')
            .next()
            .and_then(|value| usize::from_str_radix(value, 16).ok())
        {
            lowest = Some(lowest.map_or(start, |current| current.min(start)));
        }
    }
    lowest
}

pub fn record_raw_hook(
    target: *mut c_void,
    replacement: *mut c_void,
    original: *mut *mut c_void,
) -> Result<(), HookError> {
    unsafe { chain::hook_inline(target, replacement, original, None) }
}

pub unsafe fn record_raw_hook_checked(
    target: *mut c_void,
    replacement: *mut c_void,
    original: *mut *mut c_void,
    expected_prologue: *const c_void,
    expected_len: usize,
) -> Result<(), HookError> {
    if expected_prologue.is_null() || expected_len == 0 {
        return Err(HookError::InvalidArgument);
    }
    // The verification must cover at least the bytes we are about to overwrite,
    // otherwise a caller could describe fewer bytes than the patch window and we
    // would clobber unverified prologue.
    if expected_len < backend::PATCH_LEN {
        return Err(HookError::InvalidArgument);
    }

    let expected =
        unsafe { std::slice::from_raw_parts(expected_prologue.cast::<u8>(), expected_len) };
    unsafe { chain::hook_inline(target, replacement, original, Some(expected)) }
}

pub fn unhook_raw(target: *mut c_void) -> Result<(), HookError> {
    let _ = target;
    Err(HookError::Unsupported)
}

/// PLT/GOT hook: replace an imported symbol's jump-slot GOT entry in `image`
/// (or all loaded images when `image` is None). This is the preferred,
/// update-stable mechanism (A§4.2) for intercepting calls a target makes into
/// libc/liblipc/etc. Implemented as a native ELF rewriter — Dobby's
/// ImportTableReplace plugin is Darwin/Mach-O only and is not used here.
pub fn hook_import(
    image: Option<&str>,
    symbol: &str,
    replacement: *mut c_void,
    original: *mut *mut c_void,
) -> Result<(), HookError> {
    chain::hook_import(image, symbol, replacement, original)
}

pub fn log(message: &str) {
    // Runtime calls receive a state-tmpfs destination.  Do not create a
    // fallback file when launched outside a session.
    if let Ok(path) = std::env::var("KSUBSTRATE_LOG") {
        let _ = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| writeln!(file, "{message}"));
    } else {
        eprintln!("ksubstrate: {message}");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum HookError {
    Unsupported = -1,
    InvalidArgument = -2,
    Poisoned = -3,
    Panic = -4,
    System = -5,
    AlreadyHooked = -6,
    NotHooked = -7,
    PrologueMismatch = -8,
    InvalidTargetMode = -9,
    ThunkAllocationFailed = -10,
    IncompatibleImportTargets = -11,
    SignatureTooLong = -12,
    InstallationConflict = -13,
}

fn c_string<'a>(value: *const c_char) -> Result<&'a str, HookError> {
    if value.is_null() {
        return Err(HookError::InvalidArgument);
    }
    unsafe { CStr::from_ptr(value) }
        .to_str()
        .map_err(|_| HookError::InvalidArgument)
}

fn ffi_status(result: Result<(), HookError>) -> c_int {
    match result {
        Ok(()) => 0,
        Err(error) => error as c_int,
    }
}

#[no_mangle]
pub extern "C" fn kh_hook_function(
    target: *mut c_void,
    replacement: *mut c_void,
    original: *mut *mut c_void,
) -> c_int {
    match catch_unwind(AssertUnwindSafe(|| {
        record_raw_hook(target, replacement, original)
    })) {
        Ok(result) => ffi_status(result),
        Err(_) => HookError::Panic as c_int,
    }
}

#[no_mangle]
pub extern "C" fn kh_hook_function_checked(
    target: *mut c_void,
    replacement: *mut c_void,
    original: *mut *mut c_void,
    expected_prologue: *const c_void,
    expected_len: usize,
) -> c_int {
    match catch_unwind(AssertUnwindSafe(|| unsafe {
        record_raw_hook_checked(
            target,
            replacement,
            original,
            expected_prologue,
            expected_len,
        )
    })) {
        Ok(result) => ffi_status(result),
        Err(_) => HookError::Panic as c_int,
    }
}

#[no_mangle]
pub extern "C" fn kh_hook_import(
    image: *const c_char,
    symbol: *const c_char,
    replacement: *mut c_void,
    original: *mut *mut c_void,
) -> c_int {
    match catch_unwind(AssertUnwindSafe(|| {
        let image = if image.is_null() {
            None
        } else {
            c_string(image).ok()
        };
        let symbol = c_string(symbol)?;
        hook_import(image, symbol, replacement, original)
    })) {
        Ok(result) => ffi_status(result),
        Err(_) => HookError::Panic as c_int,
    }
}

#[no_mangle]
pub extern "C" fn kh_unhook_function(target: *mut c_void) -> c_int {
    match catch_unwind(AssertUnwindSafe(|| unhook_raw(target))) {
        Ok(result) => ffi_status(result),
        Err(_) => HookError::Panic as c_int,
    }
}

#[no_mangle]
pub extern "C" fn kh_resolve_rva(image: *const c_char, rva: usize) -> *mut c_void {
    catch_unwind(AssertUnwindSafe(|| {
        let image = if image.is_null() {
            None
        } else {
            c_string(image).ok()
        };
        resolve_rva(image, rva)
    }))
    .unwrap_or(ptr::null_mut())
}

#[no_mangle]
pub extern "C" fn kh_find_symbol(image: *const c_char, name: *const c_char) -> *mut c_void {
    catch_unwind(AssertUnwindSafe(|| {
        let image = if image.is_null() {
            None
        } else {
            c_string(image).ok()
        };
        let Ok(name) = c_string(name) else {
            return ptr::null_mut();
        };
        find_symbol(image, name)
    }))
    .unwrap_or(ptr::null_mut())
}

#[no_mangle]
pub extern "C" fn kh_register_named_i32_hook(
    name: *const c_char,
    replacement: Option<I32Hook>,
) -> c_int {
    match catch_unwind(AssertUnwindSafe(|| {
        let name = c_string(name)?;
        let replacement = replacement.ok_or(HookError::InvalidArgument)?;
        register_named_i32_hook(name, replacement)
    })) {
        Ok(result) => ffi_status(result),
        Err(_) => HookError::Panic as c_int,
    }
}

#[no_mangle]
pub extern "C" fn kh_clear_named_hook(name: *const c_char) -> c_int {
    match catch_unwind(AssertUnwindSafe(|| {
        let name = c_string(name)?;
        clear_named_hook(name)
    })) {
        Ok(result) => ffi_status(result),
        Err(_) => HookError::Panic as c_int,
    }
}

#[no_mangle]
pub extern "C" fn kh_call_named_i32(name: *const c_char, original: Option<I32Hook>) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        let Ok(name) = c_string(name) else {
            return 0;
        };
        let Some(original) = original else {
            return 0;
        };
        call_named_i32(name, original)
    }))
    .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn kh_log(message: *const c_char) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Ok(message) = c_string(message) {
            log(message);
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn original() -> i32 {
        41
    }

    extern "C" fn replacement() -> i32 {
        42
    }

    #[test]
    fn named_hooks_override_originals() {
        clear_named_hook("test.compute").unwrap();
        assert_eq!(call_named_i32("test.compute", original), 41);
        register_named_i32_hook("test.compute", replacement).unwrap();
        assert_eq!(call_named_i32("test.compute", original), 42);
        clear_named_hook("test.compute").unwrap();
        assert_eq!(call_named_i32("test.compute", original), 41);
    }

    #[test]
    fn module_base_picks_lowest_matching_mapping() {
        let maps = "\
00010000-00020000 r-xp 00000000 fe:01 100  /usr/bin/reader\n\
00020000-00030000 r--p 00010000 fe:01 100  /usr/bin/reader\n\
b6f00000-b6f10000 r-xp 00000000 fe:01 200  /lib/libc.so.6\n";
        assert_eq!(module_base_from_maps(maps, "reader"), Some(0x0001_0000));
        assert_eq!(
            module_base_from_maps(maps, "/usr/bin/reader"),
            Some(0x0001_0000)
        );
        assert_eq!(module_base_from_maps(maps, "libc.so.6"), Some(0xb6f0_0000));
        assert_eq!(module_base_from_maps(maps, "nonexistent"), None);
    }

    #[test]
    fn resolve_rva_requires_image() {
        assert!(resolve_rva(None, 0x10).is_null());
        assert!(resolve_rva(Some(""), 0x10).is_null());
    }

    #[test]
    fn checked_hook_rejects_short_expected_len() {
        let mut trampoline = ptr::null_mut();
        let expected = [0xffu8, 0xee, 0xdd, 0xcc];
        let result = unsafe {
            record_raw_hook_checked(
                original as *mut c_void,
                replacement as *mut c_void,
                &mut trampoline,
                expected.as_ptr().cast(),
                expected.len(),
            )
        };
        assert_eq!(result, Err(HookError::InvalidArgument));
    }

    #[test]
    fn checked_hook_refuses_mismatched_prologue() {
        let mut trampoline = ptr::null_mut();
        let expected = [0xffu8, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88];
        let result = unsafe {
            record_raw_hook_checked(
                original as *mut c_void,
                replacement as *mut c_void,
                &mut trampoline,
                expected.as_ptr().cast(),
                expected.len(),
            )
        };
        assert_eq!(result, Err(HookError::PrologueMismatch));
    }

    #[test]
    fn checked_hook_accepts_matching_prologue() {
        let target = original as *mut c_void;
        let code_addr = backend::code_address(target);
        let expected =
            unsafe { std::slice::from_raw_parts(code_addr as *const u8, backend::PATCH_LEN) };
        let mut trampoline = ptr::null_mut();
        let result = unsafe {
            record_raw_hook_checked(
                target,
                replacement as *mut c_void,
                &mut trampoline,
                expected.as_ptr().cast(),
                expected.len(),
            )
        };
        assert_eq!(result, Ok(()));
        assert!(!trampoline.is_null());
        assert_ne!(trampoline, target);
        assert_eq!(unhook_raw(target), Err(HookError::Unsupported));
    }
}
