//! Link flags that must apply to the final `libksubstrate.so` cdylib.
//!
//! `dobby-sys` declares `links = "dobby"` and `rustc-link-lib=static=dobby`.
//! Cargo **bundles** that static archive into `libdobby_sys.rlib`, so the
//! final cdylib link already contains every Dobby object — do **not** pass
//! `libdobby.a` again (that produces duplicate-symbol errors under lld).
//!
//! What this script still has to do: fold the C++ / EH runtime into the .so.
//! rustc drives the C linker with `-nodefaultlibs`, so libstdc++ / libgcc_eh
//! are not pulled automatically. Absolute archive paths as late positional
//! `link-arg-cdylib`s land after the bundled Dobby objects, which is when
//! those C++ undefineds are visible to the archive scanner.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if target_arch != "arm" {
        return;
    }

    // Presence check: dobby-sys must have run (and bundled libdobby.a).
    let _ = env::var("DEP_DOBBY_ARCHIVE").unwrap_or_else(|_| {
        panic!(
            "DEP_DOBBY_ARCHIVE unset — dobby-sys must run first and emit cargo:archive=..."
        )
    });

    let cross_tc = env::var("CROSS_TC").expect("CROSS_TC must be set for the ARM build");
    let libstdcxx = print_file_name(&format!("{cross_tc}-g++"), "libstdc++.a");
    let libgcc_eh = print_file_name(&format!("{cross_tc}-gcc"), "libgcc_eh.a");
    let libgcc = print_file_name(&format!("{cross_tc}-gcc"), "libgcc.a");

    // After bundled Dobby objects. Kindle rootfs may not ship matching
    // libstdc++.so / libgcc_s.so, so fold them into the .so.
    println!("cargo:rustc-link-arg-cdylib={}", libstdcxx.display());
    println!("cargo:rustc-link-arg-cdylib={}", libgcc_eh.display());
    println!("cargo:rustc-link-arg-cdylib={}", libgcc.display());
    println!("cargo:rerun-if-env-changed=CROSS_TC");
    println!("cargo:rerun-if-env-changed=DEP_DOBBY_ARCHIVE");
}

fn print_file_name(driver: &str, name: &str) -> PathBuf {
    let output = Command::new(driver)
        .arg(format!("-print-file-name={name}"))
        .output()
        .unwrap_or_else(|error| panic!("failed to spawn `{driver}`: {error}"));
    assert!(
        output.status.success(),
        "`{driver} -print-file-name={name}` failed"
    );
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_owned());
    assert!(
        path.is_file(),
        "{name} not found at {} (from `{driver} -print-file-name`)",
        path.display()
    );
    path
}
