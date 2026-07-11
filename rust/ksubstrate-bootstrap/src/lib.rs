use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Once;

const DEFAULT_TWEAKS_DIR: &str = "/var/local/kmc/tweaks";
const SENTINEL: &str = "/mnt/us/DISABLE_KSUBSTRATE";

#[cfg_attr(target_os = "linux", link_section = ".init_array")]
#[used]
static KSUBSTRATE_BOOTSTRAP_INIT: extern "C" fn() = bootstrap_constructor;
static BOOTSTRAP_ONCE: Once = Once::new();

extern "C" fn bootstrap_constructor() { BOOTSTRAP_ONCE.call_once(bootstrap); }

fn bootstrap() {
    if Path::new(SENTINEL).exists() { ksubstrate::log("bootstrap disabled by USB sentinel"); return; }
    let comm = process_comm().unwrap_or_else(|| "unknown".to_owned());
    let root = std::env::var("KSUBSTRATE_TWEAKS_DIR").unwrap_or_else(|_| DEFAULT_TWEAKS_DIR.to_owned());
    for (tweak, init_mode) in matching_tweaks(Path::new(&root), &comm) {
        match dlopen_tweak(&tweak, init_mode) {
            Ok(()) => ksubstrate::log(&format!("loaded tweak {} for {comm}", tweak.display())),
            Err(error) => ksubstrate::log(&format!("failed to load tweak {}: {error}", tweak.display())),
        }
    }
}

fn process_comm() -> Option<String> {
    fs::read_to_string("/proc/self/comm").ok().map(|value| value.trim().to_owned()).filter(|value| !value.is_empty())
        .or_else(|| std::env::args().next().and_then(|value| Path::new(&value).file_name().map(|name| name.to_string_lossy().into_owned())))
}

#[derive(Clone, Copy)] enum InitMode { Constructor, Entrypoint }

fn matching_tweaks(root: &Path, comm: &str) -> Vec<(PathBuf, InitMode)> {
    let Ok(metadata) = fs::symlink_metadata(root) else { return Vec::new(); };
    if metadata.file_type().is_symlink() || !metadata.is_dir() { return Vec::new(); }
    let Ok(entries) = fs::read_dir(root) else { return Vec::new(); };
    entries.filter_map(Result::ok)
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path())
        .filter(|path| fs::symlink_metadata(path).map(|metadata| !metadata.file_type().is_symlink() && metadata.is_dir()).unwrap_or(false))
        .filter_map(|path| {
            let manifest = fs::read_to_string(path.join("manifest.json")).ok()?;
            let filter = path.join(json_string(&manifest, "filter").unwrap_or_else(|| "tweak.ksfilter".to_owned()));
            let library = path.join(json_string(&manifest, "library").unwrap_or_else(|| "tweak.so".to_owned()));
            let mode = match json_string(&manifest, "initialization").as_deref() {
                Some("entrypoint") => InitMode::Entrypoint,
                Some("constructor") | None => InitMode::Constructor,
                _ => return None,
            };
            (regular_file(&filter) && regular_file(&library) && filter_matches(&filter, comm)).then_some((library, mode))
        }).collect()
}

fn regular_file(path: &Path) -> bool { fs::symlink_metadata(path).map(|metadata| !metadata.file_type().is_symlink() && metadata.is_file()).unwrap_or(false) }
fn json_string(input: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\""); let (_, rest) = input.split_once(&marker)?;
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?; let end = rest.find('"')?; let value = &rest[..end];
    (!value.contains('\\') && Path::new(value).components().count() == 1).then(|| value.to_owned())
}

fn filter_matches(path: &Path, comm: &str) -> bool {
    fs::read_to_string(path).ok().is_some_and(|contents| contents.lines().any(|line| { let token = line.split('#').next().unwrap_or("").trim(); token == "*" || comm_token_matches(token, comm) }))
}

fn comm_token_matches(token: &str, comm: &str) -> bool {
    const COMM_MAX: usize = 15;
    !token.is_empty() && (token == comm || (token.len() > COMM_MAX && comm.len() == COMM_MAX && token.as_bytes().starts_with(comm.as_bytes())))
}

fn dlopen_tweak(path: &Path, init_mode: InitMode) -> Result<(), String> {
    let cpath = CString::new(path.as_os_str().to_string_lossy().as_bytes()).map_err(|_| "path contains NUL".to_owned())?;
    unsafe {
        let handle = libc::dlopen(cpath.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if handle.is_null() {
            let error = libc::dlerror();
            return if error.is_null() { Err("dlopen returned null".to_owned()) } else { Err(std::ffi::CStr::from_ptr(error).to_string_lossy().into_owned()) };
        }
        if matches!(init_mode, InitMode::Entrypoint) { call_optional_init(handle)?; }
        Ok(())
    }
}

unsafe fn call_optional_init(handle: *mut std::os::raw::c_void) -> Result<(), String> {
    let symbol = CString::new("ksubstrate_init").expect("static string has no NUL");
    let init = libc::dlsym(handle, symbol.as_ptr());
    if init.is_null() { return Err("manifest requests ksubstrate_init but it is not exported".to_owned()); }
    let init_fn: extern "C" fn() = std::mem::transmute(init);
    init_fn();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn filters_ignore_comments_and_match_exact_comm() {
        let dir = std::env::temp_dir().join(format!("ksub-filter-{}", std::process::id())); let _ = fs::remove_dir_all(&dir); fs::create_dir_all(&dir).unwrap(); let filter = dir.join("tweak.ksfilter"); fs::write(&filter, "# comment\npillow\n").unwrap(); assert!(filter_matches(&filter, "pillow")); assert!(!filter_matches(&filter, "appmgrd")); let _ = fs::remove_dir_all(&dir);
    }
    #[test] fn filter_matches_truncated_comm() { assert!(comm_token_matches("ksubstrate-demo-target", "ksubstrate-demo")); assert!(!comm_token_matches("appmgrd", "pillow")); }
}
