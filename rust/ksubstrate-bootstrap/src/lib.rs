use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_TWEAKS_DIR: &str = "/var/local/kmc/tweaks";
const SENTINEL: &str = "/mnt/us/DISABLE_KSUBSTRATE";

#[cfg_attr(target_os = "linux", link_section = ".init_array")]
#[used]
static KSUBSTRATE_BOOTSTRAP_INIT: extern "C" fn() = bootstrap_constructor;

extern "C" fn bootstrap_constructor() {
    bootstrap();
}

fn bootstrap() {
    if Path::new(SENTINEL).exists() {
        ksubstrate::log("bootstrap disabled by USB sentinel");
        return;
    }

    let comm = process_comm().unwrap_or_else(|| "unknown".to_owned());
    let tweaks_dir = std::env::var("KSUBSTRATE_TWEAKS_DIR").unwrap_or_else(|_| DEFAULT_TWEAKS_DIR.to_owned());
    let matches = matching_tweaks(Path::new(&tweaks_dir), &comm);

    for tweak in matches {
        if let Err(error) = dlopen_tweak(&tweak) {
            ksubstrate::log(&format!("failed to load tweak {}: {error}", tweak.display()));
        } else {
            ksubstrate::log(&format!("loaded tweak {} for {comm}", tweak.display()));
        }
    }
}

fn process_comm() -> Option<String> {
    fs::read_to_string("/proc/self/comm")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::args()
                .next()
                .and_then(|value| Path::new(&value).file_name().map(|name| name.to_string_lossy().into_owned()))
        })
}

fn matching_tweaks(root: &Path, comm: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|path| {
            let filter = path.join("tweak.ksfilter");
            let library = path.join("tweak.so");
            if library.is_file() && filter_matches(&filter, comm) {
                Some(library)
            } else {
                None
            }
        })
        .collect()
}

fn filter_matches(path: &Path, comm: &str) -> bool {
    let Ok(contents) = fs::read_to_string(path) else {
        return false;
    };
    contents.lines().any(|line| {
        let token = line.split('#').next().unwrap_or("").trim();
        token == "*" || comm_token_matches(token, comm)
    })
}

/// `/proc/<pid>/comm` is truncated to `TASK_COMM_LEN - 1` (15) bytes, so a
/// filter naming a longer executable (e.g. `ksubstrate-demo-target`) would never
/// match the truncated comm (`ksubstrate-demo`). Treat a token as matching when
/// it equals the comm outright, or when the comm is exactly the 15-byte
/// truncation of the token.
fn comm_token_matches(token: &str, comm: &str) -> bool {
    const COMM_MAX: usize = 15;
    if token.is_empty() {
        return false;
    }
    if token == comm {
        return true;
    }
    token.len() > COMM_MAX && comm.len() == COMM_MAX && token.as_bytes().starts_with(comm.as_bytes())
}

fn dlopen_tweak(path: &Path) -> Result<(), String> {
    let cpath = CString::new(path.as_os_str().to_string_lossy().as_bytes())
        .map_err(|_| "path contains NUL".to_owned())?;
    unsafe {
        let handle = libc::dlopen(cpath.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if handle.is_null() {
            let error = libc::dlerror();
            if error.is_null() {
                Err("dlopen returned null".to_owned())
            } else {
                Ok(std::ffi::CStr::from_ptr(error).to_string_lossy().into_owned()).and_then(Err)
            }
        } else {
            call_optional_init(handle);
            Ok(())
        }
    }
}

/// Call the tweak's optional `ksubstrate_init(void)` entrypoint if it exports one
/// (A§4.3). Tweaks may instead use an `.init_array` constructor; both are fine.
unsafe fn call_optional_init(handle: *mut std::os::raw::c_void) {
    let Ok(symbol) = CString::new("ksubstrate_init") else {
        return;
    };
    let init = libc::dlsym(handle, symbol.as_ptr());
    if !init.is_null() {
        let init_fn: extern "C" fn() = std::mem::transmute(init);
        init_fn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_ignore_comments_and_match_exact_comm() {
        let dir = std::env::temp_dir().join(format!("ksub-filter-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let filter = dir.join("tweak.ksfilter");
        fs::write(&filter, "# comment\npillow\n").unwrap();
        assert!(filter_matches(&filter, "pillow"));
        assert!(!filter_matches(&filter, "appmgrd"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn filter_matches_truncated_comm() {
        // The kernel truncates comm to 15 bytes; a longer filter token still
        // matches the process it names.
        assert_eq!("ksubstrate-demo-target".len(), 22);
        assert!(comm_token_matches("ksubstrate-demo-target", "ksubstrate-demo"));
        assert!(comm_token_matches("pillow", "pillow"));
        assert!(!comm_token_matches("ksubstrate-demo-target", "ksubstrate-dem"));
        assert!(!comm_token_matches("appmgrd", "pillow"));
    }
}
