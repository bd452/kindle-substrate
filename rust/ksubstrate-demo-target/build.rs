fn main() {
    println!("cargo:rustc-check-cfg=cfg(ksubstrate_dynamic)");
    if let Ok(lib_dir) = std::env::var("KSUBSTRATE_LIB_DIR") {
        println!("cargo:rustc-link-search=native={lib_dir}");
        println!("cargo:rustc-link-lib=dylib=ksubstrate");
        println!("cargo:rustc-cfg=ksubstrate_dynamic");
        // Export the binary's dynamic symbols so a preloaded tweak can resolve
        // `compute` and hook it.
        println!("cargo:rustc-link-arg=-Wl,--export-dynamic");
    }
}
