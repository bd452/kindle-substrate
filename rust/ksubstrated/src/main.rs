mod control;
mod framework;
mod journal;
mod layout;
mod mounts;
mod session;
mod tweaks;

use layout::StateTmpfs;
use std::env;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

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
        Some("--monitor") => session::Session::start()?.serve(),
        Some("--disable") => disable(),
        Some("--reframe") => control::request(&StateTmpfs::new(), "reframe").map(|_| ()),
        Some("--reframe-if-active") => reframe_if_active(),
        Some("--reframe-if-active-deferred") => control::request(&StateTmpfs::new(), "reframe-if-active-deferred").map(|_| ()),
        Some("--status") => status(),
        Some("--toggle") | None => match control::request(&StateTmpfs::new(), "status") {
            Ok(_) => disable(),
            Err(_) => enable(),
        },
        Some("--help") | Some("-h") => { print_help(); Ok(()) }
        Some(option) => Err(format!("unknown ksubstrated option: {option}")),
    }
}

fn enable() -> Result<(), String> {
    if control::request(&StateTmpfs::new(), "status").is_ok() {
        println!("Kindle Substrate session is already enabled.");
        return Ok(());
    }
    session::Session::recover_crashed_session()?;
    let executable = env::current_exe().map_err(|e| format!("resolve daemon executable: {e}"))?;
    Command::new(executable)
        .arg("--monitor")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("start daemon: {e}"))?;
    for _ in 0..300 {
        if control::request(&StateTmpfs::new(), "status").is_ok() {
            println!("Kindle Substrate session enabled.");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("daemon did not become ready; inspect logs or reboot before retrying".to_owned())
}

fn disable() -> Result<(), String> {
    match control::request(&StateTmpfs::new(), "disable") {
        Ok(_) => { println!("Kindle Substrate session disabled."); Ok(()) }
        Err(_) => {
            let recovered = session::Session::recover_crashed_session()?;
            if recovered { println!("Recovered and disabled stale Kindle Substrate session."); } else { println!("Kindle Substrate session is already disabled."); }
            Ok(())
        }
    }
}

fn reframe_if_active() -> Result<(), String> {
    match control::request(&StateTmpfs::new(), "reframe-if-active") {
        Ok(_) => Ok(()),
        Err(_) => {
            // A package hook must never enable a disabled session.  Recovering
            // an already-crashed session is conservative cleanup, not enable.
            let _ = session::Session::recover_crashed_session()?;
            Ok(())
        }
    }
}

fn status() -> Result<(), String> {
    match control::request(&StateTmpfs::new(), "status") {
        Ok(status) => println!("{status}"),
        Err(_) => {
            let mounts = layout::MountTmpfs::new();
            let state = layout::StateTmpfs::new();
            if mounts::is_mountpoint(mounts.path())? || mounts::is_mountpoint(state.path())? { println!("recovery-required"); } else { println!("disabled"); }
        }
    }
    Ok(())
}

fn print_help() {
    println!("ksubstrated --enable|--disable|--status|--reframe|--reframe-if-active|--reframe-if-active-deferred|--toggle");
}
