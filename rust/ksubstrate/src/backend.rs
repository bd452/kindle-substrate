use crate::HookError;
use std::collections::BTreeMap;
use std::os::raw::c_void;
use std::sync::{Mutex, OnceLock};

#[cfg(all(target_os = "linux", target_arch = "arm"))]
use dobby::InlineHook;

#[cfg(not(all(target_os = "linux", target_arch = "arm")))]
use mock::InlineHook;

/// Bytes a checked hook must verify before patching. Dobby chooses the real
/// (variable) window itself, but callers still describe at least this much
/// prologue so the checked entrypoint can refuse a drifted signature.
pub const PATCH_LEN: usize = 8;

static INLINE_HOOKS: OnceLock<Mutex<BTreeMap<usize, InlineHook>>> = OnceLock::new();

fn hooks() -> &'static Mutex<BTreeMap<usize, InlineHook>> {
    INLINE_HOOKS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub fn hook_function(
    target: *mut c_void,
    replacement: *mut c_void,
    original: *mut *mut c_void,
) -> Result<(), HookError> {
    if target.is_null() || replacement.is_null() {
        return Err(HookError::InvalidArgument);
    }

    let mut hooks = hooks().lock().map_err(|_| HookError::Poisoned)?;
    let key = InlineHook::key(target);
    if hooks.contains_key(&key) {
        return Err(HookError::AlreadyHooked);
    }
    let hook = unsafe { InlineHook::install(target, replacement, original)? };
    hooks.insert(key, hook);
    Ok(())
}

pub fn code_address(target: *mut c_void) -> usize {
    InlineHook::key(target)
}

pub fn unhook_function(target: *mut c_void) -> Result<(), HookError> {
    if target.is_null() {
        return Err(HookError::InvalidArgument);
    }

    let mut hooks = hooks().lock().map_err(|_| HookError::Poisoned)?;
    let key = InlineHook::key(target);
    let Some(mut hook) = hooks.remove(&key) else {
        return Err(HookError::NotHooked);
    };
    unsafe { hook.uninstall() }
}

#[cfg(not(all(target_os = "linux", target_arch = "arm")))]
mod mock {
    use crate::HookError;
    use std::os::raw::c_void;

    pub struct InlineHook {
        target: usize,
        replacement: usize,
    }

    impl InlineHook {
        pub fn key(target: *mut c_void) -> usize {
            target as usize
        }

        pub unsafe fn install(
            target: *mut c_void,
            replacement: *mut c_void,
            original: *mut *mut c_void,
        ) -> Result<Self, HookError> {
            if !original.is_null() {
                unsafe {
                    *original = target;
                }
            }
            Ok(Self {
                target: target as usize,
                replacement: replacement as usize,
            })
        }

        pub unsafe fn uninstall(&mut self) -> Result<(), HookError> {
            let _ = (self.target, self.replacement);
            Ok(())
        }
    }
}

// On-device inline hooking is delegated to the vendored Dobby engine, which
// relocates ARM/Thumb-2 prologues, allocates trampolines, and handles branch
// veneers and cache flushing. We keep only a thin ownership map here so the ABI
// can refuse double-hooks and support unhook/destroy.
#[cfg(all(target_os = "linux", target_arch = "arm"))]
mod dobby {
    use crate::HookError;
    use std::os::raw::c_void;
    use std::ptr;

    pub struct InlineHook {
        addr: *mut c_void,
    }

    impl InlineHook {
        pub fn key(target: *mut c_void) -> usize {
            // Normalize the Thumb bit so a target identifies one hook.
            (target as usize) & !1
        }

        pub unsafe fn install(
            target: *mut c_void,
            replacement: *mut c_void,
            original: *mut *mut c_void,
        ) -> Result<Self, HookError> {
            let mut local: *mut c_void = ptr::null_mut();
            let out = if original.is_null() {
                &mut local as *mut *mut c_void
            } else {
                original
            };
            let rc = unsafe { dobby_sys::DobbyHook(target, replacement, out) };
            if rc == 0 {
                Ok(Self { addr: target })
            } else {
                Err(HookError::System)
            }
        }

        pub unsafe fn uninstall(&mut self) -> Result<(), HookError> {
            let rc = unsafe { dobby_sys::DobbyDestroy(self.addr) };
            if rc == 0 {
                Ok(())
            } else {
                Err(HookError::System)
            }
        }
    }

    unsafe impl Send for InlineHook {}
}
