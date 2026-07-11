//! Kindle framework restart boundary.  Commands are checked so daemon readiness
//! is not reported until the restart request has actually succeeded.
use std::fs;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program).args(args).status().map_err(|e| format!("run {program}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{program} exited with {status}")) }
}

pub fn restart_hooked() -> Result<(), String> {
    run("initctl", &["restart", "framework"])?;
    run("lipc-set-prop", &["com.lab126.appmgrd", "start", "app://com.lab126.booklet.home"])
}

pub fn restart_stock() -> Result<(), String> { run("initctl", &["restart", "framework"]) }

pub fn wait_for_framework_health(timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if ["pillow", "appmgrd"].iter().any(|name| process_alive(name)) { return Ok(()); }
        thread::sleep(Duration::from_millis(250));
    }
    Err("framework did not expose a healthy root before timeout".to_owned())
}

fn process_alive(name: &str) -> bool {
    let Ok(entries) = fs::read_dir("/proc") else { return false; };
    entries.filter_map(Result::ok).any(|entry| {
        entry.file_name().to_string_lossy().bytes().all(|byte| byte.is_ascii_digit())
            && fs::read_to_string(entry.path().join("comm")).ok().is_some_and(|comm| comm.trim() == name)
    })
}
