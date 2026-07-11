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
    command.env("KSUBSTRATE_TWEAKS_DIR", paths.tweaks);

    #[cfg(unix)]
    {
        let error = command.exec();
        Err(format!("failed to exec {program}: {error}"))
    }

    #[cfg(not(unix))]
    {
        let status = command.status().map_err(|error| format!("failed to run {program}: {error}"))?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn print_help() {
    println!("ksubstrate device helper");
    println!("usage:");
    println!("  ksubstrate run <program> [args...]");
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
            .unwrap_or_else(|_| package.join("tweaks"));
        Ok(Self {
            package,
            platform,
            bootstrap,
            tweaks,
        })
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
