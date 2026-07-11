use std::fs;
#[cfg(ksubstrate_dynamic)]
use std::{ffi::CString, os::raw::c_void};

/// The function the sample tweak inline-hooks. Exported so the tweak can resolve
/// it by name at runtime; `#[inline(never)]` keeps it a real, patchable symbol.
#[no_mangle]
#[inline(never)]
pub extern "C" fn compute() -> i32 {
    41
}

fn main() {
    kindle_compat::ensure_linked();
    let value = read_value();
    println!("ksubstrate-demo-target value={value}");
    let _ = fs::write("/mnt/us/ksubstrate-demo-result.txt", format!("{value}\n"));
}

/// Call `compute` through a runtime-resolved pointer so the optimizer cannot
/// inline or constant-fold it. On device the sample tweak installs an inline
/// hook on this same symbol before `main` runs, so the value comes back hooked.
#[cfg(ksubstrate_dynamic)]
fn read_value() -> i32 {
    unsafe {
        let symbol = b"compute\0";
        let runtime_symbol = CString::new("kh_find_symbol").unwrap();
        let runtime = libc::dlsym(libc::RTLD_DEFAULT, runtime_symbol.as_ptr());
        if runtime.is_null() {
            return compute();
        }
        let find_symbol: extern "C" fn(*const c_void, *const c_void) -> *mut c_void =
            std::mem::transmute(runtime);
        let resolved = find_symbol(std::ptr::null(), symbol.as_ptr().cast());
        if resolved.is_null() {
            return compute();
        }
        let entry: extern "C" fn() -> i32 = std::mem::transmute(resolved);
        entry()
    }
}

#[cfg(not(ksubstrate_dynamic))]
fn read_value() -> i32 {
    compute()
}
