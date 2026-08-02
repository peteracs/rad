//! WebAssembly binary emission for Phase 3 (`compiler.wasm` reactor ABI).
//!
//! When the `native-wasm-phase3` feature is enabled, modules are built with `wasm-encoder`.
//! Otherwise a tiny hand-encoded placeholder is used for `wasm32` / no-default-features builds.

#[cfg(all(feature = "native-wasm-phase3", not(target_arch = "wasm32")))]
fn emit_compiler_reactor_stub_module_impl() -> Vec<u8> {
    use wasm_encoder::{
        CodeSection, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
        Instruction, MemorySection, MemoryType, Module, TypeSection, ValType,
    };

    let mut module = Module::new();

    // Type indices: 0 vfs_read, 1 rad_init, 2 rad_update_buffer, 3 rad_check, 4 rad_query_lsp
    let mut types = TypeSection::new();
    types
        .ty()
        .function([ValType::I32, ValType::I32], [ValType::I64]);
    types.ty().function([], [ValType::I32]);
    types
        .ty()
        .function([ValType::I32, ValType::I32, ValType::I32], []);
    types.ty().function([], [ValType::I64]);
    types
        .ty()
        .function([ValType::I32, ValType::I32, ValType::I32], [ValType::I64]);
    module.section(&types);

    let mut imports = ImportSection::new();
    imports.import("env", "vfs_read", wasm_encoder::EntityType::Function(0));
    module.section(&imports);

    let mut funcs = FunctionSection::new();
    funcs.function(1);
    funcs.function(2);
    funcs.function(3);
    funcs.function(4);
    module.section(&funcs);

    let mut memories = MemorySection::new();
    memories.memory(MemoryType {
        minimum: 2,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    module.section(&memories);

    let mut exports = ExportSection::new();
    exports.export("memory", ExportKind::Memory, 0);
    exports.export("rad_init", ExportKind::Func, 1);
    exports.export("rad_update_buffer", ExportKind::Func, 2);
    exports.export("rad_check", ExportKind::Func, 3);
    exports.export("rad_query_lsp", ExportKind::Func, 4);
    module.section(&exports);

    // Function bodies: local indices 1..4 (0 is import)
    // rad_init: i32.const 0; end
    let mut code = CodeSection::new();
    let mut f_rad_init = Function::new([]);
    f_rad_init.instruction(&Instruction::I32Const(0));
    f_rad_init.instruction(&Instruction::End);
    code.function(&f_rad_init);

    // rad_update_buffer: empty
    let mut f_update = Function::new([]);
    f_update.instruction(&Instruction::End);
    code.function(&f_update);

    // rad_check: i64.const 0 (no diagnostics)
    let mut f_check = Function::new([]);
    f_check.instruction(&Instruction::I64Const(0));
    f_check.instruction(&Instruction::End);
    code.function(&f_check);

    // rad_query_lsp: i64.const 0
    let mut f_query = Function::new([]);
    f_query.instruction(&Instruction::I64Const(0));
    f_query.instruction(&Instruction::End);
    code.function(&f_query);

    module.section(&code);

    module.finish()
}

#[cfg(not(all(feature = "native-wasm-phase3", not(target_arch = "wasm32"))))]
fn emit_compiler_reactor_stub_module_impl() -> Vec<u8> {
    emit_minimal_wasm_module()
}

/// Phase 3 reactor stub: imports `env.vfs_read`, exports memory and `rad_*` entrypoints.
/// Full `compiler.wasm` replaces this once [`emit_wasm.rad`](../../core/c-backend/src/emit_wasm.rad) lowers the self-hosted compiler.
pub fn emit_compiler_reactor_stub_module() -> Vec<u8> {
    emit_compiler_reactor_stub_module_impl()
}

/// Backwards-compatible name: minimal valid v1 module (single empty export).
pub fn emit_minimal_wasm_module() -> Vec<u8> {
    vec![
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x04, 0x01, 0x60, 0x00, 0x00, 0x03,
        0x02, 0x01, 0x00, 0x07, 0x13, 0x01, 0x0f, 0x72, 0x61, 0x64, 0x5f, 0x70, 0x6c, 0x61, 0x63,
        0x65, 0x68, 0x6f, 0x6c, 0x64, 0x65, 0x72, 0x00, 0x00, 0x0a, 0x04, 0x01, 0x02, 0x00, 0x0b,
    ]
}
