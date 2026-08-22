fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // GNU ld auto-exports every public Rust symbol from a Windows cdylib when
    // no explicit C export set exists. rad-vm's cdylib is the wasm-bindgen
    // artifact; native embedders link the Rust rlib, so the native DLL has no
    // C ABI to export. Suppress auto-export instead of manufacturing an
    // unstable table that can exceed PE's 65,535-ordinal limit.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu")
    {
        println!("cargo:rustc-cdylib-link-arg=-Wl,--exclude-all-symbols");
    }
}
