#[derive(Debug, Clone)]
pub struct TestDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub body: Block,
    pub is_property: bool,
    /// `shared test` — this test deliberately observes the world the previous
    /// tests in its file left behind. Plain tests are restored to the fixture
    /// state first, so ordering cannot silently become a dependency.
    pub shared_world: bool,
    pub generators: Vec<(String, Expr)>,
    /// Test-scoped semantic write capabilities. They exist only while this
    /// test body is checked and can never be referenced by production code.
    pub ownership_writes: Vec<String>,
}

/// A stateful property model. Commands execute against one evolving world,
/// invariants are checked after every step, and temporal clauses observe both
/// component membership and emitted events. The compiler lowers models to the
/// ordinary executable-test protocol so every adapter gets identical results.
#[derive(Debug, Clone)]
pub struct ModelDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub commands: Vec<Expr>,
    pub invariants: Vec<Block>,
    pub temporal: Vec<TemporalClause>,
    pub runs: u32,
    pub max_commands: u32,
    pub seed: u64,
    /// Test-only ownership capabilities; resolved under the same non-leaking
    /// rules as ordinary `test` declarations.
    pub ownership_writes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemporalClause {
    Always(String),
    Eventually(String),
    Until {
        condition: String,
        terminal: String,
    },
    LeadsTo {
        trigger: String,
        consequence: String,
    },
    ExactlyOnce(String),
    NeverAfter {
        prohibited: String,
        terminal: String,
    },
    EventuallyWithin {
        trigger: String,
        consequence: String,
        bound: u32,
    },
}

#[derive(Debug, Clone)]
pub struct QueryExprNode {
    pub components: Vec<(String, bool)>,
    pub filter: Option<Box<Expr>>,
    pub select: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub type_annotation: Option<TypeExpr>,
    pub default_value: Expr,
    pub is_indexed: bool,
    /// This field has semantic write ownership even when the containing data
    /// type remains generally writable.
    pub is_owned: bool,
    /// Annotation-only fields (`source: entity` — no `=`): the type has no
    /// sensible zero value, so every construction must provide one. The
    /// `default_value` holds a nil placeholder for layout machinery.
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct ResourceDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    /// `transient resource` — runtime state excluded from the world's
    /// identity: world_digest() and save_world() skip it (command
    /// tapes, derived caches, spatial indexes).
    pub transient: bool,
    /// `resource X v2 { … }` — declared schema version, embedded per type
    /// in `save_world()` output and handed to `migrate X(old, from_version)`
    /// on load (dogfood feature seq 69 IDEA 03). 0 = undeclared.
    pub version: u32,
    pub fields: Vec<FieldDef>,
    pub ownership: OwnershipDecl,
}

/// A transactionally maintained subset of entities. Membership requires every
/// dependency and every declarative field predicate; an optional component
/// field supplies a unique lookup key. The closed plan lets the runtime
/// maintain it directly from writes without executing user callbacks inside a
/// commit boundary.
#[derive(Debug, Clone)]
pub struct MaterializedViewDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub dependencies: Vec<String>,
    pub key: Option<(String, String)>,
    pub predicate: crate::materialized_view::MaterializedViewPredicate,
}

/// Source-level semantic write ownership. The declaration's source file is
/// the sole owner; `coowner_modules` names imported module aliases that may
/// also declare explicit `writes owned` capabilities.
#[derive(Debug, Clone, Default)]
pub struct OwnershipDecl {
    pub owned: bool,
    pub coowner_modules: Vec<String>,
    /// `owned transfer to module_alias` moves the sole owner away from the
    /// declaring file. The destination must be an imported module alias.
    pub transferred_to: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EntityDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub components: Vec<ComponentEntry>,
}

#[derive(Debug, Clone)]
pub struct ComponentInit {
    pub id: NodeId,
    pub span: Span,
    pub comp_name: String,
    pub fields: Vec<(String, Expr)>,
}

#[derive(Debug, Clone)]
pub enum ComponentEntry {
    Init(ComponentInit),
    Expr(Expr),
}

impl ComponentEntry {
    pub fn as_init(&self) -> Option<&ComponentInit> {
        match self {
            ComponentEntry::Init(ci) => Some(ci),
            _ => None,
        }
    }

    pub fn as_expr(&self) -> Option<&Expr> {
        match self {
            ComponentEntry::Expr(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StateDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub states: Vec<StateDef>,
}

#[derive(Debug, Clone)]
pub struct StateDef {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub transitions: Vec<(String, String, Option<Expr>)>,
}

#[derive(Debug, Clone)]
pub struct SystemDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub params: Vec<(String, bool, String)>,
    /// Names of params declared `accum` (dogfood feature seq 83 IDEA 02).
    /// They also appear in `params` with `is_mut = true` — `accum` is `mut`
    /// plus fold-on-merge semantics for parallel batches.
    pub accum_params: Vec<String>,
    /// Authority-only signature entries. Unlike `params`, these do not alter
    /// the ECS query or bind a value in the body; they make cross-entity and
    /// resource authority explicit without changing execution cardinality.
    pub authority_reads: Vec<String>,
    pub authority_writes: Vec<String>,
    /// Event effects are also an authority-only upper bound. `"*"` grants
    /// every event channel, including runtime-owned transition/flush channels.
    pub authority_emits: Vec<String>,
    /// Explicit grants for transitive I/O and asynchronous reachability.
    pub authority_io: bool,
    pub authority_async: bool,
    /// Semantic ownership grants; unlike ordinary authority these can only be
    /// exercised by the owner module, an explicitly named co-owner, or a test.
    pub ownership_writes: Vec<String>,
    pub body: Block,
    pub after: Vec<String>,
    pub before: Vec<String>,
    pub contracts: CallableContracts,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallableContracts {
    pub frame: bool,
    pub tick: bool,
    pub render: bool,
    pub no_full_scan: bool,
    /// No managed guest object may be allocated on this callable's
    /// transitive path. Statically checked and confirmed by `rad bench`.
    pub no_guest_allocation: bool,
    /// No Rust process-allocator call may originate in VM runtime code while
    /// this callable runs. Verified by the metered benchmark executable.
    pub no_runtime_allocation: bool,
    /// No allocation may cross RAD's metered native host-call boundary.
    /// Plugin-private allocators remain a separately verified FFI contract.
    pub no_host_allocation: bool,
    pub instruction_budget: Option<u64>,
    pub allow_full_scan_reason: Option<String>,
    pub no_nested_flush: bool,
    pub non_reentrant: bool,
    pub must_complete_before: Option<String>,
    pub exactly_once: bool,
}

#[derive(Debug, Clone)]
pub struct EventDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    /// Field name and optional type (`event E { a, b: int }`). Untyped fields infer as `any` in the checker.
    pub fields: Vec<(String, Option<TypeExpr>)>,
}

/// A transient proposal schema owned by one resolver in its defining module.
#[derive(Debug, Clone)]
pub struct IntentDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub fields: Vec<IntentField>,
}

#[derive(Debug, Clone)]
pub struct IntentField {
    pub span: Span,
    pub name: String,
    pub type_annotation: TypeExpr,
    pub is_key: bool,
}

/// A read-only producer that can only be invoked from a settlement.
#[derive(Debug, Clone)]
pub struct LawDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub params: Vec<String>,
    pub param_types: Vec<TypeExpr>,
    pub body: Block,
}

/// The single semantic owner of one intent type.
#[derive(Debug, Clone)]
pub struct ResolverDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub intent_name: String,
    pub key_param: String,
    pub proposals_param: String,
    pub body: Block,
}

/// A validation-only invariant over one complete settlement candidate.
#[derive(Debug, Clone)]
pub struct ConstraintDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub component_name: String,
    pub subject_param: String,
    pub proposed_param: String,
    pub watches: Vec<String>,
    pub body: Block,
}

/// Schema migration (list item #5): `migrate Health(old) { return Health { … } }`.
/// Invoked by `load_world` when a persisted component/resource shape differs
/// from the declared one. `param_name` binds the stored fields as a
/// `map<str, any>` — the old shape no longer exists as a type.
#[derive(Debug, Clone)]
pub struct MigrationDecl {
    pub id: NodeId,
    pub span: Span,
    pub component: String,
    pub param_name: String,
    /// `migrate X(old, from_version)` — optional second parameter binding
    /// the schema version the SAVE declared for `X` (`component X v2 { … }`
    /// at save time), or 0 for saves without one (dogfood feature seq 69
    /// IDEA 03: turn shape-sniffing into a fact).
    pub version_param: Option<String>,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct OnHandler {
    pub id: NodeId,
    pub span: Span,
    pub event_name: String,
    pub param_name: String,
    pub body: Block,
    pub once: bool,
    pub is_async: bool,
    pub has_guard: bool,
    pub ownership_writes: Vec<String>,
    pub contracts: CallableContracts,
}

#[derive(Debug, Clone)]
pub struct FnDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub type_params: Vec<String>,
    pub params: Vec<String>,
    pub param_muts: Vec<bool>,
    pub param_types: Vec<Option<TypeExpr>>,
    pub return_type: Option<TypeExpr>,
    pub body: Block,
    pub is_pure: bool,
    pub is_async: bool,
    pub effects: Vec<String>,
    pub ownership_writes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TypeAliasDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub type_params: Vec<String>,
    pub target: TypeExpr,
}

#[derive(Debug, Clone)]
pub struct TypeDeclNode {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub type_params: Vec<String>,
    pub variants: Vec<VariantDefNode>,
}

#[derive(Debug, Clone)]
pub struct VariantDefNode {
    pub name: String,
    pub fields: Vec<(String, Expr)>,
    /// Fields declared with a TYPE annotation instead of a default value
    /// (`Homing { target: entity }`). Sparse: only annotated names appear;
    /// their `fields` entry carries a nil placeholder default.
    pub annotations: Vec<(String, TypeExpr)>,
}

#[derive(Debug, Clone)]
pub struct UseStmt {
    pub id: NodeId,
    pub span: Span,
    pub path: String,
    pub alias: Option<String>,
    pub contract: Option<String>,
    /// Loader-owned normalized target identity. Parser-only programs leave
    /// this unset; checked multi-module programs require it for provenance.
    pub module_identity: Option<String>,
}
