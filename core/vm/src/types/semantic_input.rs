use std::collections::HashMap;

use crate::ast::*;

/// Canonicalize checker input before provenance hashing. Source coordinates,
/// import spelling, and module alias spelling do not change semantics; every
/// name that can cross a module boundary is resolved to the same namespace the
/// checker and compiler use.
pub(super) fn normalize_program(program: &mut Program, aliases: &HashMap<String, ModuleAlias>) {
    normalize_declarations(&mut program.declarations, aliases);
}

pub(super) fn normalize_declarations(
    declarations: &mut [Decl],
    aliases: &HashMap<String, ModuleAlias>,
) {
    let normalizer = SemanticInputNormalizer { aliases };
    for declaration in declarations {
        normalizer.declaration(declaration);
    }
}

struct SemanticInputNormalizer<'a> {
    aliases: &'a HashMap<String, ModuleAlias>,
}

impl SemanticInputNormalizer<'_> {
    fn span(span: &mut Span) {
        span.line = 0;
        span.col = 0;
        span.file = None;
    }

    fn canonical_member(&self, alias: &str, member: &str) -> Option<String> {
        let binding = self.aliases.get(alias)?;
        Some(binding.canonical_symbol(member))
    }

    fn module_marker(&self, alias: &str) -> Option<String> {
        self.aliases
            .get(alias)
            .map(|binding| format!("@module:{}", binding.module_identity()))
    }

    fn qualified_name(&self, name: &mut String) {
        let Some((alias, member)) = name.split_once('.') else {
            return;
        };
        if let Some(canonical) = self.canonical_member(alias, member) {
            *name = canonical;
        }
    }

    fn qualified_names(&self, names: &mut [String]) {
        for name in names {
            self.qualified_name(name);
        }
    }

    fn module_alias(&self, alias: &mut String) {
        if let Some(marker) = self.module_marker(alias) {
            *alias = marker;
        }
    }

    fn ownership(&self, ownership: &mut OwnershipDecl) {
        for coowner in &mut ownership.coowner_modules {
            self.module_alias(coowner);
        }
        if let Some(transferred_to) = &mut ownership.transferred_to {
            self.module_alias(transferred_to);
        }
    }

    fn fields(&self, fields: &mut [FieldDef]) {
        for field in fields {
            if let Some(annotation) = &mut field.type_annotation {
                self.type_expr(annotation);
            }
            self.expr(&mut field.default_value);
        }
    }

    fn declaration(&self, declaration: &mut Decl) {
        match declaration {
            Decl::Component(data) | Decl::Struct(data) => {
                Self::span(&mut data.span);
                self.fields(&mut data.fields);
                self.ownership(&mut data.ownership);
            }
            Decl::Resource(resource) => {
                Self::span(&mut resource.span);
                self.fields(&mut resource.fields);
                self.ownership(&mut resource.ownership);
            }
            Decl::Intent(intent) => {
                Self::span(&mut intent.span);
                for field in &mut intent.fields {
                    Self::span(&mut field.span);
                    self.type_expr(&mut field.type_annotation);
                }
            }
            Decl::Law(law) => {
                Self::span(&mut law.span);
                for annotation in &mut law.param_types {
                    self.type_expr(annotation);
                }
                self.block(&mut law.body);
            }
            Decl::Resolver(resolver) => {
                Self::span(&mut resolver.span);
                self.qualified_name(&mut resolver.intent_name);
                self.block(&mut resolver.body);
            }
            Decl::Constraint(constraint) => {
                Self::span(&mut constraint.span);
                self.qualified_name(&mut constraint.component_name);
                self.qualified_names(&mut constraint.watches);
                self.block(&mut constraint.body);
            }
            Decl::Entity(entity) => {
                Self::span(&mut entity.span);
                for component in &mut entity.components {
                    self.component_entry(component);
                }
            }
            Decl::State(state) => {
                Self::span(&mut state.span);
                for state_definition in &mut state.states {
                    Self::span(&mut state_definition.span);
                    for (_, event, guard) in &mut state_definition.transitions {
                        self.qualified_name(event);
                        if let Some(guard) = guard {
                            self.expr(guard);
                        }
                    }
                }
            }
            Decl::System(system) => {
                Self::span(&mut system.span);
                for (_, _, component) in &mut system.params {
                    self.qualified_name(component);
                }
                self.qualified_names(&mut system.authority_reads);
                self.qualified_names(&mut system.authority_writes);
                self.qualified_names(&mut system.authority_emits);
                self.qualified_names(&mut system.ownership_writes);
                self.qualified_names(&mut system.after);
                self.qualified_names(&mut system.before);
                self.block(&mut system.body);
            }
            Decl::Event(event) => {
                Self::span(&mut event.span);
                for (_, annotation) in &mut event.fields {
                    if let Some(annotation) = annotation {
                        self.type_expr(annotation);
                    }
                }
            }
            Decl::OnHandler(handler) => {
                Self::span(&mut handler.span);
                self.qualified_name(&mut handler.event_name);
                self.qualified_names(&mut handler.ownership_writes);
                self.block(&mut handler.body);
            }
            Decl::Migration(migration) => {
                Self::span(&mut migration.span);
                self.qualified_name(&mut migration.component);
                self.block(&mut migration.body);
            }
            Decl::Phase(phase) => {
                Self::span(&mut phase.span);
                self.qualified_names(&mut phase.systems);
            }
            Decl::Fn(function) => {
                Self::span(&mut function.span);
                for annotation in function.param_types.iter_mut().flatten() {
                    self.type_expr(annotation);
                }
                if let Some(annotation) = &mut function.return_type {
                    self.type_expr(annotation);
                }
                self.qualified_names(&mut function.ownership_writes);
                self.block(&mut function.body);
            }
            Decl::Type(sum_type) => {
                Self::span(&mut sum_type.span);
                for variant in &mut sum_type.variants {
                    for (_, value) in &mut variant.fields {
                        self.expr(value);
                    }
                    for (_, annotation) in &mut variant.annotations {
                        self.type_expr(annotation);
                    }
                }
            }
            Decl::TypeAlias(alias) => {
                Self::span(&mut alias.span);
                self.type_expr(&mut alias.target);
            }
            Decl::NativeType(native) => Self::span(&mut native.span),
            Decl::MaterializedView(view) => {
                Self::span(&mut view.span);
                self.qualified_names(&mut view.dependencies);
                if let Some((component, _)) = &mut view.key {
                    self.qualified_name(component);
                }
                for clause in &mut view.predicate.clauses {
                    self.qualified_name(&mut clause.component);
                }
            }
            Decl::Use(import) => {
                Self::span(&mut import.span);
                import.path = import
                    .module_identity
                    .clone()
                    .unwrap_or_else(|| import.path.replace('\\', "/"));
                import.alias = None;
            }
            Decl::Test(test) => {
                Self::span(&mut test.span);
                self.qualified_names(&mut test.ownership_writes);
                for (_, generator) in &mut test.generators {
                    self.expr(generator);
                }
                self.block(&mut test.body);
            }
            Decl::Model(model) => {
                Self::span(&mut model.span);
                self.qualified_names(&mut model.ownership_writes);
                for command in &mut model.commands {
                    self.expr(command);
                }
                for invariant in &mut model.invariants {
                    self.block(invariant);
                }
            }
            Decl::Stmt(statement) => self.statement(statement),
            Decl::Error => {}
        }
    }

    fn block(&self, block: &mut Block) {
        Self::span(&mut block.span);
        for statement in &mut block.stmts {
            self.statement(statement);
        }
    }

    fn statement(&self, statement: &mut Stmt) {
        match statement {
            Stmt::Let(binding) => {
                Self::span(&mut binding.span);
                if let Some(annotation) = &mut binding.type_annotation {
                    self.type_expr(annotation);
                }
                self.expr(&mut binding.value);
            }
            Stmt::LetElse(binding) => {
                Self::span(&mut binding.span);
                self.qualified_name(&mut binding.variant_name);
                if let Some(annotation) = &mut binding.type_annotation {
                    self.type_expr(annotation);
                }
                self.expr(&mut binding.subject);
                self.block(&mut binding.else_block);
            }
            Stmt::Assign(assignment) => {
                Self::span(&mut assignment.span);
                self.expr(&mut assignment.target);
                self.expr(&mut assignment.value);
            }
            Stmt::If(branch) => {
                Self::span(&mut branch.span);
                self.expr(&mut branch.condition);
                self.block(&mut branch.then_block);
                if let Some(otherwise) = &mut branch.else_block {
                    self.block(otherwise);
                }
            }
            Stmt::While(loop_statement) => {
                Self::span(&mut loop_statement.span);
                self.expr(&mut loop_statement.condition);
                self.block(&mut loop_statement.body);
            }
            Stmt::For(loop_statement) => {
                Self::span(&mut loop_statement.span);
                self.expr(&mut loop_statement.iterable);
                self.block(&mut loop_statement.body);
            }
            Stmt::Return(return_statement) => {
                Self::span(&mut return_statement.span);
                if let Some(value) = &mut return_statement.value {
                    self.expr(value);
                }
            }
            Stmt::Break(statement) => Self::span(&mut statement.span),
            Stmt::Continue(statement) => Self::span(&mut statement.span),
            Stmt::Emit(emit) => {
                Self::span(&mut emit.span);
                self.qualified_name(&mut emit.event_name);
                for (_, value) in &mut emit.fields {
                    self.expr(value);
                }
                if let Some(delay) = &mut emit.delay {
                    self.expr(delay);
                }
            }
            Stmt::Schedule(schedule) => {
                Self::span(&mut schedule.span);
                self.qualified_names(&mut schedule.systems);
            }
            Stmt::Update(update) => {
                Self::span(&mut update.span);
                self.qualified_name(&mut update.comp_name);
                if let Some(entity) = &mut update.entity_expr {
                    self.expr(entity);
                }
                for field in &mut update.field_updates {
                    if let Some(index) = &mut field.index {
                        self.expr(index);
                    }
                    self.expr(&mut field.value);
                }
            }
            Stmt::Transaction(transaction) => {
                Self::span(&mut transaction.span);
                self.qualified_names(&mut transaction.changes_only);
                for condition in &mut transaction.requires {
                    self.expr(condition);
                }
                self.block(&mut transaction.body);
                for condition in &mut transaction.ensures {
                    self.expr(condition);
                }
                if let Some(post_commit) = &mut transaction.post_commit {
                    self.block(post_commit);
                }
            }
            Stmt::Settle(settlement) => {
                Self::span(&mut settlement.span);
                self.block(&mut settlement.body);
            }
            Stmt::Propose(proposal) => {
                Self::span(&mut proposal.span);
                self.qualified_name(&mut proposal.intent_name);
                for (_, value) in &mut proposal.fields {
                    self.expr(value);
                }
            }
            Stmt::Next(next) => {
                Self::span(&mut next.span);
                self.qualified_name(&mut next.component_name);
                self.expr(&mut next.entity);
                for (_, value) in &mut next.fields {
                    self.expr(value);
                }
            }
            Stmt::Require(requirement) => {
                Self::span(&mut requirement.span);
                self.expr(&mut requirement.condition);
            }
            Stmt::Match(match_statement) => self.match_statement(match_statement),
            Stmt::Expr(expression) => {
                Self::span(&mut expression.span);
                self.expr(&mut expression.expr);
            }
            Stmt::OnceGuardPass(span) | Stmt::Error(span) => Self::span(span),
        }
    }

    fn match_statement(&self, match_statement: &mut MatchStmt) {
        Self::span(&mut match_statement.span);
        self.expr(&mut match_statement.subject);
        for case in &mut match_statement.cases {
            Self::span(&mut case.span);
            self.pattern(&mut case.pattern);
            if let Some(guard) = &mut case.guard {
                self.expr(guard);
            }
            self.block(&mut case.body);
        }
    }

    fn pattern(&self, pattern: &mut Pattern) {
        match pattern {
            Pattern::Wildcard => {}
            Pattern::Literal(value) => self.expr(value),
            Pattern::Variant { path, .. } => self.path(path),
            Pattern::HasComponent { component, .. } => self.qualified_name(component),
        }
    }

    fn path(&self, path: &mut Vec<String>) {
        if path.len() < 2 {
            return;
        }
        let alias = path[0].clone();
        if let Some(canonical) = self.canonical_member(&alias, &path[1]) {
            path.splice(0..2, [canonical]);
        }
    }

    fn component_entry(&self, component: &mut ComponentEntry) {
        match component {
            ComponentEntry::Init(initializer) => {
                Self::span(&mut initializer.span);
                self.qualified_name(&mut initializer.comp_name);
                for (_, value) in &mut initializer.fields {
                    self.expr(value);
                }
            }
            ComponentEntry::Expr(expression) => self.expr(expression),
        }
    }

    fn expr(&self, expression: &mut Expr) {
        if let Expr::Field(inner, member, _) = expression {
            if let Expr::Ident(alias, _) = inner.as_ref() {
                if let Some(canonical) = self.canonical_member(alias, member) {
                    *expression = Expr::Ident(
                        canonical,
                        Span {
                            line: 0,
                            col: 0,
                            file: None,
                        },
                    );
                    return;
                }
            }
        }

        match expression {
            Expr::IntLit(_, span)
            | Expr::FloatLit(_, span)
            | Expr::StrLit(_, span)
            | Expr::BoolLit(_, span)
            | Expr::NilLit(span)
            | Expr::Error(span) => Self::span(span),
            Expr::Ident(name, span) => {
                Self::span(span);
                if let Some(marker) = self.module_marker(name) {
                    *name = marker;
                }
            }
            Expr::ListLit(items, span) | Expr::TupleLit(items, span) => {
                Self::span(span);
                for item in items {
                    self.expr(item);
                }
            }
            Expr::MapLit(entries, span) => {
                Self::span(span);
                for (key, value) in entries {
                    self.expr(key);
                    self.expr(value);
                }
            }
            Expr::FStringExpr(parts, span) => {
                Self::span(span);
                for part in parts {
                    if let FStringPart::Expr(value, _) = part {
                        self.expr(value);
                    }
                }
            }
            Expr::Binary(left, _, right, span)
            | Expr::Pipe(left, right, span)
            | Expr::Index(left, right, span) => {
                Self::span(span);
                self.expr(left);
                self.expr(right);
            }
            Expr::Unary(_, inner, span)
            | Expr::Await(inner, span)
            | Expr::Try(inner, span)
            | Expr::Spread(inner, span) => {
                Self::span(span);
                self.expr(inner);
            }
            Expr::Call(callee, arguments, span) | Expr::AsyncCall(callee, arguments, span) => {
                Self::span(span);
                self.expr(callee);
                for argument in arguments {
                    self.expr(argument);
                }
            }
            Expr::Field(inner, _, span) => {
                Self::span(span);
                self.expr(inner);
            }
            Expr::ComponentExpr(component, fields, spread, span) => {
                Self::span(span);
                self.qualified_name(component);
                for (_, value) in fields {
                    self.expr(value);
                }
                if let Some(spread) = spread {
                    self.expr(spread);
                }
            }
            Expr::StateRef(state, _, span) => {
                Self::span(span);
                self.qualified_name(state);
            }
            Expr::SystemRef(path, span) => {
                Self::span(span);
                self.path(path);
            }
            Expr::VariantExpr(sum_type, _, fields, span) => {
                Self::span(span);
                self.qualified_name(sum_type);
                for (_, value) in fields {
                    self.expr(value);
                }
            }
            Expr::MatchExpr(match_statement, span) => {
                Self::span(span);
                self.match_statement(match_statement);
            }
            Expr::IfExpr(condition, then_value, else_value, span) => {
                Self::span(span);
                self.expr(condition);
                self.expr(then_value);
                self.expr(else_value);
            }
            Expr::FnExpr(_, _, parameter_types, capabilities, return_type, body, span) => {
                Self::span(span);
                for annotation in parameter_types.iter_mut().flatten() {
                    self.type_expr(annotation);
                }
                for capability_set in capabilities.iter_mut().flatten() {
                    self.qualified_names(capability_set);
                }
                if let Some(annotation) = return_type {
                    self.type_expr(annotation);
                }
                self.block(body);
            }
            Expr::QueryExpr(query, span) => {
                Self::span(span);
                for (component, _) in &mut query.components {
                    self.qualified_name(component);
                }
                if let Some(filter) = &mut query.filter {
                    self.expr(filter);
                }
            }
            Expr::EntityLiteral(name, components, span) => {
                Self::span(span);
                if let Some(name) = name {
                    self.expr(name);
                }
                for component in components {
                    self.component_entry(component);
                }
            }
        }
    }

    fn type_expr(&self, annotation: &mut TypeExpr) {
        match annotation {
            TypeExpr::Named(name) => self.qualified_name(name),
            TypeExpr::Generic(name, arguments) => {
                self.qualified_name(name);
                for argument in arguments {
                    self.type_expr(argument);
                }
            }
            TypeExpr::Tuple(items) | TypeExpr::Union(items) => {
                for item in items {
                    self.type_expr(item);
                }
            }
            TypeExpr::FnType(parameters, return_type, _) => {
                for parameter in parameters {
                    self.type_expr(parameter);
                }
                self.type_expr(return_type);
            }
        }
    }
}
