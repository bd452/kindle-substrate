//! Fileless KPM app registry for firmware-specific Home adapters.
//!
//! This library intentionally knows nothing about private Kindle Home classes.
//! A small adapter for a verified firmware can call the exported C functions to
//! enumerate synthetic books and dispatch their activation, while this shared
//! layer owns package crawling, validation, and safe process launch.

use ksubstrate_targets::{discover_home_apps, HomeApp};
use std::ffi::{CStr, CString};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::raw::{c_char, c_int};
use std::process::Command;
use std::sync::{OnceLock, RwLock};

#[derive(Clone)]
struct Entry {
    app: HomeApp,
    id: CString,
    name: CString,
    subtitle: CString,
    icon: CString,
}

impl Entry {
    fn new(app: HomeApp) -> Option<Self> {
        Some(Self {
            id: CString::new(app.synthetic_id.as_str()).ok()?,
            name: CString::new(app.name.as_str()).ok()?,
            subtitle: CString::new(app.subtitle.as_deref().unwrap_or("")).ok()?,
            icon: CString::new(app.icon.as_os_str().as_encoded_bytes()).ok()?,
            app,
        })
    }
}

// C callers receive raw string pointers. Keep published snapshots alive for the
// process lifetime so a concurrent reload cannot invalidate a pointer after the
// getter releases its read lock. Reloads are package-lifecycle events and the
// snapshots are intentionally tiny.
static ENTRIES: OnceLock<RwLock<&'static [Entry]>> = OnceLock::new();

fn entries() -> &'static RwLock<&'static [Entry]> {
    ENTRIES.get_or_init(|| RwLock::new(&[]))
}

fn reload() -> Result<usize, String> {
    let discovered = discover_home_apps()?;
    let discovered_entries = discovered
        .into_iter()
        .filter_map(Entry::new)
        .collect::<Vec<_>>();
    let count = discovered_entries.len();
    let discovered_entries = Box::leak(discovered_entries.into_boxed_slice());
    *entries()
        .write()
        .map_err(|_| "home app registry lock poisoned".to_owned())? = discovered_entries;
    log(&format!("home apps: loaded {count} KPM app entries"));
    Ok(count)
}

fn log(message: &str) {
    if let Ok(path) = std::env::var("KSUBSTRATE_LOG") {
        let _ = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| writeln!(file, "{message}"));
    } else {
        eprintln!("ksubstrate: {message}");
    }
}

fn launch(synthetic_id: &str) -> Result<(), String> {
    let entry = entries()
        .read()
        .map_err(|_| "home app registry lock poisoned".to_owned())?
        .iter()
        .find(|entry| entry.app.synthetic_id == synthetic_id)
        .cloned()
        .ok_or_else(|| "unknown KPM Home app".to_owned())?;
    Command::new(&entry.app.executable)
        .args(&entry.app.arguments)
        .current_dir(&entry.app.working_directory)
        // Home itself is preloaded. App launchers are not framework targets, so
        // do not accidentally carry the wrapper identity into the child.
        .env_remove("LD_PRELOAD")
        .env_remove("KSUBSTRATE_TARGET")
        .env_remove("KSUBSTRATE_EXPECTED_EXE")
        .env_remove("KSUBSTRATE_SESSION_GENERATION")
        .env_remove("KSUBSTRATE_ONESHOT_PLAN_FD")
        .spawn()
        .map_err(|error| format!("launch {}/{}: {error}", entry.app.package_id, entry.app.app_id))?;
    Ok(())
}

#[cfg_attr(target_os = "linux", link_section = ".init_array")]
#[used]
static HOME_APPS_INIT: extern "C" fn() = init;

extern "C" fn init() {
    kindle_compat::ensure_linked();
    if let Err(error) = reload() {
        log(&format!("home apps: initial scan failed: {error}"));
    }
}

/// Rescan package manifests. Returns the number of available app entries, or
/// `-1` when the package root could not be read.
#[no_mangle]
pub extern "C" fn ksubstrate_home_apps_reload() -> c_int {
    match reload() {
        Ok(count) => count.try_into().unwrap_or(c_int::MAX),
        Err(error) => {
            log(&format!("home apps: reload failed: {error}"));
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn ksubstrate_home_apps_count() -> c_int {
    entries()
        .read()
        .map(|entries| entries.len().try_into().unwrap_or(c_int::MAX))
        .unwrap_or(-1)
}

/// Stable `kpm-app://<package-id>/<app-id>` identifier for a synthetic Home
/// item. Returned strings remain valid for the process lifetime, including
/// across `ksubstrate_home_apps_reload` calls.
#[no_mangle]
pub extern "C" fn ksubstrate_home_apps_id(index: c_int) -> *const c_char {
    usize::try_from(index)
        .ok()
        .and_then(|index| {
            entries()
                .read()
                .ok()
                .and_then(|entries| entries.get(index).map(|entry| entry.id.as_ptr()))
        })
        .unwrap_or(std::ptr::null())
}

#[no_mangle]
pub extern "C" fn ksubstrate_home_apps_name(index: c_int) -> *const c_char {
    usize::try_from(index)
        .ok()
        .and_then(|index| {
            entries()
                .read()
                .ok()
                .and_then(|entries| entries.get(index).map(|entry| entry.name.as_ptr()))
        })
        .unwrap_or(std::ptr::null())
}

#[no_mangle]
pub extern "C" fn ksubstrate_home_apps_subtitle(index: c_int) -> *const c_char {
    usize::try_from(index)
        .ok()
        .and_then(|index| {
            entries()
                .read()
                .ok()
                .and_then(|entries| entries.get(index).map(|entry| entry.subtitle.as_ptr()))
        })
        .unwrap_or(std::ptr::null())
}

/// Absolute validated icon path. The adapter reads it; this library never
/// copies artwork into Documents or the Kindle content catalog.
#[no_mangle]
pub extern "C" fn ksubstrate_home_apps_icon(index: c_int) -> *const c_char {
    usize::try_from(index)
        .ok()
        .and_then(|index| {
            entries()
                .read()
                .ok()
                .and_then(|entries| entries.get(index).map(|entry| entry.icon.as_ptr()))
        })
        .unwrap_or(std::ptr::null())
}

/// Launch an item by its synthetic ID. The adapter should call this only for
/// IDs it created; all executable paths were validated during the manifest scan.
///
/// # Safety
///
/// `id` must be null or point to a readable, NUL-terminated C string for the
/// duration of this call.
#[no_mangle]
pub unsafe extern "C" fn ksubstrate_home_apps_launch(id: *const c_char) -> c_int {
    if id.is_null() {
        return -1;
    }
    let Ok(id) = CStr::from_ptr(id).to_str() else {
        return -1;
    };
    match launch(id) {
        Ok(()) => 0,
        Err(error) => {
            log(&format!("home apps: {error}"));
            -1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_preserves_synthetic_id() {
        let app = HomeApp {
            package_id: "com.example.package".to_owned(),
            app_id: "app".to_owned(),
            synthetic_id: "kpm-app://com.example.package/app".to_owned(),
            name: "Example".to_owned(),
            subtitle: None,
            icon: "/tmp/icon.png".into(),
            executable: "/tmp/app".into(),
            arguments: Vec::new(),
            working_directory: "/tmp".into(),
        };
        assert_eq!(
            Entry::new(app).unwrap().id.to_str().unwrap(),
            "kpm-app://com.example.package/app"
        );
    }

    #[test]
    fn published_ids_survive_reload_and_launch_dispatches_exact_entry() {
        let first = Entry::new(HomeApp {
            package_id: "com.example.first".to_owned(),
            app_id: "main".to_owned(),
            synthetic_id: "kpm-app://com.example.first/main".to_owned(),
            name: "First".to_owned(),
            subtitle: None,
            icon: "/tmp/icon.png".into(),
            executable: "/usr/bin/true".into(),
            arguments: Vec::new(),
            working_directory: "/tmp".into(),
        })
        .unwrap();
        *entries().write().unwrap() = Box::leak(vec![first].into_boxed_slice());
        let first_id = ksubstrate_home_apps_id(0);

        let output = std::env::temp_dir().join(format!("ksub-home-launch-{}", std::process::id()));
        let _ = std::fs::remove_file(&output);
        let second = Entry::new(HomeApp {
            package_id: "com.example.second".to_owned(),
            app_id: "touch".to_owned(),
            synthetic_id: "kpm-app://com.example.second/touch".to_owned(),
            name: "Second".to_owned(),
            subtitle: None,
            icon: "/tmp/icon.png".into(),
            executable: "/usr/bin/touch".into(),
            arguments: vec![output.display().to_string()],
            working_directory: "/tmp".into(),
        })
        .unwrap();
        *entries().write().unwrap() = Box::leak(vec![second].into_boxed_slice());

        assert_eq!(
            unsafe { CStr::from_ptr(first_id) }.to_str().unwrap(),
            "kpm-app://com.example.first/main"
        );
        launch("kpm-app://com.example.second/touch").unwrap();
        for _ in 0..100 {
            if output.is_file() { break; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(output.is_file());
        let _ = std::fs::remove_file(output);
    }
}
