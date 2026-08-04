use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process;

use rad_vm::ast::SourceMap;
use rad_vm::checker::{Checker, CheckerOptions};
use rad_vm::compiler::Compiler;
use rad_vm::module_loader::load_program_with_source_map_and_options;
use rad_vm::parser::ParserOptions;
use rad_vm::vm::VM;

#[derive(Debug)]
enum CliCommand {
    Run {
        filepath: String,
        skip_check: bool,
        compat_v0_5_dx: bool,
        deny_warnings: bool,
        warn_compat: bool,
        strict_types: bool,
        write_lock: bool,
        profile_copies: bool,
        serial_schedule: bool,
        features: Vec<String>,
        record: Option<String>,
        program_args: Vec<String>,
    },
    Fmt {
        filepaths: Vec<String>,
        check_only: bool,
    },
    Lint {
        filepaths: Vec<String>,
        preset: String,
        boundaries: HashMap<String, Vec<String>>,
    },
    Lsp {
        experimental_relations: bool,
    },
    RelationsCheck {
        filepath: String,
        module_id: String,
        experimental_relations: bool,
    },
    Test {
        test_dir: String,
    },
    New {
        project_name: Option<String>,
        template: Option<String>,
        list_templates: bool,
    },
    Snapshot {
        directory: Option<String>,
        update: bool,
        create: bool,
        experimental_laws: bool,
    },
    Play {
        port: u16,
    },
    Build {
        input_rad: String,
        output_wasm: String,
    },
    SandboxServe {
        host_file: Option<String>,
        caps_file: Option<String>,
    },
    Replay {
        trace_path: String,
        to_frame: Option<u64>,
        force: bool,
        serve: bool,
        with_source: Option<String>,
    },
    Version,
}
// Lexical sections preserve one private semantic namespace.
include!("main/cli.rs");
include!("main/main.rs");
include!("main/commands_and_diagnostics.rs");
