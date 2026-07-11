//! ABI-transparent tail-jump thunks. A thunk only loads a pointer and jumps;
//! it never enters Rust while the intercepted function is executing.
use crate::HookError;
use std::os::raw::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

/// Bound process-lifetime hook memory. The arena has no free operation because
/// an ABI continuation can outlive its registering tweak.
const MAX_PROCESS_THUNKS: usize = 4096;
const MAX_SLOTS_PER_PAGE: usize = 128;

pub struct HeadThunk {
    entry: NonNull<c_void>,
    destination: NonNull<AtomicUsize>,
}
pub struct TailThunk {
    entry: NonNull<c_void>,
    destination: NonNull<AtomicUsize>,
}

unsafe impl Send for HeadThunk {}
unsafe impl Send for TailThunk {}

struct ArenaPage {
    // Keep the complete mapping alive for process lifetime. Code is its first
    // page (RX after initialization); data is the immediately following RW
    // page. Every slot uses matching offsets in each page.
    _base: NonNull<c_void>,
    code: NonNull<u8>,
    data: NonNull<AtomicUsize>,
    capacity: usize,
    used: usize,
    thumb: bool,
}
unsafe impl Send for ArenaPage {}

struct Arena {
    pages: Vec<ArenaPage>,
    allocated: usize,
}
static ARENA: OnceLock<Mutex<Arena>> = OnceLock::new();

fn arena() -> &'static Mutex<Arena> {
    ARENA.get_or_init(|| {
        Mutex::new(Arena {
            pages: Vec::new(),
            allocated: 0,
        })
    })
}

fn page_size() -> Result<usize, HookError> {
    let value = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if value <= 0 {
        Err(HookError::System)
    } else {
        Ok(value as usize)
    }
}

fn slot_size() -> Result<usize, HookError> {
    #[cfg(target_arch = "x86_64")]
    {
        return Ok(6);
    }
    #[cfg(target_arch = "arm")]
    {
        return Ok(4);
    }
    #[cfg(target_arch = "aarch64")]
    {
        return Ok(8);
    }
    #[allow(unreachable_code)]
    Err(HookError::Unsupported)
}

fn allocate_page(thumb: bool) -> Result<ArenaPage, HookError> {
    #[cfg(not(target_arch = "arm"))]
    let _ = thumb;
    let page = page_size()?;
    let code_size = slot_size()?;
    let capacity = MAX_SLOTS_PER_PAGE
        .min(page / code_size)
        .min(page / std::mem::size_of::<AtomicUsize>());
    if capacity == 0 || page.checked_mul(2).is_none() {
        return Err(HookError::ThunkAllocationFailed);
    }
    let len = page * 2;
    let base = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        )
    };
    if base == libc::MAP_FAILED {
        return Err(HookError::ThunkAllocationFailed);
    }
    let Some(base) = NonNull::new(base) else {
        return Err(HookError::ThunkAllocationFailed);
    };
    let code = base.as_ptr().cast::<u8>();
    let data = unsafe { code.add(page).cast::<AtomicUsize>() };

    // Emit all slots before sealing the code page. Later allocations only
    // write their own cell in the adjacent RW page, so existing entrypoints
    // remain executable while the arena grows.
    for index in 0..capacity {
        unsafe {
            data.add(index).write(AtomicUsize::new(0));
        }
        if let Err(error) = emit_slot(code, data.cast::<u8>(), page, index, thumb) {
            unsafe { libc::munmap(base.as_ptr(), len) };
            return Err(error);
        }
    }
    if unsafe { libc::mprotect(code.cast(), page, libc::PROT_READ | libc::PROT_EXEC) } != 0 {
        unsafe { libc::munmap(base.as_ptr(), len) };
        return Err(HookError::System);
    }
    flush_instruction_cache(code, capacity * code_size);
    Ok(ArenaPage {
        _base: base,
        code: NonNull::new(code).expect("mmap code is non-null"),
        data: NonNull::new(data).expect("mmap data is non-null"),
        capacity,
        used: 0,
        thumb,
    })
}

/// Emit a slot at a fixed code-page offset which jumps through the matching
/// pointer-sized cell in the immediately following data page.
fn emit_slot(
    code: *mut u8,
    _data: *mut u8,
    page: usize,
    index: usize,
    thumb: bool,
) -> Result<(), HookError> {
    #[cfg(not(target_arch = "arm"))]
    let _ = thumb;
    #[cfg(target_arch = "x86_64")]
    {
        let offset = index * 6;
        let next_ip = offset + 6;
        let cell = page + index * std::mem::size_of::<usize>();
        let displacement = isize::try_from(cell)
            .and_then(|cell| isize::try_from(next_ip).map(|next| cell - next))
            .ok()
            .and_then(|value| i32::try_from(value).ok())
            .ok_or(HookError::ThunkAllocationFailed)?;
        unsafe {
            code.add(offset).write(0xff);
            code.add(offset + 1).write(0x25); // jmp qword ptr [rip + disp32]
            std::ptr::copy_nonoverlapping(
                displacement.to_le_bytes().as_ptr(),
                code.add(offset + 2),
                4,
            );
        }
        return Ok(());
    }
    #[cfg(target_arch = "arm")]
    {
        let offset = index * 4;
        let cell = page + index * std::mem::size_of::<u32>();
        let pc = if thumb { offset + 4 } else { offset + 8 };
        let displacement = cell
            .checked_sub(pc)
            .ok_or(HookError::ThunkAllocationFailed)?;
        if displacement > 0x0fff {
            return Err(HookError::ThunkAllocationFailed);
        }
        let instruction = if thumb {
            // ldr.w pc, [pc, #imm12] (T2)
            ((0xf000u32 | displacement as u32) << 16) | 0xf8df
        } else {
            // ldr pc, [pc, #imm12] (A32)
            0xe59f_f000u32 | displacement as u32
        };
        unsafe {
            std::ptr::copy_nonoverlapping(instruction.to_le_bytes().as_ptr(), code.add(offset), 4);
        }
        return Ok(());
    }
    #[cfg(target_arch = "aarch64")]
    {
        let offset = index * 8;
        let cell = page + index * std::mem::size_of::<usize>();
        let distance = isize::try_from(cell)
            .and_then(|cell| isize::try_from(offset).map(|code| cell - code))
            .ok()
            .ok_or(HookError::ThunkAllocationFailed)?;
        if distance % 4 != 0 {
            return Err(HookError::ThunkAllocationFailed);
        }
        let immediate = distance / 4;
        if !(-(1 << 18)..(1 << 18)).contains(&immediate) {
            return Err(HookError::ThunkAllocationFailed);
        }
        let ldr_x16_literal = 0x5800_0010u32 | ((immediate as u32 & 0x7ffff) << 5);
        unsafe {
            std::ptr::copy_nonoverlapping(
                ldr_x16_literal.to_le_bytes().as_ptr(),
                code.add(offset),
                4,
            );
            std::ptr::copy_nonoverlapping(
                0xd61f_0200u32.to_le_bytes().as_ptr(),
                code.add(offset + 4),
                4,
            );
        }
        return Ok(());
    }
    #[allow(unreachable_code, unused_variables)]
    Err(HookError::Unsupported)
}

/// Make freshly emitted instructions visible before their address escapes.
/// ARM has split instruction/data caches, so mprotect alone is insufficient.
#[cfg(all(target_os = "linux", target_arch = "arm"))]
fn flush_instruction_cache(start: *mut u8, len: usize) {
    unsafe extern "C" {
        fn __clear_cache(begin: *mut u8, end: *mut u8);
    }
    unsafe { __clear_cache(start, start.add(len)) };
}

#[cfg(not(all(target_os = "linux", target_arch = "arm")))]
fn flush_instruction_cache(_start: *mut u8, _len: usize) {}

fn make(
    destination: usize,
    thumb: bool,
) -> Result<(NonNull<c_void>, NonNull<AtomicUsize>), HookError> {
    let mut arena = arena().lock().map_err(|_| HookError::Poisoned)?;
    if arena.allocated == MAX_PROCESS_THUNKS {
        return Err(HookError::ThunkAllocationFailed);
    }
    let page_index = match arena
        .pages
        .iter()
        .position(|page| page.thumb == thumb && page.used < page.capacity)
    {
        Some(index) => index,
        None => {
            arena.pages.push(allocate_page(thumb)?);
            arena.pages.len() - 1
        }
    };
    arena.allocated += 1;
    let page = &mut arena.pages[page_index];
    let index = page.used;
    page.used += 1;
    let cell = unsafe { page.data.as_ptr().add(index) };
    unsafe { cell.write(AtomicUsize::new(destination)) };
    let entry = unsafe { page.code.as_ptr().add(index * slot_size()?) }.cast::<c_void>();
    let tag = usize::from(thumb);
    let entry =
        NonNull::new((entry as usize | tag) as *mut c_void).expect("arena entry is non-null");
    Ok((entry, NonNull::new(cell).expect("arena cell is non-null")))
}

impl HeadThunk {
    pub fn new(destination: usize) -> Result<Self, HookError> {
        Self::new_for_mode(destination, false)
    }
    pub fn new_for_mode(destination: usize, thumb: bool) -> Result<Self, HookError> {
        let (entry, destination) = make(destination, thumb)?;
        Ok(Self { entry, destination })
    }
    pub fn entry(&self) -> *mut c_void {
        self.entry.as_ptr()
    }
    pub fn destination(&self) -> usize {
        unsafe { self.destination.as_ref() }.load(Ordering::Acquire)
    }
    pub fn publish(&self, destination: usize) {
        unsafe { self.destination.as_ref() }.store(destination, Ordering::Release);
    }
}
impl TailThunk {
    pub fn new(destination: usize) -> Result<Self, HookError> {
        Self::new_for_mode(destination, false)
    }
    pub fn new_for_mode(destination: usize, thumb: bool) -> Result<Self, HookError> {
        let (entry, destination) = make(destination, thumb)?;
        Ok(Self { entry, destination })
    }
    pub fn entry(&self) -> *mut c_void {
        self.entry.as_ptr()
    }
    /// This is only used while preparing a first installation, before the
    /// thunk is visible to the target or a caller.
    pub fn publish(&self, destination: usize) {
        unsafe { self.destination.as_ref() }.store(destination, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn first() -> i32 {
        17
    }
    extern "C" fn second() -> i32 {
        29
    }

    #[test]
    fn x86_head_thunk_jumps_through_its_pointer_cell() {
        let thunk = HeadThunk::new(first as *const () as usize).unwrap();
        let invoke: extern "C" fn() -> i32 = unsafe { std::mem::transmute(thunk.entry()) };
        assert_eq!(invoke(), 17);
        thunk.publish(second as *const () as usize);
        assert_eq!(invoke(), 29);
    }

    #[test]
    fn arena_reuses_a_paired_page_for_multiple_thunks() {
        let first = HeadThunk::new(first as *const () as usize).unwrap();
        let second = TailThunk::new(second as *const () as usize).unwrap();
        let page = page_size().unwrap();
        assert_eq!(
            first.entry() as usize & !(page - 1),
            second.entry() as usize & !(page - 1)
        );
    }
}
