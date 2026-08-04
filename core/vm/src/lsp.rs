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

use crate::checker::{Checker, CheckerOptions};
use crate::module_loader::load_program_with_overrides;
use crate::parser::ParserOptions;
use crate::simulate_syntax;
use crate::types::SystemType;

pub struct LspBackend {
    pub client: Client,
    pub documents: RwLock<HashMap<Url, String>>,
    pub experimental_relations: bool,
}
// Lexical sections preserve one private semantic namespace.
include!("lsp/check_document.rs");
include!("lsp/server.rs");
include!("lsp/document_analysis.rs");
