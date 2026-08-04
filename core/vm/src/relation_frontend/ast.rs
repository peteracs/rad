use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct SourceSpan {
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RelationType {
    Entity,
    Int,
    Count,
    Text,
}

impl RelationType {
    pub(crate) fn tag(self) -> u8 {
        match self {
            Self::Entity => b'e',
            Self::Int => b'i',
            Self::Count => b'c',
            Self::Text => b't',
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum OnDelete {
    Restrict,
    Cascade,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RelationKind {
    Authoritative,
    Derived,
}

impl RelationKind {
    pub(crate) fn tag(self) -> u8 {
        match self {
            Self::Authoritative => b'a',
            Self::Derived => b'd',
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RelationColumn {
    pub name: String,
    pub value_type: RelationType,
    pub on_delete: Option<OnDelete>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct UniqueConstraint {
    pub name: String,
    pub columns: Vec<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RelationSchema {
    pub identity: String,
    pub owner: String,
    pub kind: RelationKind,
    pub columns: Vec<RelationColumn>,
    pub unique: Vec<UniqueConstraint>,
    pub symmetric: bool,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Literal {
    Int(i64),
    Count(u64),
    Text(String),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum RawTerm {
    Variable(String),
    Literal(Literal),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct RawAtom {
    pub relation: String,
    pub terms: Vec<RawTerm>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum RawPredicate {
    Greater(String, String),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AggregateKind {
    Count,
    Sum,
    Min,
    Max,
}

impl AggregateKind {
    pub(crate) fn tag(self) -> u8 {
        match self {
            Self::Count => b'c',
            Self::Sum => b's',
            Self::Min => b'n',
            Self::Max => b'x',
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct RawAggregate {
    pub kind: AggregateKind,
    pub input: Option<String>,
    pub output: String,
    pub group_by: Vec<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct RawRuleAst {
    pub explicit_id: Option<String>,
    pub head_relation: String,
    pub head: Vec<RawTerm>,
    pub atoms: Vec<RawAtom>,
    pub predicates: Vec<RawPredicate>,
    pub aggregate: Option<RawAggregate>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RawRuleSummary {
    pub maximum_identifier_length: usize,
    pub head_terms: usize,
    pub total_terms: usize,
    pub atoms: usize,
    pub predicates: usize,
    pub aggregate_groups: usize,
    pub ast_nodes: usize,
    pub structural_cost: usize,
}

#[derive(Clone, Debug)]
pub struct BoundedRawRule {
    pub(super) ast: RawRuleAst,
    pub(super) module_id: Arc<str>,
    pub(super) source_span: SourceSpan,
    summary: RawRuleSummary,
}

impl BoundedRawRule {
    pub fn summary(&self) -> RawRuleSummary {
        self.summary
    }

    pub(super) fn new(
        ast: RawRuleAst,
        module_id: Arc<str>,
        source_span: SourceSpan,
        summary: RawRuleSummary,
    ) -> Self {
        Self {
            ast,
            module_id,
            source_span,
            summary,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RelationOperationKind {
    Insert,
    Remove,
    ReplaceBy {
        constraint: String,
        key: Vec<RawOperationValue>,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RawOperationValue {
    /// A ground symbolic entity reference resolved by the eventual candidate.
    EntitySymbol(String),
    Literal(Literal),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RelationOperation {
    pub kind: RelationOperationKind,
    pub relation: String,
    pub owner: String,
    pub tuple: Vec<RawOperationValue>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RawInputStats {
    pub source_bytes: usize,
    pub tokens: usize,
    pub ast_nodes: usize,
    pub relations: usize,
    pub rules: usize,
    pub operations: usize,
    pub structural_cost: usize,
}

#[derive(Clone, Debug)]
pub struct BoundedRawProgram {
    pub(super) module_ids: Vec<String>,
    pub(super) relations: Vec<RelationSchema>,
    pub(super) relation_spans: Vec<SourceSpan>,
    pub(super) rules: Vec<BoundedRawRule>,
    pub(super) operations: Vec<RelationOperation>,
    pub(super) operation_spans: Vec<SourceSpan>,
    stats: RawInputStats,
}

impl BoundedRawProgram {
    pub fn input_stats(&self) -> RawInputStats {
        self.stats
    }

    pub fn rule_summaries(&self) -> impl ExactSizeIterator<Item = RawRuleSummary> + '_ {
        self.rules.iter().map(BoundedRawRule::summary)
    }

    pub(super) fn new(
        module_ids: Vec<String>,
        relations: Vec<RelationSchema>,
        relation_spans: Vec<SourceSpan>,
        rules: Vec<BoundedRawRule>,
        operations: Vec<RelationOperation>,
        operation_spans: Vec<SourceSpan>,
        stats: RawInputStats,
    ) -> Self {
        Self {
            module_ids,
            relations,
            relation_spans,
            rules,
            operations,
            operation_spans,
            stats,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticResourceQuote {
    pub atoms: usize,
    pub predicates: usize,
    pub terms: usize,
    pub canonical_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct SealedRulePlan {
    identity: String,
    canonical_bytes: Arc<[u8]>,
    digest: [u8; 32],
    dependencies: Arc<[String]>,
    inferred_head: RelationSchema,
    resource_quote: StaticResourceQuote,
}

impl SealedRulePlan {
    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }

    pub fn dependencies(&self) -> &[String] {
        &self.dependencies
    }

    pub fn inferred_head(&self) -> &RelationSchema {
        &self.inferred_head
    }

    pub fn resource_quote(&self) -> &StaticResourceQuote {
        &self.resource_quote
    }

    pub(crate) fn new(
        identity: String,
        canonical_bytes: Vec<u8>,
        digest: [u8; 32],
        dependencies: Vec<String>,
        inferred_head: RelationSchema,
        resource_quote: StaticResourceQuote,
    ) -> Self {
        Self {
            identity,
            canonical_bytes: canonical_bytes.into(),
            digest,
            dependencies: dependencies.into(),
            inferred_head,
            resource_quote,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RelationManifest {
    schemas: Arc<[RelationSchema]>,
}

impl RelationManifest {
    pub fn schemas(&self) -> &[RelationSchema] {
        &self.schemas
    }

    pub(crate) fn new(schemas: Vec<RelationSchema>) -> Self {
        Self {
            schemas: schemas.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DerivationDependencyDag {
    edges: Arc<[(String, String)]>,
}

impl DerivationDependencyDag {
    pub fn edges(&self) -> &[(String, String)] {
        &self.edges
    }

    pub(crate) fn new(edges: Vec<(String, String)>) -> Self {
        Self {
            edges: edges.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct FrontendArtifacts {
    pub modules: Arc<[String]>,
    pub relations: RelationManifest,
    pub rules: Arc<[Arc<SealedRulePlan>]>,
    pub dependency_dag: DerivationDependencyDag,
    pub operations: Arc<[RelationOperation]>,
    pub manifest_digest: FrontendManifestDigest,
}

impl FrontendArtifacts {
    /// Recompute the sealed identity from the immutable semantic artifacts.
    /// Runtime installation uses this instead of trusting public aggregate
    /// fields to remain paired with the digest returned by the checker.
    pub fn verify_manifest_digest(&self) -> bool {
        super::canonical::manifest_digest(
            &self.modules,
            self.relations.schemas(),
            &self.rules,
            self.dependency_dag.edges(),
            &self.operations,
        ) == self.manifest_digest.as_bytes()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrontendManifestDigest([u8; 32]);

impl FrontendManifestDigest {
    pub fn as_bytes(self) -> [u8; 32] {
        self.0
    }

    pub(crate) fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}
