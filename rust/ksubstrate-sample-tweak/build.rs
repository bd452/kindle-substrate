fn main() {
    println!("cargo:rustc-check-cfg=cfg(ksubstrate_dynamic)");
    if let Ok(lib_dir) = std::env::var("KSUBSTRATE_LIB_DIR") {
        println!("cargo:rustc-link-search=native={lib_dir}");
        println!("cargo:rustc-link-lib=dylib=ksubstrate");
        println!("cargo:rustc-cfg=ksubstrate_dynamic");
    }
}
