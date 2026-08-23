use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process;

use rad_vm::ast::SourceMap;
use rad_vm::checker::CheckerOptions;
use rad_vm::module_loader::load_program_with_source_map_and_options;
use rad_vm::parser::ParserOptions;
use rad_vm::vm::VM;

#[global_allocator]
static GLOBAL_ALLOCATOR: rad_vm::allocation_meter::MeteredSystemAllocator =
    rad_vm::allocation_meter::MeteredSystemAllocator;

#[derive(Debug, Clone, PartialEq, Eq)]
enum AuthorityQuery {
    Effects { symbol: String },
    Writers { authority: String },
    Readers { authority: String },
    Path { from: String, to: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OperationalQuery {
    QueryPlan {
        target: String,
    },
    CostPath {
        target: String,
    },
    Why {
        entity: String,
        component: String,
    },
    WhyField {
        entity: String,
        component: String,
        field: String,
    },
    WhyRemoved {
        entity: String,
        component: String,
    },
    WhyNotInView {
        view: String,
        entity: String,
    },
}

#[derive(Debug)]
enum CliCommand {
    Surface {
        filepath: String,
        json: bool,
    },
    Authority {
        query: AuthorityQuery,
        filepath: String,
        json: bool,
    },
    Operational {
        query: OperationalQuery,
        filepath: String,
        json: bool,
    },
    Bench {
        filepath: String,
        json: bool,
        program_args: Vec<String>,
    },
    ModelCheck {
        filepath: String,
        model: Option<String>,
        runs: u32,
        max_commands: u32,
        seed: u64,
        artifact_directory: Option<String>,
        json: bool,
    },
    ShrinkModel {
        artifact: String,
    },
    FfiVerify {
        plugin: String,
        contract: Option<String>,
        json: bool,
    },
    Run {
        filepath: String,
        skip_check: bool,
        deny_warnings: bool,
        strict_types: bool,
        write_lock: bool,
        profile_copies: bool,
        serial_schedule: bool,
        features: Vec<String>,
        relation_schema: Option<String>,
        relation_module: String,
        experimental_relations: bool,
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
    Types {
        input_rad: String,
        output_typescript: String,
        features: Vec<String>,
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
// CLI argument parsing and command execution share one private adapter state.
include!("cli/arguments.rs");
include!("cli/program_pipeline.rs");
include!("cli/authority_commands.rs");
include!("cli/operational_commands.rs");
include!("cli/model_commands.rs");
include!("cli/ffi_commands.rs");
include!("cli/source_commands.rs");
include!("cli/run.rs");
include!("cli/test_parallel.rs");
include!("cli/commands_and_diagnostics.rs");
include!("cli/tests.rs");
