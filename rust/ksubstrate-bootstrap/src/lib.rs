use ksubstrate_targets::{decode_plan, verify_library, Init, LibraryIdentity, SessionPlan};
use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs;
use std::io::Read;
use std::os::fd::FromRawFd;
use std::path::Path;
use std::sync::Once;

const SENTINEL: &str = "/mnt/us/DISABLE_KSUBSTRATE";
const STATE_PLAN: &str = "/var/local/kmc/ksubstrate-runtime/state/session.plan";
static BOOTSTRAP_ONCE: Once = Once::new();

#[cfg_attr(target_os = "linux", link_section = ".init_array")]
#[used]
static KSUBSTRATE_BOOTSTRAP_INIT: extern "C" fn() = bootstrap_constructor;

extern "C" fn bootstrap_constructor() {
    BOOTSTRAP_ONCE.call_once(bootstrap);
}

fn bootstrap() {
    if Path::new(SENTINEL).exists() {
        return;
    }
    let target = match std::env::var("KSUBSTRATE_TARGET") {
        Ok(value) => value,
        Err(_) => return,
    };
    let expected = match std::env::var("KSUBSTRATE_EXPECTED_EXE") {
        Ok(value) => value,
        Err(_) => return,
    };
    let generation = match std::env::var("KSUBSTRATE_SESSION_GENERATION")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        Some(value) => value,
        None => {
            clear_inherited();
            return;
        }
    };
    if !verify_executable_identity(Path::new(&expected)) {
        clear_inherited();
        return;
    }
    let plan = match read_plan() {
        Ok(plan) => plan,
        Err(error) => {
            ksubstrate::log(&format!("bootstrap plan rejected: {error}"));
            clear_inherited();
            return;
        }
    };
    if plan.generation != generation {
        clear_inherited();
        return;
    }
    let Some(entry) = plan
        .targets
        .iter()
        .find(|entry| entry.target.id == target && entry.alias == Path::new(&expected))
    else {
        clear_inherited();
        return;
    };
    load_libraries(&entry.libraries, |library| {
        verify_library(library).and_then(|_| dlopen_tweak(&library.library, &library.init))
    });
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoadStatus {
    Loaded,
    Failed,
    Skipped,
}

/// A plan is already topologically ordered.  Keeping explicit statuses prevents
/// a dependent constructor from running after its prerequisite failed.
fn load_libraries<F>(libraries: &[LibraryIdentity], mut load: F) -> BTreeMap<String, LoadStatus>
where
    F: FnMut(&LibraryIdentity) -> Result<(), String>,
{
    ksubstrate::log(&format!(
        "bootstrap resolved load order: {}",
        libraries
            .iter()
            .map(|library| library.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    let mut statuses = BTreeMap::new();
    for library in libraries {
        let unavailable = library
            .dependencies
            .iter()
            .find(|dependency| statuses.get(*dependency) != Some(&LoadStatus::Loaded));
        if let Some(dependency) = unavailable {
            ksubstrate::log(&format!(
                "skipped {}: dependency {dependency} did not load",
                library.id
            ));
            statuses.insert(library.id.clone(), LoadStatus::Skipped);
            continue;
        }
        match load(library) {
            Ok(()) => {
                ksubstrate::log(&format!("loaded {}", library.id));
                statuses.insert(library.id.clone(), LoadStatus::Loaded);
            }
            Err(error) => {
                ksubstrate::log(&format!("failed to load {}: {error}", library.id));
                statuses.insert(library.id.clone(), LoadStatus::Failed);
            }
        }
    }
    statuses
}

fn read_plan() -> Result<SessionPlan, String> {
    if let Ok(fd) = std::env::var("KSUBSTRATE_ONESHOT_PLAN_FD") {
        let fd: i32 = fd
            .parse()
            .map_err(|_| "invalid one-shot plan fd".to_owned())?;
        let mut text = String::new();
        unsafe { fs::File::from_raw_fd(fd) }
            .read_to_string(&mut text)
            .map_err(|e| format!("read one-shot plan: {e}"))?;
        return decode_plan(&text);
    }
    decode_plan(&fs::read_to_string(STATE_PLAN).map_err(|e| format!("read session plan: {e}"))?)
}

fn verify_executable_identity(expected: &Path) -> bool {
    let actual = fs::read_link("/proc/self/exe").ok();
    actual.as_deref() == Some(expected)
}

fn clear_inherited() {
    for key in [
        "KSUBSTRATE_TARGET",
        "KSUBSTRATE_EXPECTED_EXE",
        "KSUBSTRATE_SESSION_GENERATION",
        "KSUBSTRATE_ONESHOT_PLAN_FD",
        "LD_PRELOAD",
    ] {
        std::env::remove_var(key);
    }
}

fn dlopen_tweak(path: &Path, init: &Init) -> Result<(), String> {
    let path = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| "tweak path contains NUL".to_owned())?;
    unsafe {
        let handle = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if handle.is_null() {
            let error = libc::dlerror();
            return Err(if error.is_null() {
                "dlopen returned null".to_owned()
            } else {
                std::ffi::CStr::from_ptr(error)
                    .to_string_lossy()
                    .into_owned()
            });
        }
        if matches!(init, Init::Entrypoint) {
            let symbol = CString::new("ksubstrate_init").unwrap();
            let function = libc::dlsym(handle, symbol.as_ptr());
            if function.is_null() {
                return Err("manifest requests missing ksubstrate_init".to_owned());
            }
            let function: extern "C" fn() = std::mem::transmute(function);
            function();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn library(id: &str, dependencies: &[&str]) -> LibraryIdentity {
        LibraryIdentity {
            id: id.to_owned(),
            library: PathBuf::from(format!("/{id}.so")),
            init: Init::Constructor,
            dev: 1,
            ino: 1,
            size: 1,
            digest: 1,
            dependencies: dependencies.iter().map(|id| (*id).to_owned()).collect(),
            order: 0,
        }
    }

    #[test]
    fn failed_dependency_skips_all_dependents() {
        let libraries = vec![
            library("base", &[]),
            library("child", &["base"]),
            library("grandchild", &["child"]),
        ];
        let statuses = load_libraries(&libraries, |library| {
            if library.id == "base" {
                Err("boom".to_owned())
            } else {
                Ok(())
            }
        });
        assert_eq!(statuses["base"], LoadStatus::Failed);
        assert_eq!(statuses["child"], LoadStatus::Skipped);
        assert_eq!(statuses["grandchild"], LoadStatus::Skipped);
    }
}
