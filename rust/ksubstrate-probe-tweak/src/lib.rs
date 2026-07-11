//! Inheritance probe (A§6.4). A `*`-filter tweak that records every process it
//! is loaded into. After enabling a session with this installed, inspect the
//! tweak log to see which processes actually received `LD_PRELOAD` — including
//! booklets spawned by a preloaded `appmgrd` — to confirm inheritance or find a
//! process whose launcher strips the environment (which then needs a Tier-2
//! wrapper).

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
    let comm = std::fs::read_to_string("/proc/self/comm")
        .map(|value| value.trim().to_owned())
        .unwrap_or_else(|_| "unknown".to_owned());
    let pid = std::process::id();
    log(&format!("probe: loaded in comm={comm} pid={pid}"));
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
