//! The one-time physical Dobby installer.  Logical chaining lives in `chain`.
use crate::HookError;
use std::os::raw::c_void;

pub const PATCH_LEN: usize = 8;
pub struct InlinePhysical {
    pub(crate) relocated_original: *mut c_void,
    hook: InlineHook,
}
unsafe impl Send for InlinePhysical {}

pub fn code_address(target: *mut c_void) -> usize {
    InlineHook::key(target)
}

fn validate_target_mode(target: *mut c_void) -> Result<usize, HookError> {
    if target.is_null() {
        return Err(HookError::InvalidArgument);
    }
    let raw = target as usize;
    #[cfg(target_arch = "arm")]
    {
        let thumb = raw & 1 != 0;
        let code = raw & !1;
        // A32 instructions are word-aligned, while Thumb instructions are
        // halfword-aligned and select state through bit zero.
        if (thumb && code & 1 != 0) || (!thumb && code & 3 != 0) {
            return Err(HookError::InvalidTargetMode);
        }
        return Ok(code);
    }
    #[cfg(not(target_arch = "arm"))]
    Ok(raw)
}

/// Reject null, mis-tagged, or non-executable replacement entries before they
/// can become a chain head.  This is intentionally separate from prologue
/// validation: a replacement needs no readable bytes, only executable code.
pub fn validate_replacement(replacement: *mut c_void) -> Result<(), HookError> {
    let addr = validate_target_mode(replacement)?;
    #[cfg(not(target_os = "linux"))]
    let _ = addr;
    #[cfg(target_os = "linux")]
    {
        let maps = std::fs::read_to_string("/proc/self/maps").map_err(|_| HookError::System)?;
        let executable = maps.lines().any(|line| {
            let mut fields = line.split_whitespace();
            let range = fields.next().unwrap_or("");
            let perms = fields.next().unwrap_or("");
            let Some((start, end)) = range.split_once('-') else { return false; };
            let (Ok(start), Ok(end)) = (usize::from_str_radix(start, 16), usize::from_str_radix(end, 16)) else { return false; };
            perms.as_bytes().get(2) == Some(&b'x') && addr >= start && addr < end
        });
        if !executable { return Err(HookError::InvalidTargetMode); }
    }
    Ok(())
}

pub fn snapshot_prologue(target: *mut c_void, len: usize) -> Result<Vec<u8>, HookError> {
    let addr = validate_target_mode(target)?;
    // The caller limits this to a small fixed signature window.  The mapping
    // check prevents a signature read from crossing into an unmapped page.
    #[cfg(not(target_os = "linux"))]
    {
        return Ok(unsafe { std::slice::from_raw_parts(addr as *const u8, len).to_vec() });
    }
    #[cfg(target_os = "linux")]
    {
        let maps = std::fs::read_to_string("/proc/self/maps").map_err(|_| HookError::System)?;
        let executable = maps.lines().any(|line| {
        let mut fields = line.split_whitespace(); let range = fields.next().unwrap_or(""); let perms = fields.next().unwrap_or("");
        let Some((start,end)) = range.split_once('-') else { return false; };
        let start = usize::from_str_radix(start,16).ok(); let end = usize::from_str_radix(end,16).ok();
        matches!((start,end), (Some(start),Some(end)) if perms.as_bytes().get(0)==Some(&b'r') && perms.as_bytes().get(2)==Some(&b'x') && addr >= start && addr.checked_add(len).map(|v| v <= end).unwrap_or(false))
    });
        if !executable {
            return Err(HookError::InvalidArgument);
        }
        Ok(unsafe { std::slice::from_raw_parts(addr as *const u8, len).to_vec() })
    }
}

pub unsafe fn install_inline_physical(
    target: *mut c_void,
    head: *mut c_void,
) -> Result<InlinePhysical, HookError> {
    let mut original = std::ptr::null_mut();
    let hook = unsafe { InlineHook::install(target, head, &mut original)? };
    Ok(InlinePhysical {
        relocated_original: original,
        hook,
    })
}

#[cfg(not(all(target_os = "linux", target_arch = "arm")))]
struct InlineHook;
#[cfg(not(all(target_os = "linux", target_arch = "arm")))]
impl InlineHook {
    fn key(target: *mut c_void) -> usize {
        target as usize & !1
    }
    unsafe fn install(
        target: *mut c_void,
        _head: *mut c_void,
        original: *mut *mut c_void,
    ) -> Result<Self, HookError> {
        unsafe {
            *original = target;
        }
        Ok(Self)
    }
}

#[cfg(all(target_os = "linux", target_arch = "arm"))]
struct InlineHook {
    addr: *mut c_void,
}
#[cfg(all(target_os = "linux", target_arch = "arm"))]
impl InlineHook {
    fn key(target: *mut c_void) -> usize {
        target as usize & !1
    }
    unsafe fn install(
        target: *mut c_void,
        head: *mut c_void,
        original: *mut *mut c_void,
    ) -> Result<Self, HookError> {
        if unsafe { dobby_sys::DobbyHook(target, head, original) } == 0 {
            Ok(Self { addr: target })
        } else {
            Err(HookError::System)
        }
    }
}
#[cfg(all(target_os = "linux", target_arch = "arm"))]
unsafe impl Send for InlineHook {}
