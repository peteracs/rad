#![cfg(all(feature = "native-wasm-phase3", not(target_arch = "wasm32")))]

use rad_vm::wasm_compiler_host::WasmCompilerHost;

#[test]
fn reactor_stub_loads() {
    let mut host = WasmCompilerHost::default_stub().expect("load stub wasm");
    assert_eq!(host.rad_init().expect("init"), 0);
    host.rad_update_buffer(0, "let x = 1").expect("buffer");
    let diags = host.rad_check().expect("check");
    assert!(diags.is_empty());
}
