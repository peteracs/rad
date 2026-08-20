use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use crate::ast::{
    Block, Decl, Expr, FStringPart, FnDecl, FnTypePurity, OnHandler, Pattern, Program, Span, Stmt,
    SystemDecl, TypeExpr,
};
use crate::builtins;
use crate::types::{AuthorityCallableKind, AuthorityEffects, AuthorityReport, CallableAuthority};

use super::Checker;

mod builtin_scanner;
mod enforcement;
mod graph;

const WHOLE_WORLD: &str = "*";
const EVENT_LOG: &str = "$events";
const FORK_STATE: &str = "$fork";
const SCHEMA: &str = "$schema";
const STATE_TRANSITION: &str = "$transition";
const ENTITY_NAMES: &str = "$entity_names";
const ENTITY_IDENTITY: &str = "$entity_identity";

#[derive(Clone, Default)]
struct EffectDraft {
    reads: BTreeSet<String>,
    writes: BTreeSet<String>,
    emits: BTreeSet<String>,
    io: bool,
    async_effect: bool,
    unknown: bool,
}

impl EffectDraft {
    fn merge(&mut self, other: &Self) -> bool {
        let before = (
            self.reads.len(),
            self.writes.len(),
            self.emits.len(),
            self.io,
            self.async_effect,
            self.unknown,
        );
        self.reads.extend(other.reads.iter().cloned());
        self.writes.extend(other.writes.iter().cloned());
        self.emits.extend(other.emits.iter().cloned());
        self.io |= other.io;
        self.async_effect |= other.async_effect;
        self.unknown |= other.unknown;
        before
            != (
                self.reads.len(),
                self.writes.len(),
                self.emits.len(),
                self.io,
                self.async_effect,
                self.unknown,
            )
    }

    fn freeze(&self) -> AuthorityEffects {
        AuthorityEffects {
            reads: self.reads.iter().cloned().collect(),
            writes: self.writes.iter().cloned().collect(),
            emits: self.emits.iter().cloned().collect(),
            io: self.io,
            async_effect: self.async_effect,
            unknown: self.unknown,
        }
    }
}

#[derive(Clone)]
struct Seed {
    name: String,
    display_name: String,
    kind: AuthorityCallableKind,
    block: Block,
    span: Span,
    redirects: HashMap<String, String>,
    params: Vec<String>,
    param_bounds: BTreeMap<usize, CallableBound>,
    system_params: BTreeMap<String, (String, bool)>,
    authority_reads: BTreeSet<String>,
    authority_writes: BTreeSet<String>,
    authority_emits: BTreeSet<String>,
    authority_io: bool,
    authority_async: bool,
    declared: EffectDraft,
    captured_locals: HashMap<String, LocalBinding>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum CallableBound {
    Pure,
    Readonly,
    Unbounded,
}

impl CallableBound {
    fn from_type(annotation: &TypeExpr) -> Option<Self> {
        let TypeExpr::FnType(_, _, purity) = annotation else {
            return None;
        };
        Some(match purity {
            FnTypePurity::Pure => Self::Pure,
            FnTypePurity::Readonly => Self::Readonly,
            FnTypePurity::Default => Self::Unbounded,
        })
    }

    fn apply(self, effects: &mut EffectDraft) -> bool {
        let before = (
            effects.reads.len(),
            effects.writes.len(),
            effects.emits.len(),
            effects.io,
            effects.async_effect,
            effects.unknown,
        );
        match self {
            Self::Pure => {}
            Self::Readonly => {
                effects.reads.insert(WHOLE_WORLD.to_string());
            }
            Self::Unbounded => effects.unknown = true,
        }
        before
            != (
                effects.reads.len(),
                effects.writes.len(),
                effects.emits.len(),
                effects.io,
                effects.async_effect,
                effects.unknown,
            )
    }

    fn union(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unbounded, _) | (_, Self::Unbounded) => Self::Unbounded,
            (Self::Readonly, _) | (_, Self::Readonly) => Self::Readonly,
            (Self::Pure, Self::Pure) => Self::Pure,
        }
    }
}

#[derive(Clone)]
struct NodeDraft {
    seed: Seed,
    direct: EffectDraft,
    calls: BTreeSet<String>,
    /// Synchronous edges that do not carry higher-order arguments (builtin
    /// callbacks, statically named simulation systems, and flushed handlers).
    /// Ordinary function calls are represented by `CallSite` so system
    /// specialization can keep their callback tuple separate.
    plain_calls: BTreeSet<String>,
    deferred_calls: BTreeSet<String>,
    dynamic_params: BTreeMap<usize, CallableBound>,
    /// Writes already diagnosed by the ordinary mutability checker. Keeping
    /// these separate avoids emitting two errors for `p.x = ...` when `p` is
    /// a read-only system parameter; builtin/helper writes still receive the
    /// authority diagnostic.
    parameter_assignments: BTreeSet<String>,
}

#[derive(Clone)]
struct CallSite {
    caller: String,
    callee: String,
    args: Vec<CallableArgument>,
}

#[derive(Clone, Default)]
struct CallableArgument {
    targets: BTreeSet<String>,
    forwarded_param: Option<(usize, CallableBound)>,
    bound: Option<CallableBound>,
}

impl CallableArgument {
    fn merge(&mut self, other: &Self) -> bool {
        let before = (self.targets.len(), self.bound);
        self.targets.extend(other.targets.iter().cloned());
        self.bound = match (self.bound, other.bound) {
            (Some(left), Some(right)) => Some(left.union(right)),
            (bound @ Some(_), None) | (None, bound @ Some(_)) => bound,
            (None, None) => None,
        };
        before != (self.targets.len(), self.bound)
    }
}

#[derive(Clone, Default)]
struct LocalBinding {
    targets: BTreeSet<String>,
    bound: Option<CallableBound>,
}

struct Resolver {
    known_callables: HashSet<String>,
    known_data: HashSet<String>,
    module_aliases: HashMap<String, HashMap<String, String>>,
    type_redirects: HashMap<String, String>,
    static_schedules: HashMap<String, Vec<Expr>>,
    phases: HashMap<String, Vec<String>>,
}

impl Resolver {
    fn symbol_name(&self, raw: &str, redirects: &HashMap<String, String>) -> String {
        redirects
            .get(raw)
            .cloned()
            .or_else(|| {
                raw.split_once('.').and_then(|(alias, member)| {
                    self.module_aliases
                        .get(alias)
                        .and_then(|members| members.get(member))
                        .cloned()
                })
            })
            .unwrap_or_else(|| raw.to_string())
    }

    fn callable(&self, expr: &Expr, redirects: &HashMap<String, String>) -> Option<String> {
        match expr {
            Expr::Ident(name, _) => {
                let resolved = self.symbol_name(name, redirects);
                self.known_callables.contains(&resolved).then_some(resolved)
            }
            Expr::Field(owner, member, _) => match owner.as_ref() {
                Expr::Ident(alias, _) => self
                    .module_aliases
                    .get(alias)
                    .and_then(|members| members.get(member))
                    .filter(|name| self.known_callables.contains(*name))
                    .cloned(),
                _ => None,
            },
            _ => None,
        }
    }

    fn system_ref(&self, path: &[String], redirects: &HashMap<String, String>) -> Option<String> {
        let raw = crate::simulate_syntax::system_ref_qualified_string(path);
        let resolved = self.symbol_name(&raw, redirects);
        self.known_callables.contains(&resolved).then_some(resolved)
    }

    fn type_name(&self, raw: &str, redirects: &HashMap<String, String>) -> String {
        let mut current = self.symbol_name(raw, redirects);
        let mut seen = HashSet::new();
        while seen.insert(current.clone()) {
            let Some(next) = self.type_redirects.get(&current) else {
                break;
            };
            current = next.clone();
        }
        current
    }

    fn data_expr(&self, expr: &Expr, redirects: &HashMap<String, String>) -> Option<String> {
        let raw = match expr {
            Expr::Ident(name, _) | Expr::ComponentExpr(name, _, _, _) => name.clone(),
            Expr::Field(owner, member, _) => match owner.as_ref() {
                Expr::Ident(alias, _) => format!("{alias}.{member}"),
                _ => return None,
            },
            Expr::StrLit(name, _) => name.clone(),
            _ => return None,
        };
        let canonical = self.type_name(&raw, redirects);
        self.known_data.contains(&canonical).then_some(canonical)
    }
}

struct Scanner<'a> {
    resolver: &'a Resolver,
    seed: &'a Seed,
    direct: EffectDraft,
    calls: BTreeSet<String>,
    plain_calls: BTreeSet<String>,
    deferred_calls: BTreeSet<String>,
    dynamic_params: BTreeMap<usize, CallableBound>,
    parameter_assignments: BTreeSet<String>,
    call_sites: Vec<CallSite>,
    closures: Vec<Seed>,
    closure_names: HashMap<(Option<u32>, u32, u32), String>,
    local_scopes: Vec<HashMap<String, LocalBinding>>,
}

impl<'a> Scanner<'a> {
    fn new(resolver: &'a Resolver, seed: &'a Seed) -> Self {
        let mut direct = seed.declared.clone();
        if seed.kind == AuthorityCallableKind::System {
            direct
                .reads
                .extend(seed.system_params.values().map(|(name, _)| name.clone()));
        }
        Self {
            resolver,
            seed,
            direct,
            calls: BTreeSet::new(),
            plain_calls: BTreeSet::new(),
            deferred_calls: BTreeSet::new(),
            dynamic_params: BTreeMap::new(),
            parameter_assignments: BTreeSet::new(),
            call_sites: Vec::new(),
            closures: Vec::new(),
            closure_names: HashMap::new(),
            local_scopes: vec![seed.captured_locals.clone()],
        }
    }

    fn scan(mut self) -> (NodeDraft, Vec<Seed>, Vec<CallSite>) {
        self.scan_block(&self.seed.block);
        let node = NodeDraft {
            seed: self.seed.clone(),
            direct: self.direct,
            calls: self.calls,
            plain_calls: self.plain_calls,
            deferred_calls: self.deferred_calls,
            dynamic_params: self.dynamic_params,
            parameter_assignments: self.parameter_assignments,
        };
        (node, self.closures, self.call_sites)
    }

    fn scan_block(&mut self, block: &Block) {
        self.local_scopes.push(HashMap::new());
        for stmt in &block.stmts {
            self.scan_stmt(stmt);
        }
        self.local_scopes.pop();
    }

    fn scan_block_with_bindings(&mut self, block: &Block, bindings: &[String]) {
        self.local_scopes.push(HashMap::new());
        for binding in bindings {
            self.declare_local(binding.clone(), LocalBinding::default());
        }
        for stmt in &block.stmts {
            self.scan_stmt(stmt);
        }
        self.local_scopes.pop();
    }

    fn declare_local(&mut self, name: String, binding: LocalBinding) {
        self.local_scopes
            .last_mut()
            .expect("callable scanning always owns a lexical scope")
            .insert(name, binding);
    }

    fn local(&self, name: &str) -> Option<&LocalBinding> {
        self.local_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
    }

    fn local_mut(&mut self, name: &str) -> Option<&mut LocalBinding> {
        self.local_scopes
            .iter_mut()
            .rev()
            .find_map(|scope| scope.get_mut(name))
    }

    fn visible_locals(&self) -> HashMap<String, LocalBinding> {
        let mut visible = HashMap::new();
        for scope in &self.local_scopes {
            for (name, binding) in scope {
                visible.insert(name.clone(), binding.clone());
            }
        }
        visible
    }

    fn scan_match_case(&mut self, case: &crate::ast::MatchCase) {
        if let Pattern::Literal(value) = &case.pattern {
            self.scan_expr(value);
        }
        self.local_scopes.push(HashMap::new());
        for binding in pattern_binding_names(&case.pattern) {
            self.declare_local(binding, LocalBinding::default());
        }
        if let Some(guard) = &case.guard {
            self.scan_expr(guard);
        }
        for stmt in &case.body.stmts {
            self.scan_stmt(stmt);
        }
        self.local_scopes.pop();
    }

    fn scan_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let(s) => {
                self.scan_expr(&s.value);
                if s.names.len() == 1 {
                    let targets = self.callable_targets(&s.value);
                    let bound = s
                        .type_annotation
                        .as_ref()
                        .and_then(CallableBound::from_type);
                    self.declare_local(s.names[0].clone(), LocalBinding { targets, bound });
                } else {
                    for name in &s.names {
                        self.declare_local(name.clone(), LocalBinding::default());
                    }
                }
            }
            Stmt::LetElse(s) => {
                self.scan_expr(&s.subject);
                self.scan_block(&s.else_block);
                let bound = s
                    .type_annotation
                    .as_ref()
                    .and_then(CallableBound::from_type);
                let names: Vec<&String> = if s.pattern_bindings.is_empty() {
                    s.bindings.iter().collect()
                } else {
                    s.pattern_bindings
                        .iter()
                        .map(|binding| &binding.name)
                        .collect()
                };
                for name in names {
                    self.declare_local(
                        name.clone(),
                        LocalBinding {
                            targets: BTreeSet::new(),
                            bound,
                        },
                    );
                }
            }
            Stmt::Assign(s) => {
                if let Expr::Ident(name, _) = &s.target {
                    let targets = self.callable_targets(&s.value);
                    if let Some(local) = self.local_mut(name) {
                        if targets.is_empty() {
                            if !local.targets.is_empty() || local.bound.is_some() {
                                local.bound = Some(CallableBound::Unbounded);
                            }
                        } else {
                            local.targets.extend(targets);
                        }
                    }
                }
                self.record_system_param_write(&s.target);
                self.scan_expr(&s.target);
                self.scan_expr(&s.value);
            }
            Stmt::If(s) => {
                self.scan_expr(&s.condition);
                self.scan_block(&s.then_block);
                if let Some(block) = &s.else_block {
                    self.scan_block(block);
                }
            }
            Stmt::While(s) => {
                self.scan_expr(&s.condition);
                self.scan_block(&s.body);
            }
            Stmt::For(s) => {
                self.scan_expr(&s.iterable);
                let bindings = s.destructure_bindings.as_ref().unwrap_or(&s.bindings);
                self.scan_block_with_bindings(&s.body, bindings);
            }
            Stmt::Return(s) => {
                if let Some(value) = &s.value {
                    self.scan_expr(value);
                }
            }
            Stmt::Emit(s) => {
                let event = self.resolver.type_name(&s.event_name, &self.seed.redirects);
                self.direct.emits.insert(event);
                for (_, value) in &s.fields {
                    self.scan_expr(value);
                }
                if let Some(delay) = &s.delay {
                    self.scan_expr(delay);
                }
            }
            Stmt::Schedule(s) => {
                for system in &s.systems {
                    let target = self.resolver.symbol_name(system, &self.seed.redirects);
                    if self.resolver.known_callables.contains(&target) {
                        self.deferred_calls.insert(target);
                    } else if let Some(members) = self.resolver.phases.get(&target) {
                        for member in members {
                            if self.resolver.known_callables.contains(member) {
                                self.deferred_calls.insert(member.clone());
                            } else {
                                self.direct.unknown = true;
                            }
                        }
                    } else {
                        self.direct.unknown = true;
                    }
                }
            }
            Stmt::Update(s) => {
                let name = self.resolver.type_name(&s.comp_name, &self.seed.redirects);
                self.direct.reads.insert(name.clone());
                self.direct.writes.insert(name);
                if let Some(entity) = &s.entity_expr {
                    self.scan_expr(entity);
                }
                for update in &s.field_updates {
                    if let Some(index) = &update.index {
                        self.scan_expr(index);
                    }
                    self.scan_expr(&update.value);
                }
            }
            Stmt::Settle(s) => self.scan_block(&s.body),
            Stmt::Propose(s) => {
                self.direct
                    .emits
                    .insert(format!("intent::{}", s.intent_name));
                for (_, value) in &s.fields {
                    self.scan_expr(value);
                }
            }
            Stmt::Next(s) => {
                let name = self
                    .resolver
                    .type_name(&s.component_name, &self.seed.redirects);
                self.direct.reads.insert(name.clone());
                self.direct.writes.insert(name);
                self.scan_expr(&s.entity);
                for (_, value) in &s.fields {
                    self.scan_expr(value);
                }
            }
            Stmt::Require(s) => self.scan_expr(&s.condition),
            Stmt::Match(s) => {
                self.scan_expr(&s.subject);
                for case in &s.cases {
                    self.scan_match_case(case);
                }
            }
            Stmt::Expr(s) => self.scan_expr(&s.expr),
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::OnceGuardPass(_) | Stmt::Error(_) => {}
        }
    }

    fn scan_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::IntLit(_, _)
            | Expr::FloatLit(_, _)
            | Expr::StrLit(_, _)
            | Expr::BoolLit(_, _)
            | Expr::NilLit(_)
            | Expr::Ident(_, _)
            | Expr::StateRef(_, _, _)
            | Expr::SystemRef(_, _)
            | Expr::Error(_) => {}
            Expr::ListLit(items, _) | Expr::TupleLit(items, _) => {
                for item in items {
                    self.scan_expr(item);
                }
            }
            Expr::MapLit(entries, _) => {
                for (key, value) in entries {
                    self.scan_expr(key);
                    self.scan_expr(value);
                }
            }
            Expr::FStringExpr(parts, _) => {
                for part in parts {
                    if let FStringPart::Expr(value, _) = part {
                        self.scan_expr(value);
                    }
                }
            }
            Expr::Binary(left, _, right, _) => {
                self.scan_expr(left);
                self.scan_expr(right);
            }
            Expr::Pipe(left, right, _) => {
                self.scan_expr(left);
                match right.as_ref() {
                    Expr::Call(callee, args, _) => {
                        self.scan_call(callee, args, false, Some(left));
                    }
                    Expr::AsyncCall(callee, args, _) => {
                        self.direct.async_effect = true;
                        self.scan_call(callee, args, true, Some(left));
                    }
                    callee => self.scan_call(callee, &[], false, Some(left)),
                }
            }
            Expr::Unary(_, value, _)
            | Expr::Field(value, _, _)
            | Expr::Await(value, _)
            | Expr::Try(value, _)
            | Expr::Spread(value, _) => {
                if matches!(expr, Expr::Await(_, _)) {
                    self.direct.async_effect = true;
                }
                self.scan_expr(value);
            }
            Expr::Call(callee, args, _) => self.scan_call(callee, args, false, None),
            Expr::AsyncCall(callee, args, _) => {
                self.direct.async_effect = true;
                self.scan_call(callee, args, true, None);
            }
            Expr::Index(owner, index, _) => {
                self.scan_expr(owner);
                self.scan_expr(index);
            }
            Expr::ComponentExpr(_, fields, spread, _) => {
                for (_, value) in fields {
                    self.scan_expr(value);
                }
                if let Some(value) = spread {
                    self.scan_expr(value);
                }
            }
            Expr::VariantExpr(_, _, fields, _) => {
                for (_, value) in fields {
                    self.scan_expr(value);
                }
            }
            Expr::MatchExpr(m, _) => {
                self.scan_expr(&m.subject);
                for case in &m.cases {
                    self.scan_match_case(case);
                }
            }
            Expr::IfExpr(condition, yes, no, _) => {
                self.scan_expr(condition);
                self.scan_expr(yes);
                self.scan_expr(no);
            }
            Expr::FnExpr(_, _, _, _, _, _, _) => {
                self.ensure_closure(expr);
            }
            Expr::QueryExpr(query, _) => {
                for (component, _) in &query.components {
                    self.direct
                        .reads
                        .insert(self.resolver.type_name(component, &self.seed.redirects));
                }
                if let Some(filter) = &query.filter {
                    self.scan_expr(filter);
                }
            }
            Expr::EntityLiteral(name, components, _) => {
                if let Some(name) = name {
                    self.scan_expr(name);
                    self.direct.writes.insert(ENTITY_NAMES.to_string());
                }
                self.direct.writes.insert(ENTITY_IDENTITY.to_string());
                self.record_spawn_entries(components);
            }
        }
    }

    fn scan_call(&mut self, callee: &Expr, args: &[Expr], is_async: bool, implicit: Option<&Expr>) {
        let targets = self.callable_targets(callee);
        for arg in args {
            self.scan_expr(arg);
        }
        let logical_args = implicit.into_iter().chain(args.iter()).collect::<Vec<_>>();
        if let Expr::Ident(name, _) = callee {
            if builtins::is_builtin(name) {
                self.scan_builtin(name, &logical_args);
                return;
            }
            if let Some(index) = self.seed.params.iter().position(|param| param == name) {
                let bound = self
                    .seed
                    .param_bounds
                    .get(&index)
                    .copied()
                    .unwrap_or(CallableBound::Unbounded);
                self.dynamic_params.insert(index, bound);
                return;
            }
            if let Some(bound) = self.local(name).and_then(|local| local.bound) {
                let _ = bound.apply(&mut self.direct);
                if targets.is_empty() {
                    return;
                }
            }
        }
        if !targets.is_empty() {
            let bounded_args = logical_args
                .iter()
                .map(|arg| self.callable_argument(arg))
                .collect::<Vec<_>>();
            for target in targets {
                self.calls.insert(target.clone());
                self.call_sites.push(CallSite {
                    caller: self.seed.name.clone(),
                    callee: target,
                    args: bounded_args.clone(),
                });
            }
        } else {
            self.scan_expr(callee);
            self.direct.unknown = true;
        }
        self.direct.async_effect |= is_async;
    }

    fn record_system_param_write(&mut self, target: &Expr) {
        let Some(root) = root_ident(target) else {
            return;
        };
        if self.seed.params.iter().any(|param| param == root) || self.local(root).is_some() {
            return;
        }
        if let Some((name, _)) = self.seed.system_params.get(root) {
            self.direct.writes.insert(name.clone());
            self.parameter_assignments.insert(name.clone());
        }
    }

    fn callable_targets(&mut self, expr: &Expr) -> BTreeSet<String> {
        match expr {
            Expr::FnExpr(_, _, _, _, _, _, _) => BTreeSet::from([self.ensure_closure(expr)]),
            Expr::Ident(name, _) => {
                if let Some(local) = self.local(name) {
                    return local.targets.clone();
                }
                self.resolver
                    .callable(expr, &self.seed.redirects)
                    .map(|target| BTreeSet::from([target]))
                    .unwrap_or_default()
            }
            _ => self
                .resolver
                .callable(expr, &self.seed.redirects)
                .map(|target| BTreeSet::from([target]))
                .unwrap_or_default(),
        }
    }

    fn callable_argument(&mut self, expr: &Expr) -> CallableArgument {
        if let Expr::Ident(name, _) = expr {
            if let Some(index) = self.seed.params.iter().position(|param| param == name) {
                let bound = self
                    .seed
                    .param_bounds
                    .get(&index)
                    .copied()
                    .unwrap_or(CallableBound::Unbounded);
                return CallableArgument {
                    targets: BTreeSet::new(),
                    forwarded_param: Some((index, bound)),
                    bound: None,
                };
            }
            if let Some(local) = self.local(name) {
                return CallableArgument {
                    targets: local.targets.clone(),
                    forwarded_param: None,
                    bound: local.bound,
                };
            }
        }
        CallableArgument {
            targets: self.callable_targets(expr),
            forwarded_param: None,
            bound: None,
        }
    }

    fn ensure_closure(&mut self, expr: &Expr) -> String {
        let Expr::FnExpr(params, _, param_types, _, _, body, span) = expr else {
            unreachable!("closure registration requires FnExpr")
        };
        let key = (span.file.map(|file| file.0), span.line, span.col);
        if let Some(name) = self.closure_names.get(&key) {
            return name.clone();
        }
        let file = span.file.map_or(0, |file| file.0);
        let name = format!(
            "{}::__closure_{}_{}_{}",
            self.seed.name, file, span.line, span.col
        );
        let display_name = format!(
            "{}::<closure {}:{}>",
            self.seed.display_name, span.line, span.col
        );
        self.closure_names.insert(key, name.clone());
        self.closures.push(Seed {
            name: name.clone(),
            display_name,
            kind: AuthorityCallableKind::Closure,
            block: body.clone(),
            span: span.clone(),
            redirects: self.seed.redirects.clone(),
            params: params.clone(),
            param_bounds: param_types
                .iter()
                .enumerate()
                .filter_map(|(index, annotation)| {
                    annotation
                        .as_ref()
                        .and_then(CallableBound::from_type)
                        .map(|bound| (index, bound))
                })
                .collect(),
            system_params: self.seed.system_params.clone(),
            authority_reads: BTreeSet::new(),
            authority_writes: BTreeSet::new(),
            authority_emits: BTreeSet::new(),
            authority_io: false,
            authority_async: false,
            declared: EffectDraft::default(),
            captured_locals: self.visible_locals(),
        });
        name
    }
}

fn root_ident(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(name, _) => Some(name),
        Expr::Field(owner, _, _) | Expr::Index(owner, _, _) => root_ident(owner),
        _ => None,
    }
}

fn pattern_binding_names(pattern: &Pattern) -> Vec<String> {
    match pattern {
        Pattern::Variant {
            pattern_bindings, ..
        } if !pattern_bindings.is_empty() => pattern_bindings
            .iter()
            .map(|binding| binding.name.clone())
            .collect(),
        Pattern::Variant { bindings, .. } => bindings.clone(),
        Pattern::HasComponent {
            binding: Some(binding),
            ..
        } => vec![binding.clone()],
        Pattern::Wildcard | Pattern::Literal(_) | Pattern::HasComponent { binding: None, .. } => {
            Vec::new()
        }
    }
}
