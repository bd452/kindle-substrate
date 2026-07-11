//! Cross-builds the vendored Dobby engine as a static library and links it.
//!
//! Dobby source lives in the ksubstrate KPM package's submodule
//! (`apps/com.bd452.ksubstrate/vendor/Dobby`). We drive its CMake `dobby_static`
//! target with the koxtoolchain (the same `CROSS_TC` cross build the app
//! `build.sh` exports), then static-link `libdobby.a` plus the C++ runtime.
//!
//! Only the on-device ARM build compiles this; a host build uses the mock
//! backend in `ksubstrate` and never depends on this crate.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if target_arch != "arm" {
        println!("cargo:warning=dobby-sys: non-arm target ({target_arch}); skipping Dobby build");
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let repo_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("dobby-sys must live at <repo>/rust/dobby-sys")
        .to_path_buf();
    let vendor_dobby_dir = repo_root
        .join("apps")
        .join("com.bd452.ksubstrate")
        .join("vendor")
        .join("Dobby");
    let header = vendor_dobby_dir.join("include").join("dobby.h");
    assert!(
        header.exists(),
        "Dobby source missing at {} — run: git submodule update --init --recursive {}",
        vendor_dobby_dir.display(),
        vendor_dobby_dir.display()
    );

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    // Patch a private build copy so compiling never dirties the pinned submodule.
    let dobby_dir = out.join("dobby-src");
    let _ = std::fs::remove_dir_all(&dobby_dir);
    copy_dir_all(&vendor_dobby_dir, &dobby_dir)
        .expect("failed to copy Dobby source into the Cargo build directory");
    patch_logging_odr(&dobby_dir);

    let build_dir = out.join("dobby-build");
    let _ = std::fs::remove_dir_all(&build_dir);
    let cross_tc = env::var("CROSS_TC").expect("CROSS_TC must be set for the Dobby cross build");

    // Configure. CMAKE_SYSTEM_* forces Dobby's ARM source selection; PIC lets the
    // archive link into libksubstrate.so (a cdylib).
    let status = Command::new("cmake")
        .arg("-S")
        .arg(&dobby_dir)
        .arg("-B")
        .arg(&build_dir)
        .arg("-DCMAKE_BUILD_TYPE=Release")
        .arg("-DDOBBY_DEBUG=OFF")
        .arg("-DDOBBY_BUILD_EXAMPLE=OFF")
        .arg("-DDOBBY_BUILD_TEST=OFF")
        // Symbol resolver backs kh_find_symbol for non-exported firmware
        // internals. ImportTableReplace stays OFF: that plugin is Darwin/Mach-O
        // only; Linux PLT/GOT lives in ksubstrate::plt.
        .arg("-DPlugin.SymbolResolver=ON")
        .arg("-DPlugin.ImportTableReplace=OFF")
        .arg("-DCMAKE_POSITION_INDEPENDENT_CODE=ON")
        .arg("-DCMAKE_SYSTEM_NAME=Linux")
        .arg("-DCMAKE_SYSTEM_PROCESSOR=arm")
        .arg(format!("-DCMAKE_C_COMPILER={cross_tc}-gcc"))
        .arg(format!("-DCMAKE_CXX_COMPILER={cross_tc}-g++"))
        .arg(format!("-DCMAKE_ASM_COMPILER={cross_tc}-gcc"))
        .status()
        .expect("failed to spawn `cmake` (is cmake installed in the build image?)");
    assert!(status.success(), "cmake configure for Dobby failed");

    let jobs = env::var("NUM_JOBS").unwrap_or_else(|_| "2".to_owned());
    let status = Command::new("cmake")
        .arg("--build")
        .arg(&build_dir)
        .arg("--target")
        .arg("dobby_static")
        .arg("-j")
        .arg(&jobs)
        .status()
        .expect("failed to spawn `cmake --build`");
    assert!(status.success(), "cmake build for Dobby failed");

    let archive = find_archive(&build_dir).unwrap_or_else(|| {
        panic!("libdobby.a not found under {}", build_dir.display());
    });
    println!(
        "cargo:rustc-link-search=native={}",
        archive.parent().unwrap().display()
    );
    println!("cargo:rustc-link-lib=static=dobby");
    // Absolute path for dependents that produce a cdylib: Cargo sometimes drops
    // propagated `-ldobby` on the final shared-object link line (search path
    // survives; the `-l` does not). ksubstrate reads DEP_DOBBY_ARCHIVE and
    // passes it as a late positional link-arg so the archive is actually linked.
    println!("cargo:archive={}", archive.display());

    println!("cargo:rerun-if-changed={}", vendor_dobby_dir.display());
    println!("cargo:rerun-if-env-changed=CROSS_TC");
}

/// Upstream ODR fix. `external/logging/logging/logging.h` defines the static
/// member `Logger::Shared()` out-of-class in the header **without** `inline`
/// (the author marked the adjacent `gLogger` variable inline but missed the
/// function). Every translation unit that includes the header then emits its own
/// definition, and strict-ODR GCC refuses to link the resulting duplicates into
/// `libdobby.a`. Add the missing `inline`. Idempotent, and re-applied on a fresh
/// build because it runs against a private copy of the pinned submodule.
fn patch_logging_odr(dobby_dir: &Path) {
    let header = dobby_dir
        .join("external")
        .join("logging")
        .join("logging")
        .join("logging.h");
    let Ok(contents) = std::fs::read_to_string(&header) else {
        return;
    };
    if contents.contains("inline Logger *Logger::Shared()") {
        return; // already patched
    }
    let patched = contents.replace(
        "\nLogger *Logger::Shared() {",
        "\ninline Logger *Logger::Shared() {",
    );
    if patched != contents {
        let _ = std::fs::write(&header, patched);
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        if entry.file_name() == ".git" {
            continue;
        }
        let source = entry.path();
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&source, &target)?;
        } else {
            std::fs::copy(source, target)?;
        }
    }
    Ok(())
}

fn find_archive(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_archive(&path) {
                return Some(found);
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some("libdobby.a") {
            return Some(path);
        }
    }
    None
}
