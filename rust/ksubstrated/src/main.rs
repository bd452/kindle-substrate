use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const STATE_DIR: &str = "/var/local/kmc/ksubstrate";
const RUN_DIR: &str = "/var/local/kmc/ksubstrate/run";
const LOG_DIR: &str = "/var/local/kmc/ksubstrate/log";
const PID_FILE: &str = "/var/local/kmc/ksubstrate/run/ksubstrated.pid";
const DISABLE_FILE: &str = "/var/local/kmc/ksubstrate/run/disable";
const WRAPPED_FILE: &str = "/var/local/kmc/ksubstrate/run/wrappers.list";
const STARTS_FILE: &str = "/var/local/kmc/ksubstrate/run/starts.log";
const LOG_FILE: &str = "/var/local/kmc/ksubstrate/log/ksubstrated.log";
const TWEAK_LOG_FILE: &str = "/var/local/kmc/ksubstrate/log/tweaks.log";
const DEFAULT_PACKAGE: &str = "/mnt/us/kmc/kpm/packages/com.bd452.ksubstrate";
const TWEAKS_DIR: &str = "/var/local/kmc/tweaks";
// Volatile session state. Wrappers and the bind-mount handles to the originals
// live here; both are cleared by a reboot (mounts don't persist), which is what
// makes "hard reboot = clean boot" (A§14.1) hold with no on-disk rootfs edits.
const WRAPPERS_DIR: &str = "/var/local/kmc/ksubstrate/run/wrappers";
const ORIG_DIR: &str = "/var/local/kmc/ksubstrate/run/orig";

/// Crash-loop guard: if the session restarts this many times inside the window,
/// return to stock instead of re-arming hooks.
const CRASH_WINDOW_SECS: u64 = 120;
const CRASH_THRESHOLD: usize = 3;

fn main() {
    kindle_compat::ensure_linked();
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    match env::args().nth(1).as_deref() {
        Some("--enable") => enable(),
        Some("--disable") => disable(),
        Some("--status") => status(),
        Some("--toggle") | None => toggle(),
        Some("--monitor") => monitor(),
        Some("--help") | Some("-h") => {
            print_help();
            Ok(())
        }
        Some(other) => Err(format!("unknown ksubstrated option: {other}")),
    }
}

fn ensure_dirs() -> Result<(), String> {
    for dir in [STATE_DIR, RUN_DIR, LOG_DIR] {
        fs::create_dir_all(dir).map_err(|error| format!("failed to create {dir}: {error}"))?;
    }
    Ok(())
}

fn enable() -> Result<(), String> {
    ensure_dirs()?;
    if is_running() {
        println!("Kindle Substrate session is already enabled.");
        return Ok(());
    }

    let exe = env::current_exe().map_err(|error| format!("failed to resolve current executable: {error}"))?;
    let child = Command::new(exe)
        .arg("--monitor")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("failed to start ksubstrated monitor: {error}"))?;

    fs::write(PID_FILE, child.id().to_string()).map_err(|error| format!("failed to write {PID_FILE}: {error}"))?;
    println!("Kindle Substrate session enabled.");
    Ok(())
}

fn disable() -> Result<(), String> {
    ensure_dirs()?;
    fs::write(DISABLE_FILE, "disable\n").map_err(|error| format!("failed to write {DISABLE_FILE}: {error}"))?;

    if let Some(pid) = read_pid() {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }

    cleanup_session();
    // A deliberate disable clears the crash-loop history so the next manual
    // enable starts from a clean slate.
    let _ = fs::remove_file(STARTS_FILE);
    restart_framework_stock();
    println!("Kindle Substrate session disabled.");
    Ok(())
}

fn toggle() -> Result<(), String> {
    if is_running() {
        disable()
    } else {
        enable()
    }
}

fn status() -> Result<(), String> {
    if is_running() {
        println!("enabled");
    } else {
        println!("clean");
    }
    Ok(())
}

fn monitor() -> Result<(), String> {
    ensure_dirs()?;
    let pid = std::process::id();
    fs::write(PID_FILE, pid.to_string()).map_err(|error| format!("failed to write {PID_FILE}: {error}"))?;
    let _ = fs::remove_file(DISABLE_FILE);
    log("monitor started");

    // Crash-loop guard. Every monitor start is recorded; if the session has come
    // up too many times inside the window, the framework is almost certainly
    // crash-looping under the hooks, so return to stock instead of re-arming.
    let starts = record_start(now_secs());
    if starts >= CRASH_THRESHOLD {
        log(&format!(
            "crash-loop guard tripped ({starts} starts within {CRASH_WINDOW_SECS}s); returning to stock"
        ));
        cleanup_session();
        restart_framework_stock();
        return Ok(());
    }

    let session = Session::detect();
    session.write_environment_summary();

    let wrapping_active = if wrap_enabled() {
        match install_wrappers(&session) {
            Ok(wrapped) => {
                log(&format!("wrapped {} spawn root(s)", wrapped.len()));
                restart_framework_hooked();
                true
            }
            Err(error) => {
                log(&format!("wrapper install failed: {error}; restoring stock"));
                cleanup_session();
                restart_framework_stock();
                return Err(error);
            }
        }
    } else {
        log("system wrappers not installed; set KSUBSTRATE_SYSTEM_WRAP=1 to manage Kindle UI roots");
        false
    };

    // Health guard: while wrapped, watch the UI roots. If they die repeatedly in
    // a short window the hooked framework is crash-looping, so return to stock
    // automatically (A§7 Level 3). Grace period lets the framework restart come
    // up before we start counting.
    let mut guard = HealthGuard::new();
    let start = now_secs();
    loop {
        if Path::new(DISABLE_FILE).exists() {
            log("disable marker observed");
            break;
        }
        if wrapping_active && now_secs().saturating_sub(start) >= HEALTH_GRACE_SECS {
            let alive = ui_roots_alive();
            if guard.observe(alive, now_secs(), CRASH_WINDOW_SECS, CRASH_THRESHOLD) {
                log(&format!(
                    "health guard tripped ({CRASH_THRESHOLD} UI deaths within {CRASH_WINDOW_SECS}s); returning to stock"
                ));
                break;
            }
        }
        thread::sleep(Duration::from_secs(2));
    }

    cleanup_session();
    restart_framework_stock();
    log("monitor exited");
    Ok(())
}

const HEALTH_GRACE_SECS: u64 = 20;

/// Tracks falling edges (alive -> dead) of the UI roots within a sliding window.
#[derive(Default)]
struct HealthGuard {
    was_alive: bool,
    initialized: bool,
    deaths: Vec<u64>,
}

impl HealthGuard {
    fn new() -> Self {
        Self::default()
    }

    /// Record one health sample; returns true once the death count within the
    /// window reaches the threshold.
    fn observe(&mut self, alive: bool, now: u64, window: u64, threshold: usize) -> bool {
        if self.initialized && self.was_alive && !alive {
            self.deaths.push(now);
        }
        self.was_alive = alive;
        self.initialized = true;
        self.deaths = prune_history(&self.deaths, now, window);
        self.deaths.len() >= threshold
    }
}

/// True if any of the built-in UI root processes is currently running.
fn ui_roots_alive() -> bool {
    ["pillow", "appmgrd"].iter().any(|comm| process_alive(comm))
}

/// Scan `/proc/<pid>/comm` for a process whose comm matches (comm is truncated
/// to 15 bytes by the kernel, so compare on that basis).
fn process_alive(comm: &str) -> bool {
    let Ok(entries) = fs::read_dir("/proc") else {
        return false;
    };
    let want = &comm.as_bytes()[..comm.len().min(15)];
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if let Ok(found) = fs::read_to_string(entry.path().join("comm")) {
            if found.trim().as_bytes() == want {
                return true;
            }
        }
    }
    false
}

fn wrap_enabled() -> bool {
    env::var("KSUBSTRATE_SYSTEM_WRAP").as_deref() == Ok("1")
}

/// Install session wrappers for every resolved spawn root, recording exactly
/// which roots were wrapped so cleanup restores the same set. Any failure rolls
/// back the roots wrapped so far so a UI root is never left missing.
fn install_wrappers(session: &Session) -> Result<Vec<String>, String> {
    let mut wrapped: Vec<String> = Vec::new();
    for root in &session.spawn_roots {
        match wrap_root(session, root) {
            Ok(true) => wrapped.push(root.clone()),
            Ok(false) => log(&format!("spawn root unavailable, skipping: {root}")),
            Err(error) => {
                log(&format!("failed to wrap {root}: {error}; rolling back"));
                restore_roots(&wrapped);
                let _ = fs::remove_file(WRAPPED_FILE);
                return Err(error);
            }
        }
    }
    write_wrapped(&wrapped)?;
    Ok(wrapped)
}

/// Wrap a spawn root with a volatile bind mount: expose the real binary at a
/// stable path (bind mount), then shadow the original path with a wrapper that
/// re-execs it under LD_PRELOAD. Nothing on the rootfs is modified, and a reboot
/// drops both mounts, so the session is inherently clean-boot (A§14.1). Returns
/// Ok(true) when wrapped, Ok(false) when skipped, Err (rolled back) on failure.
fn wrap_root(session: &Session, root: &str) -> Result<bool, String> {
    let path = Path::new(root);
    if !path.exists() {
        return Ok(false);
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return Ok(false);
    };
    fs::create_dir_all(ORIG_DIR).map_err(|error| format!("create {ORIG_DIR}: {error}"))?;
    fs::create_dir_all(WRAPPERS_DIR).map_err(|error| format!("create {WRAPPERS_DIR}: {error}"))?;
    let orig_mount = Path::new(ORIG_DIR).join(name);
    let wrapper = Path::new(WRAPPERS_DIR).join(name);
    if wrapper.exists() {
        // Already wrapped this session.
        return Ok(false);
    }

    // Bind the real binary to a stable path the wrapper can exec after shadowing.
    fs::write(&orig_mount, b"").map_err(|error| format!("create bind target {}: {error}", orig_mount.display()))?;
    bind_mount(path, &orig_mount)?;

    let script = wrapper_script(session, &orig_mount);
    if let Err(error) = fs::write(&wrapper, &script) {
        let _ = umount(&orig_mount);
        let _ = fs::remove_file(&orig_mount);
        return Err(format!("write wrapper {}: {error}", wrapper.display()));
    }
    let _ = Command::new("chmod").arg("+x").arg(&wrapper).status();

    // Shadow the original path with the wrapper.
    if let Err(error) = bind_mount(&wrapper, path) {
        let _ = umount(&orig_mount);
        let _ = fs::remove_file(&orig_mount);
        let _ = fs::remove_file(&wrapper);
        return Err(error);
    }
    log(&format!("wrapped {root} (bind mount)"));
    Ok(true)
}

fn wrapper_script(session: &Session, exec_target: &Path) -> String {
    format!(
        "#!/bin/sh\nexport LD_PRELOAD=\"{}${{LD_PRELOAD:+ $LD_PRELOAD}}\"\nexport LD_LIBRARY_PATH=\"{}${{LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}}\"\nexport KSUBSTRATE_TWEAKS_DIR=\"{}\"\nexport KSUBSTRATE_LOG=\"{}\"\nexec \"{}\" \"$@\"\n",
        session.bootstrap.display(),
        session.lib_dir.display(),
        session.tweaks.display(),
        TWEAK_LOG_FILE,
        exec_target.display()
    )
}

fn bind_mount(src: &Path, dst: &Path) -> Result<(), String> {
    let status = Command::new("mount")
        .arg("--bind")
        .arg(src)
        .arg(dst)
        .status()
        .map_err(|error| format!("spawn mount: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("mount --bind {} {} failed", src.display(), dst.display()))
    }
}

fn umount(path: &Path) -> Result<(), String> {
    let status = Command::new("umount")
        .arg(path)
        .status()
        .map_err(|error| format!("spawn umount: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("umount {} failed", path.display()))
    }
}

fn restore_roots(roots: &[String]) {
    for root in roots {
        let path = Path::new(root);
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let orig_mount = Path::new(ORIG_DIR).join(name);
        let wrapper = Path::new(WRAPPERS_DIR).join(name);
        // Unshadow the original path, then release the bind to the real binary.
        let _ = umount(path);
        let _ = umount(&orig_mount);
        let _ = fs::remove_file(&wrapper);
        let _ = fs::remove_file(&orig_mount);
        log(&format!("restored {root}"));
    }
}

fn cleanup_session() {
    // Restore exactly what we wrapped. If the manifest is missing (crash before
    // it was written), a reboot still clears the volatile mounts, so there is no
    // stranded-root hazard as there was with the rename mechanism.
    restore_roots(&read_wrapped());
    let _ = fs::remove_file(WRAPPED_FILE);
    let _ = fs::remove_file(PID_FILE);
    let _ = fs::remove_file(DISABLE_FILE);
}

fn write_wrapped(roots: &[String]) -> Result<(), String> {
    fs::write(WRAPPED_FILE, roots.join("\n")).map_err(|error| format!("failed to write {WRAPPED_FILE}: {error}"))
}

fn read_wrapped() -> Vec<String> {
    fs::read_to_string(WRAPPED_FILE)
        .map(|contents| {
            contents
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Append the current start time, prune entries outside the crash window, and
/// return how many starts remain inside it.
fn record_start(now: u64) -> usize {
    let mut history = read_history();
    history.push(now);
    history = prune_history(&history, now, CRASH_WINDOW_SECS);
    let _ = fs::write(
        STARTS_FILE,
        history.iter().map(u64::to_string).collect::<Vec<_>>().join("\n"),
    );
    history.len()
}

fn read_history() -> Vec<u64> {
    fs::read_to_string(STARTS_FILE)
        .map(|contents| contents.lines().filter_map(|line| line.trim().parse().ok()).collect())
        .unwrap_or_default()
}

fn prune_history(history: &[u64], now: u64, window: u64) -> Vec<u64> {
    history
        .iter()
        .copied()
        .filter(|&stamp| now.saturating_sub(stamp) <= window)
        .collect()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_secs())
        .unwrap_or(0)
}

fn restart_framework_hooked() {
    log("restarting framework for hooked session");
    let _ = Command::new("initctl").arg("restart").arg("framework").status();
    let _ = Command::new("lipc-set-prop")
        .args(["com.lab126.appmgrd", "start", "app://com.lab126.booklet.home"])
        .status();
}

fn restart_framework_stock() {
    log("restarting framework stock");
    let _ = Command::new("initctl").arg("restart").arg("framework").status();
}

fn read_pid() -> Option<i32> {
    fs::read_to_string(PID_FILE).ok()?.trim().parse().ok()
}

fn is_running() -> bool {
    let Some(pid) = read_pid() else {
        return false;
    };
    unsafe { libc::kill(pid, 0) == 0 }
}

fn log(message: &str) {
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG_FILE)
        .and_then(|mut file| writeln!(file, "{message}"));
}

fn print_help() {
    println!("ksubstrated --enable|--disable|--status|--toggle");
    println!("set KSUBSTRATE_SYSTEM_WRAP=1 before --enable to install session wrappers");
}

struct Session {
    package: PathBuf,
    platform: String,
    lib_dir: PathBuf,
    bootstrap: PathBuf,
    tweaks: PathBuf,
    spawn_roots: Vec<String>,
}

impl Session {
    fn detect() -> Self {
        let package = env::var("KSUBSTRATE_PACKAGE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| DEFAULT_PACKAGE.into());
        let platform = if Path::new("/lib/ld-linux-armhf.so.3").exists() {
            "kindlehf"
        } else {
            "kindlepw2"
        }
        .to_owned();
        let lib_dir = package.join("lib").join(&platform);
        let bootstrap = lib_dir.join("libksubstrate-bootstrap.so");
        // Installed tweaks are aggregated at a single well-known location (A§8.1),
        // overridable for self-contained demos via KSUBSTRATE_TWEAKS_DIR.
        let tweaks = env::var("KSUBSTRATE_TWEAKS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(TWEAKS_DIR));
        let spawn_roots = compute_spawn_roots(&tweaks);
        Self {
            package,
            platform,
            lib_dir,
            bootstrap,
            tweaks,
            spawn_roots,
        }
    }

    fn write_environment_summary(&self) {
        let summary = format!(
            "package={}\nplatform={}\nbootstrap={}\ntweaks={}\nspawn_roots={}\n",
            self.package.display(),
            self.platform,
            self.bootstrap.display(),
            self.tweaks.display(),
            self.spawn_roots.join(",")
        );
        let _ = fs::write(Path::new(RUN_DIR).join("session.env"), summary);
    }
}

fn builtin_spawn_roots() -> Vec<&'static str> {
    vec!["/usr/bin/appmgrd", "/usr/bin/pillow", "/usr/sbin/pillow"]
}

/// Processes that must never be wrapped, regardless of what a tweak filter asks
/// for. Wrapping any of these risks a soft-brick or breaks the recovery path
/// (USB/SSH). See the architecture blacklist (A§6.3, A§10).
fn blacklisted_comm(name: &str) -> bool {
    const BLACKLIST: [&str; 9] = [
        "powerd", "sshd", "dbus-daemon", "dbus", "otav3", "otaupd", "mmcqd", "wpa_supplicant",
        "dhcpd",
    ];
    BLACKLIST.contains(&name)
}

/// Built-in Kindle UI roots plus any firmware-resolved roots named by installed
/// tweak filters. Filters list process comm names; each is resolved against the
/// common system bin directories. Blacklisted names are dropped so a stray
/// filter can never wrap a recovery-critical process.
fn compute_spawn_roots(tweaks_dir: &Path) -> Vec<String> {
    let mut roots: Vec<String> = builtin_spawn_roots().into_iter().map(str::to_owned).collect();
    for name in filter_root_names(tweaks_dir) {
        if blacklisted_comm(&name) {
            log(&format!("refusing blacklisted spawn root from filter: {name}"));
            continue;
        }
        for candidate in resolve_root_paths(&name) {
            if !roots.contains(&candidate) {
                roots.push(candidate);
            }
        }
    }
    roots
}

fn filter_root_names(tweaks_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let Ok(entries) = fs::read_dir(tweaks_dir) else {
        return names;
    };
    for entry in entries.filter_map(Result::ok) {
        let filter = entry.path().join("tweak.ksfilter");
        let Ok(contents) = fs::read_to_string(&filter) else {
            continue;
        };
        for line in contents.lines() {
            let token = line.split('#').next().unwrap_or("").trim();
            if token.is_empty() || token == "*" {
                continue;
            }
            let token = token.to_owned();
            if !names.contains(&token) {
                names.push(token);
            }
        }
    }
    names
}

fn resolve_root_paths(name: &str) -> Vec<String> {
    const BIN_DIRS: [&str; 4] = ["/usr/bin", "/usr/sbin", "/bin", "/sbin"];
    BIN_DIRS
        .iter()
        .map(|dir| format!("{dir}/{name}"))
        .filter(|path| Path::new(path).exists())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_history_drops_entries_outside_window() {
        let history = vec![10u64, 50, 100, 200];
        assert_eq!(prune_history(&history, 200, 120), vec![100, 200]);
    }

    #[test]
    fn prune_history_keeps_recent_entries() {
        let history = vec![100u64, 150, 200];
        assert_eq!(prune_history(&history, 200, 120), vec![100, 150, 200]);
    }

    #[test]
    fn blacklist_blocks_recovery_critical_processes() {
        assert!(blacklisted_comm("powerd"));
        assert!(blacklisted_comm("sshd"));
        assert!(blacklisted_comm("dbus-daemon"));
        assert!(!blacklisted_comm("pillow"));
        assert!(!blacklisted_comm("appmgrd"));
    }

    #[test]
    fn health_guard_trips_on_repeated_deaths() {
        let mut guard = HealthGuard::new();
        // alive baseline, then three death transitions within the window.
        assert!(!guard.observe(true, 100, 120, 3));
        assert!(!guard.observe(false, 101, 120, 3)); // death 1
        assert!(!guard.observe(true, 102, 120, 3));
        assert!(!guard.observe(false, 103, 120, 3)); // death 2
        assert!(!guard.observe(true, 104, 120, 3));
        assert!(guard.observe(false, 105, 120, 3)); // death 3 -> trip
    }

    #[test]
    fn health_guard_ignores_stable_uptime() {
        let mut guard = HealthGuard::new();
        for t in 0..10 {
            assert!(!guard.observe(true, t, 120, 3));
        }
    }

    #[test]
    fn filter_root_names_collects_tokens_excluding_wildcards() {
        let dir = std::env::temp_dir().join(format!("ksub-roots-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let tweak = dir.join("com.example.tweak");
        fs::create_dir_all(&tweak).unwrap();
        fs::write(tweak.join("tweak.ksfilter"), "# comment\ncooltool\n*\ncooltool\n").unwrap();
        let names = filter_root_names(&dir);
        assert_eq!(names, vec!["cooltool".to_owned()]);
        let _ = fs::remove_dir_all(&dir);
    }
}
