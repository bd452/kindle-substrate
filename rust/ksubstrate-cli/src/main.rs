use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

fn main() {
    kindle_compat::ensure_linked();
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("run") => run_preloaded(args.collect()),
        Some("wrapped-exec") => wrapped_exec(args.collect()),
        Some("paths") => {
            let paths = Paths::detect()?;
            println!("package={}", paths.package.display());
            println!("platform={}", paths.platform);
            println!("bootstrap={}", paths.bootstrap.display());
            println!("tweaks={}", paths.tweaks.display());
            Ok(())
        }
        Some("help") | Some("--help") | Some("-h") | None => {
            print_help();
            Ok(())
        }
        Some(command) => Err(format!("unknown command: {command}")),
    }
}

fn run_preloaded(command: Vec<String>) -> Result<(), String> {
    let (program, rest) = command
        .split_first()
        .ok_or_else(|| "usage: ksubstrate run <program> [args...]".to_owned())?;
    let paths = Paths::detect()?;
    exec_preloaded(Path::new(program), rest, &paths, None)
}

fn wrapped_exec(command: Vec<String>) -> Result<(), String> {
    let (invoked, rest) = command.split_first().ok_or_else(|| "usage: ksubstrate wrapped-exec <invoked-path> [args...]".to_owned())?;
    let invoked = Path::new(invoked);
    if !valid_system_path(invoked) {
        return Err("wrapped executable path is not an approved system executable".to_owned());
    }
    let original = Path::new("/var/local/kmc/ksubstrate-runtime/mounts/original").join(invoked.strip_prefix("/").map_err(|_| "invalid invoked path")?);
    verify_wrapped_mounts(invoked, &original)?;
    let paths = Paths::detect_runtime()?;
    exec_preloaded(&original, rest, &paths, Some(Path::new("/var/local/kmc/ksubstrate-runtime/state/log/tweaks.log")))
}

fn exec_preloaded(program: &Path, rest: &[String], paths: &Paths, log: Option<&Path>) -> Result<(), String> {

    let mut ld_preload = paths.bootstrap.to_string_lossy().into_owned();
    if let Ok(existing) = env::var("LD_PRELOAD") {
        if !existing.trim().is_empty() {
            ld_preload.push(' ');
            ld_preload.push_str(&existing);
        }
    }

    let lib_dir = paths
        .bootstrap
        .parent()
        .ok_or_else(|| "bootstrap path has no parent".to_owned())?;
    let mut ld_library_path = lib_dir.to_string_lossy().into_owned();
    if let Ok(existing) = env::var("LD_LIBRARY_PATH") {
        if !existing.trim().is_empty() {
            ld_library_path.push(':');
            ld_library_path.push_str(&existing);
        }
    }

    let mut command = Command::new(program);
    command.args(rest);
    command.env("LD_PRELOAD", ld_preload);
    command.env("LD_LIBRARY_PATH", ld_library_path);
    command.env("KSUBSTRATE_TWEAKS_DIR", &paths.tweaks);
    if let Some(log) = log { command.env("KSUBSTRATE_LOG", log); }

    #[cfg(unix)]
    {
        let error = command.exec();
        Err(format!("failed to exec {}: {error}", program.display()))
    }

    #[cfg(not(unix))]
    {
        let status = command.status().map_err(|error| format!("failed to run {}: {error}", program.display()))?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn valid_system_path(path: &Path) -> bool {
    path.is_absolute()
        && path.components().all(|component| !matches!(component, std::path::Component::ParentDir | std::path::Component::CurDir))
        && ["/usr/bin", "/usr/sbin", "/bin", "/sbin"].iter().any(|root| path.starts_with(root))
        && !matches!(path.file_name().and_then(|name| name.to_str()), Some("powerd" | "sshd" | "dbus-daemon" | "dbus" | "otav3" | "otaupd" | "mmcqd" | "wpa_supplicant" | "dhcpd"))
}

fn verify_wrapped_mounts(invoked: &Path, original: &Path) -> Result<(), String> {
    let entries = mount_info()?;
    let mounts_root = Path::new("/var/local/kmc/ksubstrate-runtime/mounts");
    let state_root = Path::new("/var/local/kmc/ksubstrate-runtime/state");
    verify_tmpfs(&entries, mounts_root, false)?;
    verify_tmpfs(&entries, state_root, true)?;
    let alias = entries.iter().find(|entry| entry.point == original).ok_or_else(|| format!("no original alias for {}", invoked.display()))?;
    if !alias.readonly() || alias.root != invoked { return Err("original alias is not the expected read-only bind mount".to_owned()); }
    let wrapper = entries.iter().find(|entry| entry.point == invoked).ok_or_else(|| format!("no wrapper mount for {}", invoked.display()))?;
    if !wrapper.readonly() || wrapper.root != Path::new("/var/local/kmc/ksubstrate-assets/wrapper.sh") { return Err("system path is not the expected read-only wrapper bind".to_owned()); }
    Ok(())
}

fn verify_tmpfs(entries: &[MountInfo], path: &Path, noexec: bool) -> Result<(), String> {
    let entry = entries.iter().find(|entry| entry.point == path).ok_or_else(|| format!("missing runtime tmpfs {}", path.display()))?;
    if entry.fs_type != "tmpfs" || !entry.has("nodev") || !entry.has("nosuid") || entry.has("noexec") != noexec { return Err(format!("runtime tmpfs {} has unsafe options", path.display())); }
    Ok(())
}

#[derive(Debug)] struct MountInfo { root: PathBuf, point: PathBuf, options: String, fs_type: String }
impl MountInfo { fn has(&self, option: &str) -> bool { self.options.split(',').any(|value| value == option) } fn readonly(&self) -> bool { self.has("ro") } }
fn mount_info() -> Result<Vec<MountInfo>, String> {
    std::fs::read_to_string("/proc/self/mountinfo").map_err(|e| format!("read mountinfo: {e}"))?.lines().map(|line| {
        let (left, right) = line.split_once(" - ").ok_or_else(|| "malformed mountinfo".to_owned())?; let left: Vec<_> = left.split_whitespace().collect(); let right: Vec<_> = right.split_whitespace().collect();
        Ok(MountInfo { root: PathBuf::from(left.get(3).ok_or_else(|| "missing mount root".to_owned())?), point: PathBuf::from(left.get(4).ok_or_else(|| "missing mount point".to_owned())?), options: left.get(5).ok_or_else(|| "missing mount options".to_owned())?.to_string(), fs_type: right.first().ok_or_else(|| "missing filesystem type".to_owned())?.to_string() })
    }).collect()
}

fn print_help() {
    println!("ksubstrate device helper");
    println!("usage:");
    println!("  ksubstrate run <program> [args...]");
    println!("  ksubstrate wrapped-exec <invoked-path> [args...]");
    println!("  ksubstrate paths");
}

struct Paths {
    package: PathBuf,
    platform: String,
    bootstrap: PathBuf,
    tweaks: PathBuf,
}

impl Paths {
    fn detect() -> Result<Self, String> {
        let package = env::var("KSUBSTRATE_PACKAGE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| package_from_exe().unwrap_or_else(|| PathBuf::from("/mnt/us/kmc/kpm/packages/com.bd452.ksubstrate")));
        let platform = env::var("KSUBSTRATE_PLATFORM").unwrap_or_else(|_| detect_platform());
        let bootstrap = package
            .join("lib")
            .join(&platform)
            .join("libksubstrate-bootstrap.so");
        if !bootstrap.is_file() {
            return Err(format!("bootstrap not found at {}", bootstrap.display()));
        }
        let tweaks = env::var("KSUBSTRATE_TWEAKS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/var/local/kmc/tweaks"));
        Ok(Self {
            package,
            platform,
            bootstrap,
            tweaks,
        })
    }

    fn detect_runtime() -> Result<Self, String> {
        let package = PathBuf::from("/mnt/us/kmc/kpm/packages/com.bd452.ksubstrate");
        let platform = detect_platform();
        let bootstrap = package.join("lib").join(&platform).join("libksubstrate-bootstrap.so");
        if !bootstrap.is_file() { return Err(format!("bootstrap not found at {}", bootstrap.display())); }
        Ok(Self { package, platform, bootstrap, tweaks: PathBuf::from("/var/local/kmc/tweaks") })
    }
}

fn package_from_exe() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let platform_dir = exe.parent()?;
    let bin_dir = platform_dir.parent()?;
    let package = bin_dir.parent()?;
    Some(package.to_path_buf())
}

fn detect_platform() -> String {
    if Path::new("/lib/ld-linux-armhf.so.3").exists() {
        "kindlehf".to_owned()
    } else {
        "kindlepw2".to_owned()
    }
}
