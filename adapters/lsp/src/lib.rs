use std::collections::HashMap;
use std::path::Path;
#[cfg(all(feature = "native-wasm-phase3", not(target_arch = "wasm32")))]
use std::path::PathBuf;
#[cfg(all(feature = "native-wasm-phase3", not(target_arch = "wasm32")))]
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use rad_vm::checker::{CheckerOptions, CheckerSemanticIndex};
use rad_vm::module_loader::{load_program_with_overrides, LoadResult};
use rad_vm::parser::ParserOptions;
use rad_vm::pipeline::Diagnostics;
use rad_vm::simulate_syntax;
use rad_vm::types::SystemType;

pub struct LspBackend {
    pub client: Client,
    pub documents: RwLock<HashMap<Url, String>>,
    pub experimental_relations: bool,
}
// Lexical sections preserve one private semantic namespace.
include!("lsp/semantics.rs");
include!("lsp/check_document.rs");
include!("lsp/server.rs");
include!("lsp/document_analysis.rs");
