fn main() {
    println!("cargo:rustc-check-cfg=cfg(ksubstrate_dynamic)");
    if std::env::var_os("KSUBSTRATE_LIB_DIR").is_some() {
        println!("cargo:rustc-cfg=ksubstrate_dynamic");
        // Export the binary's dynamic symbols so a preloaded tweak can resolve
        // `compute` and hook it.
        println!("cargo:rustc-link-arg=-Wl,--export-dynamic");
    }
}
