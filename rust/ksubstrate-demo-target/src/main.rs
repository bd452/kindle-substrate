use std::fs;
#[cfg(ksubstrate_dynamic)]
use std::os::raw::{c_char, c_void};

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
        let resolved = kh_find_symbol(std::ptr::null(), symbol.as_ptr().cast());
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

#[cfg(ksubstrate_dynamic)]
extern "C" {
    fn kh_find_symbol(image: *const c_char, name: *const c_char) -> *mut c_void;
}
