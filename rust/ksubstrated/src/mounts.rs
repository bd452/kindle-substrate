//! Linux mount syscalls.  This module never mutates system executables and the
//! only created file is an O_EXCL alias target beneath a verified mounts tmpfs.
use crate::layout::{MountTmpfs, OriginalAlias, StateTmpfs, SystemExecutable, WrapperAsset};
use std::ffi::CString;
use std::fs;
use std::io::Read;
use std::os::fd::FromRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
const MNT_DETACH: libc::c_int = 2;
const MS_RDONLY: libc::c_ulong = 1;
const MS_NOSUID: libc::c_ulong = 2;
const MS_NODEV: libc::c_ulong = 4;
const MS_NOEXEC: libc::c_ulong = 8;
const MS_REMOUNT: libc::c_ulong = 32;
const MS_BIND: libc::c_ulong = 4096;

fn c(path: &Path) -> Result<CString, String> { CString::new(path.as_os_str().as_bytes()).map_err(|_| "path contains NUL".to_owned()) }
fn errno(op: &str) -> String { format!("{op}: {}", std::io::Error::last_os_error()) }

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn mount(source: *const libc::c_char, target: *const libc::c_char, fstype: *const libc::c_char, flags: libc::c_ulong, data: *const libc::c_void) -> libc::c_int;
    fn umount2(target: *const libc::c_char, flags: libc::c_int) -> libc::c_int;
}

#[cfg(target_os = "linux")]
fn mount_call(source: *const libc::c_char, target: *const libc::c_char, fstype: *const libc::c_char, flags: libc::c_ulong, data: *const libc::c_void) -> Result<(), String> {
    if unsafe { mount(source, target, fstype, flags, data) } != 0 { Err(errno("mount")) } else { Ok(()) }
}

#[cfg(not(target_os = "linux"))]
fn mount_call(_: *const libc::c_char, _: *const libc::c_char, _: *const libc::c_char, _: libc::c_ulong, _: *const libc::c_void) -> Result<(), String> { Err("mount sessions require Linux".to_owned()) }

pub fn mount_runtime_tmpfs(mounts: &MountTmpfs, state: &StateTmpfs) -> Result<(), String> {
    ensure_empty_unmounted(mounts.path())?;
    ensure_empty_unmounted(state.path())?;
    mount_tmpfs(mounts.path(), false)?;
    if let Err(error) = mount_tmpfs(state.path(), true) {
        let _ = umount(mounts.path());
        return Err(error);
    }
    if let Err(error) = verify_tmpfs(mounts.path(), false).and_then(|_| verify_tmpfs(state.path(), true)) {
        let _ = umount(state.path());
        let _ = umount(mounts.path());
        return Err(error);
    }
    Ok(())
}

pub fn remount_fresh_mounts_tmpfs(mounts: &MountTmpfs) -> Result<(), String> {
    if is_mountpoint(mounts.path())? { umount(mounts.path())?; }
    ensure_empty_unmounted(mounts.path())?;
    mount_tmpfs(mounts.path(), false)?;
    verify_tmpfs(mounts.path(), false)
}

fn ensure_empty_unmounted(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| format!("create runtime mount point {}: {e}", path.display()))?;
    if is_mountpoint(path)? { return Err(format!("runtime mount point remains mounted: {}; reboot required", path.display())); }
    if fs::read_dir(path).map_err(|e| format!("inspect {}: {e}", path.display()))?.next().is_some() {
        return Err(format!("runtime mount point is not empty: {}; reboot required", path.display()));
    }
    Ok(())
}

fn tmpfs_mount_spec(require_noexec: bool) -> (libc::c_ulong, &'static str) {
    let mut flags = MS_NODEV | MS_NOSUID;
    if require_noexec { flags |= MS_NOEXEC; }
    // VFS options belong in mount flags. tmpfs receives only filesystem data.
    (flags, "mode=0700,size=4m")
}

fn mount_tmpfs(target: &Path, require_noexec: bool) -> Result<(), String> {
    let source = CString::new("tmpfs").unwrap();
    let fs_type = CString::new("tmpfs").unwrap();
    let target = c(target)?;
    let (flags, data) = tmpfs_mount_spec(require_noexec);
    let data = CString::new(data).unwrap();
    mount_call(source.as_ptr(), target.as_ptr(), fs_type.as_ptr(), flags, data.as_ptr().cast())
}

pub fn umount(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    { let path = c(path)?; if unsafe { umount2(path.as_ptr(), MNT_DETACH) } != 0 { Err(errno("umount")) } else { Ok(()) } }
    #[cfg(not(target_os = "linux"))]
    { let _ = path; Ok(()) }
}

pub fn bind_original_readonly(executable: &SystemExecutable, alias: &OriginalAlias) -> Result<(), String> {
    bind_readonly(executable.path(), alias.path())
}

pub fn bind_wrapper_readonly(wrapper: &WrapperAsset, executable: &SystemExecutable) -> Result<(), String> {
    bind_readonly(wrapper.path(), executable.path())
}

fn bind_readonly(source: &Path, target: &Path) -> Result<(), String> {
    let source = c(source)?;
    let target = c(target)?;
    mount_call(source.as_ptr(), target.as_ptr(), std::ptr::null(), MS_BIND, std::ptr::null())?;
    if let Err(error) = mount_call(std::ptr::null(), target.as_ptr(), std::ptr::null(), MS_BIND | MS_REMOUNT | MS_RDONLY, std::ptr::null()) {
        let _ = umount_path_cstring(&target);
        return Err(format!("remount bind read-only: {error}"));
    }
    verify_readonly_mount_path(&target)?;
    Ok(())
}

fn umount_path_cstring(path: &CString) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    { if unsafe { umount2(path.as_ptr(), MNT_DETACH) } != 0 { Err(errno("umount")) } else { Ok(()) } }
    #[cfg(not(target_os = "linux"))]
    { let _ = path; Ok(()) }
}

pub fn create_alias_target(mounts: &MountTmpfs, alias: &OriginalAlias) -> Result<(), String> {
    verify_tmpfs(mounts.path(), false)?;
    let parent = alias.path().parent().ok_or_else(|| "alias has no parent".to_owned())?;
    fs::create_dir_all(parent).map_err(|e| format!("create alias parent: {e}"))?;
    let path = c(alias.path())?;
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_RDWR, 0o700) };
    if fd < 0 { return Err(errno("create alias target")); }
    unsafe { drop(fs::File::from_raw_fd(fd)); }
    Ok(())
}

pub fn is_mountpoint(path: &Path) -> Result<bool, String> {
    Ok(mount_info()?.iter().any(|entry| entry.point == path))
}

pub fn verify_tmpfs(path: &Path, require_noexec: bool) -> Result<(), String> {
    let entry = mount_info()?.into_iter().find(|entry| entry.point == path)
        .ok_or_else(|| format!("{} is not mounted", path.display()))?;
    if entry.fs_type != "tmpfs" { return Err(format!("{} is not tmpfs", path.display())); }
    let options = split_options(&entry.options);
    for required in ["nodev", "nosuid"] { if !options.iter().any(|value| *value == required) { return Err(format!("tmpfs missing {required}")); } }
    if require_noexec {
        if !options.iter().any(|value| *value == "noexec") { return Err("state tmpfs missing noexec".to_owned()); }
    } else if options.iter().any(|value| *value == "noexec") { return Err("mounts tmpfs must be executable".to_owned()); }
    Ok(())
}

fn verify_readonly_mount_path(path: &CString) -> Result<(), String> {
    let path = PathBuf::from(std::ffi::OsStr::from_bytes(path.as_bytes()));
    let entry = mount_info()?.into_iter().find(|entry| entry.point == path)
        .ok_or_else(|| format!("{} is not a mount point", path.display()))?;
    if split_options(&entry.options).iter().any(|option| *option == "ro") { Ok(()) } else { Err(format!("{} is not read-only", path.display())) }
}

fn split_options(options: &str) -> Vec<&str> { options.split(',').collect() }

#[derive(Debug, Clone)]
pub struct MountInfo { pub point: PathBuf, pub options: String, pub fs_type: String }

pub fn mount_info() -> Result<Vec<MountInfo>, String> {
    let mut input = String::new();
    fs::File::open("/proc/self/mountinfo").map_err(|e| format!("open mountinfo: {e}"))?
        .read_to_string(&mut input).map_err(|e| format!("read mountinfo: {e}"))?;
    input.lines().map(parse_mountinfo_line).collect()
}

fn parse_mountinfo_line(line: &str) -> Result<MountInfo, String> {
    let (left, right) = line.split_once(" - ").ok_or_else(|| "malformed mountinfo separator".to_owned())?;
    let left: Vec<_> = left.split_whitespace().collect();
    let right: Vec<_> = right.split_whitespace().collect();
    Ok(MountInfo {
        point: unescape(left.get(4).ok_or_else(|| "malformed mountinfo target".to_owned())?),
        options: left.get(5).ok_or_else(|| "malformed mountinfo options".to_owned())?.to_string(),
        fs_type: right.first().ok_or_else(|| "malformed mountinfo fs type".to_owned())?.to_string(),
    })
}

fn unescape(value: &str) -> PathBuf {
    PathBuf::from(value.replace("\\040", " ").replace("\\011", "\t").replace("\\012", "\n").replace("\\134", "\\"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_mountinfo_flags() {
        let entry = parse_mountinfo_line("24 23 0:22 / /run rw,nosuid,nodev - tmpfs tmpfs rw,size=1m").unwrap();
        assert_eq!(entry.point, Path::new("/run"));
        assert!(split_options(&entry.options).contains(&"nodev"));
    }

    #[test]
    fn tmpfs_vfs_options_use_mount_flags() {
        let (mounts_flags, mounts_data) = tmpfs_mount_spec(false);
        assert_eq!(mounts_flags, MS_NODEV | MS_NOSUID);
        assert_eq!(mounts_data, "mode=0700,size=4m");

        let (state_flags, state_data) = tmpfs_mount_spec(true);
        assert_eq!(state_flags, MS_NODEV | MS_NOSUID | MS_NOEXEC);
        assert_eq!(state_data, "mode=0700,size=4m");
    }
}
