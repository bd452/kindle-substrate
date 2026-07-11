mod framework;
mod journal;
mod layout;
mod mounts;
mod session;
mod tweaks;

use std::env;
use std::fs::File;
use std::os::fd::AsRawFd;

fn main() { kindle_compat::ensure_linked(); if let Err(error) = run() { eprintln!("{error}"); std::process::exit(1); } }

fn run() -> Result<(), String> {
    let _lock = operation_lock()?;
    match env::args().nth(1).as_deref() {
        Some("enable") | Some("--enable") => session::enable(),
        Some("disable") | Some("--disable") => session::disable(),
        Some("status") | Some("--status") => { println!("{}", session::status()?); Ok(()) },
        Some("reframe-if-active") | Some("--reframe-if-active") => { let changed=session::reframe_if_active()?; println!("{}", if changed{"reframed"}else{"disabled"}); Ok(()) },
        Some("reframe") | Some("--reframe") => { if session::reframe_if_active()? { Ok(()) } else { Err("session is disabled".to_owned()) } },
        Some("post-package-change") => { let changed=session::post_package_change()?; println!("{}", if changed{"reframed"}else{"disabled"}); Ok(()) },
        Some("prepare-target-package-change") => { let package=env::args().nth(2).ok_or_else(||"usage: ksubstrated prepare-target-package-change <package>".to_owned())?; let changed=session::prepare_target_package_change(&package)?; println!("{}",if changed{"prepared"}else{"disabled-or-unaffected"}); Ok(()) },
        Some("finish-target-package-change") => { let package=env::args().nth(2).ok_or_else(||"usage: ksubstrated finish-target-package-change <package>".to_owned())?; let changed=session::finish_target_package_change(&package)?; println!("{}",if changed{"reframed"}else{"disabled"}); Ok(()) },
        Some("toggle") | Some("--toggle") | None => if session::status()?=="active" { session::disable() } else { session::enable() },
        Some("help") | Some("--help") | Some("-h") => { println!("ksubstrated enable|disable|status|reframe-if-active|post-package-change"); Ok(()) },
        Some(command) => Err(format!("unknown ksubstrated command: {command}")),
    }
}

fn operation_lock() -> Result<File, String> {
    let root = std::path::Path::new(layout::RUNTIME_ROOT);
    let file = File::open(root).map_err(|e| format!("open runtime root for operation lock: {e}"))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 { return Err(format!("acquire operation lock: {}", std::io::Error::last_os_error())); }
    Ok(file)
}
