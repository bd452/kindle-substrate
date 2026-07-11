//! Explicit-target diagnostic probe. It records the canonical target identity
//! and executable identity supplied by the wrapper/controller.

#[cfg(ksubstrate_dynamic)]
use std::os::raw::c_char;

#[cfg_attr(target_os = "linux", link_section = ".init_array")]
#[used]
static KSUBSTRATE_PROBE_INIT: extern "C" fn() = init;

extern "C" fn init() {
    kindle_compat::ensure_linked();
    report();
}

#[cfg(ksubstrate_dynamic)]
fn report() {
    let target = std::env::var("KSUBSTRATE_TARGET").unwrap_or_else(|_| "unknown".to_owned());
    let executable = std::fs::read_link("/proc/self/exe").map(|value| value.display().to_string()).unwrap_or_else(|_| "unknown".to_owned());
    let pid = std::process::id();
    log(&format!("probe: loaded target={target} exe={executable} pid={pid}"));
}

#[cfg(not(ksubstrate_dynamic))]
fn report() {
    // Host build has no engine linked; keep the logging path referenced.
    log("probe: inert host build");
}

fn log(message: &str) {
    #[cfg(ksubstrate_dynamic)]
    unsafe {
        let mut bytes = Vec::with_capacity(message.len() + 1);
        bytes.extend_from_slice(message.as_bytes());
        bytes.push(0);
        kh_log(bytes.as_ptr().cast());
    }

    #[cfg(not(ksubstrate_dynamic))]
    {
        let _ = message;
    }
}

#[cfg(ksubstrate_dynamic)]
extern "C" {
    fn kh_log(message: *const c_char);
}
