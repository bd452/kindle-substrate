//! ELF PLT/GOT import hooking (A§4.2 preferred mechanism).
//!
//! Dobby's `ImportTableReplace` plugin is Darwin/Mach-O only, so on Kindle Linux
//! we rewrite jump-slot GOT entries ourselves: parse the loaded image's ELF
//! dynamic table, find `R_*_JUMP_SLOT` relocations matching the symbol, and
//! swap the pointer. No inline prologue patch is involved.

use crate::HookError;
use std::ffi::CString;
use std::fs;
use std::os::raw::c_void;
use std::path::Path;

const EI_CLASS: usize = 4;
const ELFCLASS32: u8 = 1;
const ELFCLASS64: u8 = 2;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const DT_NULL: i64 = 0;
const DT_STRTAB: i64 = 5;
const DT_SYMTAB: i64 = 6;
const DT_RELA: i64 = 7;
const DT_REL: i64 = 17;
const DT_JMPREL: i64 = 23;
const DT_PLTRELSZ: i64 = 2;
const DT_PLTREL: i64 = 20;
const R_ARM_JUMP_SLOT: u32 = 22;
const R_X86_64_JUMP_SLOT: u32 = 7;
const R_AARCH64_JUMP_SLOT: u32 = 1026;

/// Physical import interception is deliberately image-specific.  A global
/// request can have heterogeneous loader resolutions and cannot truthfully
/// produce one `original` continuation.
pub struct ImportDiscovery {
    slots: Vec<*mut *mut c_void>,
    pub true_original: *mut c_void,
}
pub struct ImportPhysical {
    slots: Vec<*mut *mut c_void>,
}
unsafe impl Send for ImportPhysical {}

pub fn image_identity(image: &str) -> Result<String, HookError> {
    let maps = fs::read_to_string("/proc/self/maps").map_err(|_| HookError::System)?;
    // Callers commonly use /proc/self/exe.  The pathname in maps is the
    // resolved executable path, so compare canonical paths as well as the
    // literal and basename forms.
    let requested = std::fs::canonicalize(image).ok();
    let module = loaded_modules(&maps)
        .into_iter()
        .find(|module| {
            module.path == image
                || requested
                    .as_ref()
                    .map(|path| path == Path::new(&module.path))
                    .unwrap_or(false)
                || Path::new(&module.path)
                    .file_name()
                    .map(|name| name == image)
                    .unwrap_or(false)
        })
        .ok_or(HookError::System)?;
    Ok(format!(
        "{}@{:x}",
        std::fs::canonicalize(&module.path)
            .unwrap_or_else(|_| module.path.into())
            .display(),
        module.base
    ))
}

pub fn discover_import(identity: &str, symbol: &str) -> Result<ImportDiscovery, HookError> {
    let (path, base) = identity
        .rsplit_once('@')
        .and_then(|(path, base)| {
            usize::from_str_radix(base, 16)
                .ok()
                .map(|base| (path, base))
        })
        .ok_or(HookError::InvalidArgument)?;
    let bytes = fs::read(path).map_err(|_| HookError::System)?;
    let slots = jump_slot_addresses(&bytes, base, symbol)?;
    if slots.is_empty() {
        return Err(HookError::System);
    }
    // Never use a jump-slot's current contents as the continuation: before
    // lazy binding it points into the PLT resolver, which will later overwrite
    // our permanent chain.  dlsym gives us the loader-resolved implementation
    // without invoking an unknown-signature import.
    let true_original = resolve_import_symbol(path, symbol)?;
    if true_original.is_null() {
        return Err(HookError::System);
    }
    // Slots for one symbol in one image must all be replaced as one unit. They
    // need not have equal current values: lazy and eager slots legitimately
    // differ before this interception is installed.
    if slots.iter().any(|slot| slot.is_null()) {
        return Err(HookError::IncompatibleImportTargets);
    }
    Ok(ImportDiscovery {
        slots,
        true_original,
    })
}

pub fn install_import_physical(
    discovery: ImportDiscovery,
    replacement: *mut c_void,
) -> Result<ImportPhysical, HookError> {
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return Err(HookError::System);
    }
    let page_size = page_size as usize;
    let mut pages: Vec<(usize, i32)> = Vec::new();
    for slot in &discovery.slots {
        let page = *slot as usize & !(page_size - 1);
        if !pages.iter().any(|(known, _)| *known == page) {
            pages.push((page, page_protection(page)?));
        }
    }
    for (page, protection) in &pages {
        if unsafe {
            libc::mprotect(
                *page as *mut c_void,
                page_size,
                *protection | libc::PROT_WRITE,
            )
        } != 0
        {
            let _ = restore_pages(&pages, page_size);
            return Err(HookError::System);
        }
    }
    let old: Vec<*mut c_void> = discovery
        .slots
        .iter()
        .map(|slot| unsafe { **slot })
        .collect();
    for slot in &discovery.slots {
        unsafe {
            **slot = replacement;
        }
    }
    if restore_pages(&pages, page_size).is_err() {
        // Some pages may already be back to their original read-only state.
        // Re-open all of them before rollback; writing first would fault on a
        // RELRO page and leave an unreported half-installed hook.
        let reopen = make_pages_writable(&pages, page_size);
        if reopen.is_ok() {
            for (slot, value) in discovery.slots.iter().zip(old) {
                unsafe {
                    **slot = value;
                }
            }
            let _ = restore_pages(&pages, page_size);
        }
        return Err(HookError::System);
    }
    Ok(ImportPhysical {
        slots: discovery.slots,
    })
}

/// Resolve the implementation selected for an import without reading the
/// mutable GOT. `RTLD_NOLOAD` retains the target's own loader scope when the
/// platform supports it. Main executables cannot always be re-opened, so the
/// process scope is the compatible fallback for that case.
fn resolve_import_symbol(image: &str, symbol: &str) -> Result<*mut c_void, HookError> {
    let image = CString::new(image).map_err(|_| HookError::InvalidArgument)?;
    let symbol = CString::new(symbol).map_err(|_| HookError::InvalidArgument)?;
    // RTLD_NOLOAD is Linux/glibc/uClibc's value. Kindle's loader accepts it;
    // the fallback below also covers loaders that do not.
    const RTLD_NOLOAD: i32 = 0x0004;
    unsafe {
        let handle = libc::dlopen(
            image.as_ptr(),
            libc::RTLD_LAZY | libc::RTLD_LOCAL | RTLD_NOLOAD,
        );
        if !handle.is_null() {
            let resolved = libc::dlsym(handle, symbol.as_ptr()).cast::<c_void>();
            libc::dlclose(handle);
            if !resolved.is_null() {
                return Ok(resolved);
            }
        }

        let process = libc::dlopen(std::ptr::null(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
        if process.is_null() {
            return Err(HookError::System);
        }
        let resolved = libc::dlsym(process, symbol.as_ptr()).cast::<c_void>();
        libc::dlclose(process);
        if resolved.is_null() {
            Err(HookError::System)
        } else {
            Ok(resolved)
        }
    }
}

fn make_pages_writable(pages: &[(usize, i32)], size: usize) -> Result<(), HookError> {
    for (page, protection) in pages {
        if unsafe {
            libc::mprotect(
                *page as *mut c_void,
                size,
                *protection | libc::PROT_WRITE,
            )
        } != 0
        {
            return Err(HookError::System);
        }
    }
    Ok(())
}

fn page_protection(page: usize) -> Result<i32, HookError> {
    let maps = fs::read_to_string("/proc/self/maps").map_err(|_| HookError::System)?;
    for line in maps.lines() {
        let mut parts = line.split_whitespace();
        let Some(range) = parts.next() else { continue };
        let perms = parts.next().unwrap_or("");
        let Some((start, end)) = range.split_once('-') else {
            continue;
        };
        let (Ok(start), Ok(end)) = (
            usize::from_str_radix(start, 16),
            usize::from_str_radix(end, 16),
        ) else {
            continue;
        };
        if page >= start && page < end {
            let mut p = 0;
            let b = perms.as_bytes();
            if b.first() == Some(&b'r') {
                p |= libc::PROT_READ
            };
            if b.get(1) == Some(&b'w') {
                p |= libc::PROT_WRITE
            };
            if b.get(2) == Some(&b'x') {
                p |= libc::PROT_EXEC
            };
            return Ok(p);
        }
    }
    Err(HookError::System)
}
fn restore_pages(pages: &[(usize, i32)], size: usize) -> Result<(), HookError> {
    for (page, protection) in pages {
        if unsafe { libc::mprotect(*page as *mut c_void, size, *protection) } != 0 {
            return Err(HookError::System);
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct LoadedModule {
    path: String,
    base: usize,
}

fn loaded_modules(maps: &str) -> Vec<LoadedModule> {
    let mut out: Vec<LoadedModule> = Vec::new();
    for line in maps.lines() {
        let mut parts = line.split_whitespace();
        let Some(range) = parts.next() else {
            continue;
        };
        // permissions, offset, device, inode, then path
        let _perms = parts.next();
        let _offset = parts.next();
        let _dev = parts.next();
        let _inode = parts.next();
        let Some(path) = parts.next() else {
            continue;
        };
        if !path.starts_with('/') {
            continue;
        }
        let Some(start) = range
            .split('-')
            .next()
            .and_then(|value| usize::from_str_radix(value, 16).ok())
        else {
            continue;
        };
        if let Some(existing) = out.iter_mut().find(|m| m.path == path) {
            existing.base = existing.base.min(start);
        } else {
            out.push(LoadedModule {
                path: path.to_owned(),
                base: start,
            });
        }
    }
    out
}

/// Locate GOT entry addresses for `symbol`'s jump slots in an ELF image.
fn jump_slot_addresses(
    bytes: &[u8],
    mapped_base: usize,
    symbol: &str,
) -> Result<Vec<*mut *mut c_void>, HookError> {
    if bytes.len() < 16 || &bytes[0..4] != b"\x7fELF" {
        return Err(HookError::InvalidArgument);
    }
    match bytes[EI_CLASS] {
        ELFCLASS32 => jump_slots_elf32(bytes, mapped_base, symbol),
        ELFCLASS64 => jump_slots_elf64(bytes, mapped_base, symbol),
        _ => Err(HookError::InvalidArgument),
    }
}

fn jump_slots_elf32(
    bytes: &[u8],
    mapped_base: usize,
    symbol: &str,
) -> Result<Vec<*mut *mut c_void>, HookError> {
    let e_type = read_u16(bytes, 16)?;
    if e_type != ET_EXEC && e_type != ET_DYN {
        return Err(HookError::InvalidArgument);
    }
    let e_phoff = read_u32(bytes, 28)? as usize;
    let e_phentsize = read_u16(bytes, 42)? as usize;
    let e_phnum = read_u16(bytes, 44)? as usize;

    let (load_bias, dynamic_file_off, dynamic_filesz) =
        load_bias_and_dynamic32(bytes, e_phoff, e_phentsize, e_phnum, mapped_base)?;

    let dyn_entries = parse_dynamic32(bytes, dynamic_file_off, dynamic_filesz)?;
    let strtab_vaddr = dyn_tag(&dyn_entries, DT_STRTAB).ok_or(HookError::System)? as usize;
    let symtab_vaddr = dyn_tag(&dyn_entries, DT_SYMTAB).ok_or(HookError::System)? as usize;

    let jmprel = dyn_tag(&dyn_entries, DT_JMPREL).unwrap_or(0) as usize;
    let pltrelsz = dyn_tag(&dyn_entries, DT_PLTRELSZ).unwrap_or(0) as usize;
    let pltrel = dyn_tag(&dyn_entries, DT_PLTREL).unwrap_or(DT_REL as u64) as i64;

    let strtab_off = vaddr_to_offset32(bytes, e_phoff, e_phentsize, e_phnum, strtab_vaddr)?;
    let symtab_off = vaddr_to_offset32(bytes, e_phoff, e_phentsize, e_phnum, symtab_vaddr)?;

    let mut slots = Vec::new();
    if jmprel != 0 && pltrelsz != 0 {
        let jmprel_off = vaddr_to_offset32(bytes, e_phoff, e_phentsize, e_phnum, jmprel)?;
        collect_slots32(
            bytes, jmprel_off, pltrelsz, pltrel, symtab_off, strtab_off, load_bias, symbol,
            &mut slots,
        )?;
    }
    Ok(slots)
}

fn jump_slots_elf64(
    bytes: &[u8],
    mapped_base: usize,
    symbol: &str,
) -> Result<Vec<*mut *mut c_void>, HookError> {
    let e_type = read_u16(bytes, 16)?;
    if e_type != ET_EXEC && e_type != ET_DYN {
        return Err(HookError::InvalidArgument);
    }
    let e_phoff = read_u64(bytes, 32)? as usize;
    let e_phentsize = read_u16(bytes, 54)? as usize;
    let e_phnum = read_u16(bytes, 56)? as usize;

    let (load_bias, dynamic_file_off, dynamic_filesz) =
        load_bias_and_dynamic64(bytes, e_phoff, e_phentsize, e_phnum, mapped_base)?;

    let dyn_entries = parse_dynamic64(bytes, dynamic_file_off, dynamic_filesz)?;
    let strtab_vaddr = dyn_tag(&dyn_entries, DT_STRTAB).ok_or(HookError::System)? as usize;
    let symtab_vaddr = dyn_tag(&dyn_entries, DT_SYMTAB).ok_or(HookError::System)? as usize;

    let jmprel = dyn_tag(&dyn_entries, DT_JMPREL).unwrap_or(0) as usize;
    let pltrelsz = dyn_tag(&dyn_entries, DT_PLTRELSZ).unwrap_or(0) as usize;
    let pltrel = dyn_tag(&dyn_entries, DT_PLTREL).unwrap_or(DT_RELA as u64) as i64;

    let strtab_off = vaddr_to_offset64(bytes, e_phoff, e_phentsize, e_phnum, strtab_vaddr)?;
    let symtab_off = vaddr_to_offset64(bytes, e_phoff, e_phentsize, e_phnum, symtab_vaddr)?;

    let mut slots = Vec::new();
    if jmprel != 0 && pltrelsz != 0 {
        let jmprel_off = vaddr_to_offset64(bytes, e_phoff, e_phentsize, e_phnum, jmprel)?;
        collect_slots64(
            bytes, jmprel_off, pltrelsz, pltrel, symtab_off, strtab_off, load_bias, symbol,
            &mut slots,
        )?;
    }
    Ok(slots)
}

fn load_bias_and_dynamic32(
    bytes: &[u8],
    phoff: usize,
    phentsize: usize,
    phnum: usize,
    mapped_base: usize,
) -> Result<(usize, usize, usize), HookError> {
    let mut min_vaddr = None;
    let mut dynamic = None;
    for i in 0..phnum {
        let off = phoff + i * phentsize;
        let p_type = read_u32(bytes, off)?;
        let p_offset = read_u32(bytes, off + 4)? as usize;
        let p_vaddr = read_u32(bytes, off + 8)? as usize;
        let p_filesz = read_u32(bytes, off + 16)? as usize;
        if p_type == PT_LOAD {
            min_vaddr = Some(min_vaddr.map_or(p_vaddr, |v: usize| v.min(p_vaddr)));
        } else if p_type == PT_DYNAMIC {
            dynamic = Some((p_offset, p_filesz));
        }
    }
    let min_vaddr = min_vaddr.ok_or(HookError::InvalidArgument)?;
    let (dyn_off, dyn_sz) = dynamic.ok_or(HookError::System)?;
    Ok((mapped_base.wrapping_sub(min_vaddr), dyn_off, dyn_sz))
}

fn load_bias_and_dynamic64(
    bytes: &[u8],
    phoff: usize,
    phentsize: usize,
    phnum: usize,
    mapped_base: usize,
) -> Result<(usize, usize, usize), HookError> {
    let mut min_vaddr = None;
    let mut dynamic = None;
    for i in 0..phnum {
        let off = phoff + i * phentsize;
        // Elf64_Phdr: p_type@0, p_flags@4, p_offset@8, p_vaddr@16, p_filesz@32
        let p_type = read_u32(bytes, off)?;
        let p_offset = read_u64(bytes, off + 8)? as usize;
        let p_vaddr = read_u64(bytes, off + 16)? as usize;
        let p_filesz = read_u64(bytes, off + 32)? as usize;
        if p_type == PT_LOAD {
            min_vaddr = Some(min_vaddr.map_or(p_vaddr, |v: usize| v.min(p_vaddr)));
        } else if p_type == PT_DYNAMIC {
            dynamic = Some((p_offset, p_filesz));
        }
    }
    let min_vaddr = min_vaddr.ok_or(HookError::InvalidArgument)?;
    let (dyn_off, dyn_sz) = dynamic.ok_or(HookError::System)?;
    Ok((mapped_base.wrapping_sub(min_vaddr), dyn_off, dyn_sz))
}

fn parse_dynamic32(bytes: &[u8], off: usize, size: usize) -> Result<Vec<(i64, u64)>, HookError> {
    let mut out = Vec::new();
    let mut cursor = off;
    let end = off.checked_add(size).ok_or(HookError::InvalidArgument)?;
    while cursor + 8 <= end && cursor + 8 <= bytes.len() {
        let tag = read_u32(bytes, cursor)? as i32 as i64;
        let val = read_u32(bytes, cursor + 4)? as u64;
        if tag == DT_NULL {
            break;
        }
        out.push((tag, val));
        cursor += 8;
    }
    Ok(out)
}

fn parse_dynamic64(bytes: &[u8], off: usize, size: usize) -> Result<Vec<(i64, u64)>, HookError> {
    let mut out = Vec::new();
    let mut cursor = off;
    let end = off.checked_add(size).ok_or(HookError::InvalidArgument)?;
    while cursor + 16 <= end && cursor + 16 <= bytes.len() {
        let tag = read_u64(bytes, cursor)? as i64;
        let val = read_u64(bytes, cursor + 8)?;
        if tag == DT_NULL {
            break;
        }
        out.push((tag, val));
        cursor += 16;
    }
    Ok(out)
}

fn dyn_tag(entries: &[(i64, u64)], tag: i64) -> Option<u64> {
    entries.iter().find(|(t, _)| *t == tag).map(|(_, v)| *v)
}

fn collect_slots32(
    bytes: &[u8],
    rel_off: usize,
    rel_size: usize,
    rel_kind: i64,
    symtab_off: usize,
    strtab_off: usize,
    load_bias: usize,
    symbol: &str,
    out: &mut Vec<*mut *mut c_void>,
) -> Result<(), HookError> {
    let ent_size = if rel_kind == DT_RELA { 12 } else { 8 };
    let count = rel_size / ent_size;
    for i in 0..count {
        let off = rel_off + i * ent_size;
        let r_offset = read_u32(bytes, off)? as usize;
        let r_info = read_u32(bytes, off + 4)?;
        let r_type = r_info & 0xff;
        let r_sym = (r_info >> 8) as usize;
        if r_type != R_ARM_JUMP_SLOT {
            continue;
        }
        if symbol_name32(bytes, symtab_off, strtab_off, r_sym)? == symbol {
            let addr = (load_bias + r_offset) as *mut *mut c_void;
            out.push(addr);
        }
    }
    Ok(())
}

fn collect_slots64(
    bytes: &[u8],
    rel_off: usize,
    rel_size: usize,
    rel_kind: i64,
    symtab_off: usize,
    strtab_off: usize,
    load_bias: usize,
    symbol: &str,
    out: &mut Vec<*mut *mut c_void>,
) -> Result<(), HookError> {
    let ent_size = if rel_kind == DT_RELA { 24 } else { 16 };
    let count = rel_size / ent_size;
    for i in 0..count {
        let off = rel_off + i * ent_size;
        let r_offset = read_u64(bytes, off)? as usize;
        let r_info = read_u64(bytes, off + 8)?;
        let r_type = (r_info & 0xffff_ffff) as u32;
        let r_sym = (r_info >> 32) as usize;
        if r_type != R_X86_64_JUMP_SLOT && r_type != R_AARCH64_JUMP_SLOT {
            continue;
        }
        if symbol_name64(bytes, symtab_off, strtab_off, r_sym)? == symbol {
            let addr = (load_bias + r_offset) as *mut *mut c_void;
            out.push(addr);
        }
    }
    Ok(())
}

fn symbol_name32(
    bytes: &[u8],
    symtab_off: usize,
    strtab_off: usize,
    index: usize,
) -> Result<&str, HookError> {
    // Elf32_Sym is 16 bytes; st_name at +0
    let sym_off = symtab_off + index * 16;
    let st_name = read_u32(bytes, sym_off)? as usize;
    read_cstr(bytes, strtab_off + st_name)
}

fn symbol_name64(
    bytes: &[u8],
    symtab_off: usize,
    strtab_off: usize,
    index: usize,
) -> Result<&str, HookError> {
    // Elf64_Sym is 24 bytes; st_name at +0
    let sym_off = symtab_off + index * 24;
    let st_name = read_u32(bytes, sym_off)? as usize;
    read_cstr(bytes, strtab_off + st_name)
}

fn vaddr_to_offset32(
    bytes: &[u8],
    phoff: usize,
    phentsize: usize,
    phnum: usize,
    vaddr: usize,
) -> Result<usize, HookError> {
    for i in 0..phnum {
        let off = phoff + i * phentsize;
        if read_u32(bytes, off)? != PT_LOAD {
            continue;
        }
        let p_offset = read_u32(bytes, off + 4)? as usize;
        let p_vaddr = read_u32(bytes, off + 8)? as usize;
        let p_filesz = read_u32(bytes, off + 16)? as usize;
        if vaddr >= p_vaddr && vaddr < p_vaddr + p_filesz {
            return Ok(p_offset + (vaddr - p_vaddr));
        }
    }
    Err(HookError::System)
}

fn vaddr_to_offset64(
    bytes: &[u8],
    phoff: usize,
    phentsize: usize,
    phnum: usize,
    vaddr: usize,
) -> Result<usize, HookError> {
    for i in 0..phnum {
        let off = phoff + i * phentsize;
        if read_u32(bytes, off)? != PT_LOAD {
            continue;
        }
        let p_offset = read_u64(bytes, off + 8)? as usize;
        let p_vaddr = read_u64(bytes, off + 16)? as usize;
        let p_filesz = read_u64(bytes, off + 32)? as usize;
        if vaddr >= p_vaddr && vaddr < p_vaddr + p_filesz {
            return Ok(p_offset + (vaddr - p_vaddr));
        }
    }
    Err(HookError::System)
}

fn read_cstr(bytes: &[u8], off: usize) -> Result<&str, HookError> {
    if off >= bytes.len() {
        return Err(HookError::InvalidArgument);
    }
    let end = bytes[off..]
        .iter()
        .position(|&b| b == 0)
        .map(|n| off + n)
        .ok_or(HookError::InvalidArgument)?;
    std::str::from_utf8(&bytes[off..end]).map_err(|_| HookError::InvalidArgument)
}

fn read_u16(bytes: &[u8], off: usize) -> Result<u16, HookError> {
    let slice = bytes.get(off..off + 2).ok_or(HookError::InvalidArgument)?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], off: usize) -> Result<u32, HookError> {
    let slice = bytes.get(off..off + 4).ok_or(HookError::InvalidArgument)?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u64(bytes: &[u8], off: usize) -> Result<u64, HookError> {
    let slice = bytes.get(off..off + 8).ok_or(HookError::InvalidArgument)?;
    Ok(u64::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_elf() {
        assert_eq!(
            jump_slot_addresses(b"not elf", 0x1000, "open"),
            Err(HookError::InvalidArgument)
        );
    }

    #[test]
    fn loaded_modules_dedupes_by_path_and_keeps_lowest_base() {
        let maps = "\
00010000-00020000 r-xp 00000000 fe:01 1  /lib/libc.so.6\n\
00020000-00030000 r--p 00010000 fe:01 1  /lib/libc.so.6\n\
00040000-00050000 r-xp 00000000 fe:01 2  /usr/bin/reader\n";
        let modules = loaded_modules(maps);
        assert_eq!(modules.len(), 2);
        assert_eq!(modules[0].path, "/lib/libc.so.6");
        assert_eq!(modules[0].base, 0x0001_0000);
        assert_eq!(modules[1].path, "/usr/bin/reader");
        assert_eq!(modules[1].base, 0x0004_0000);
    }

    #[test]
    fn resolves_an_import_without_using_a_jump_slot_value() {
        // The main executable cannot necessarily be dlopen'ed on every
        // loader, which exercises the process-scope fallback used for it.
        let resolved = resolve_import_symbol("/proc/self/exe", "malloc").unwrap();
        assert!(!resolved.is_null());
    }

    /// Minimal ELF32 ET_DYN with one JUMP_SLOT for `open`.
    #[test]
    fn finds_arm_jump_slot_in_synthetic_elf() {
        // Layout:
        //  0x00: Ehdr (52 bytes)
        //  0x34: Phdr LOAD
        //  0x54: Phdr DYNAMIC
        //  0x80: .dynstr "\0open\0"
        //  0x88: .dynsym (2 entries: null + open)
        //  0xA8: .rel.plt (1 entry)
        //  0xB0: .dynamic
        //  0x200: GOT slot storage (file offset = vaddr for simplicity; min_vaddr=0)
        let mut elf = vec![0u8; 0x220];
        // e_ident
        elf[0..4].copy_from_slice(b"\x7fELF");
        elf[EI_CLASS] = ELFCLASS32;
        elf[5] = 1; // ELFDATA2LSB
        elf[6] = 1; // EV_CURRENT
                    // e_type ET_DYN, e_machine EM_ARM
        elf[16..18].copy_from_slice(&ET_DYN.to_le_bytes());
        elf[18..20].copy_from_slice(&40u16.to_le_bytes());
        // e_phoff=0x34, e_ehsize=52, e_phentsize=32, e_phnum=2
        elf[28..32].copy_from_slice(&0x34u32.to_le_bytes());
        elf[40..42].copy_from_slice(&52u16.to_le_bytes());
        elf[42..44].copy_from_slice(&32u16.to_le_bytes());
        elf[44..46].copy_from_slice(&2u16.to_le_bytes());

        // LOAD: offset=0, vaddr=0, filesz=0x220, memsz=0x220
        write_phdr32(&mut elf, 0x34, PT_LOAD, 0, 0, 0x220, 0x220);
        // DYNAMIC: offset=0xB0, vaddr=0xB0, filesz=0x40
        write_phdr32(&mut elf, 0x54, PT_DYNAMIC, 0xB0, 0xB0, 0x40, 0x40);

        // dynstr
        elf[0x80] = 0;
        elf[0x81..0x86].copy_from_slice(b"open\0");

        // dynsym[0] null, dynsym[1] open (st_name=1)
        // entry at 0x88+16=0x98
        elf[0x98..0x9C].copy_from_slice(&1u32.to_le_bytes());

        // rel.plt: r_offset=0x200 (GOT), r_info = (sym<<8)|R_ARM_JUMP_SLOT
        let r_info = (1u32 << 8) | R_ARM_JUMP_SLOT;
        elf[0xA8..0xAC].copy_from_slice(&0x200u32.to_le_bytes());
        elf[0xAC..0xB0].copy_from_slice(&r_info.to_le_bytes());

        // dynamic entries
        write_dyn32(&mut elf, 0xB0, DT_STRTAB, 0x80);
        write_dyn32(&mut elf, 0xB8, DT_SYMTAB, 0x88);
        write_dyn32(&mut elf, 0xC0, DT_JMPREL, 0xA8);
        write_dyn32(&mut elf, 0xC8, DT_PLTRELSZ, 8);
        write_dyn32(&mut elf, 0xD0, DT_PLTREL, DT_REL as u64);
        write_dyn32(&mut elf, 0xD8, DT_NULL, 0);

        let mapped_base = 0x1000_0000usize;
        let slots = jump_slot_addresses(&elf, mapped_base, "open").unwrap();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0] as usize, mapped_base + 0x200);
    }

    fn write_phdr32(
        buf: &mut [u8],
        at: usize,
        p_type: u32,
        offset: u32,
        vaddr: u32,
        filesz: u32,
        memsz: u32,
    ) {
        buf[at..at + 4].copy_from_slice(&p_type.to_le_bytes());
        buf[at + 4..at + 8].copy_from_slice(&offset.to_le_bytes());
        buf[at + 8..at + 12].copy_from_slice(&vaddr.to_le_bytes());
        buf[at + 12..at + 16].copy_from_slice(&vaddr.to_le_bytes()); // p_paddr
        buf[at + 16..at + 20].copy_from_slice(&filesz.to_le_bytes());
        buf[at + 20..at + 24].copy_from_slice(&memsz.to_le_bytes());
    }

    fn write_dyn32(buf: &mut [u8], at: usize, tag: i64, val: u64) {
        buf[at..at + 4].copy_from_slice(&(tag as i32 as u32).to_le_bytes());
        buf[at + 4..at + 8].copy_from_slice(&(val as u32).to_le_bytes());
    }
}
