use super::*;

impl Compiler {
    pub(crate) fn intent_runtime_type(name: &str) -> String {
        name.to_string()
    }

    pub(crate) fn compile_intent_decl(&mut self, decl: &IntentDecl) -> Result<(), CompileError> {
        self.require_causal_feature(&decl.span)?;
        let name = self.resolve_canonical_name(&decl.name);
        let key = decl
            .fields
            .iter()
            .find(|field| field.is_key)
            .map(|field| field.name.clone())
            .unwrap_or_default();
        self.intent_types.insert(
            name,
            (
                key,
                decl.fields.iter().map(|field| field.name.clone()).collect(),
            ),
        );
        Ok(())
    }

    pub(crate) fn compile_law_decl(&mut self, decl: &LawDecl) -> Result<(), CompileError> {
        self.require_causal_feature(&decl.span)?;
        let name = self.resolve_canonical_name(&decl.name);
        self.compile_fn_decl(&FnDecl {
            id: decl.id,
            span: decl.span.clone(),
            name,
            is_pub: decl.is_pub,
            type_params: Vec::new(),
            params: decl.params.clone(),
            param_muts: vec![false; decl.params.len()],
            param_types: decl.param_types.iter().cloned().map(Some).collect(),
            return_type: None,
            body: decl.body.clone(),
            is_pure: false,
            is_async: false,
            effects: vec!["readonly".to_string()],
        })
    }

    pub(crate) fn compile_resolver_decl(
        &mut self,
        decl: &ResolverDecl,
    ) -> Result<(), CompileError> {
        self.require_causal_feature(&decl.span)?;
        let resolver_name = self.resolve_canonical_name(&decl.name);
        let intent_name = self.resolve_canonical_name(&decl.intent_name);
        let hidden_name = format!("__resolver__{}", resolver_name);
        self.compile_fn_decl(&FnDecl {
            id: decl.id,
            span: decl.span.clone(),
            name: hidden_name.clone(),
            is_pub: false,
            type_params: Vec::new(),
            params: vec![decl.key_param.clone(), decl.proposals_param.clone()],
            param_muts: vec![false, false],
            param_types: vec![None, None],
            return_type: None,
            body: decl.body.clone(),
            is_pure: false,
            is_async: false,
            effects: vec!["readonly".to_string()],
        })?;
        let global_slot = self.ensure_global_slot(&hidden_name);
        self.resolvers.push(ResolverChunkInfo {
            name: resolver_name,
            intent: intent_name,
            global_slot,
        });
        Ok(())
    }

    pub(crate) fn compile_settle(&mut self, stmt: &SettleStmt) -> Result<(), CompileError> {
        self.require_causal_feature(&stmt.span)?;
        self.emit_op(Op::BeginSettlement, stmt.span.line);
        self.compile_body(&stmt.body.stmts)?;
        self.emit_op(Op::EndSettlement, stmt.span.line);
        Ok(())
    }

    pub(crate) fn compile_propose(&mut self, stmt: &ProposeStmt) -> Result<(), CompileError> {
        self.require_causal_feature(&stmt.span)?;
        let intent_name = self.resolve_canonical_name(&stmt.intent_name);
        let runtime_type = Self::intent_runtime_type(&intent_name);
        let type_idx = self.add_constant_gc(|gc| Value::from_string(gc, runtime_type));
        for (field, expr) in &stmt.fields {
            self.emit_constant_gc(stmt.span.line, |gc| Value::from_string(gc, field.clone()));
            self.compile_expr(expr)?;
        }
        self.emit_op(Op::MakeComp, stmt.span.line);
        self.emit_u16(type_idx, stmt.span.line);
        self.emit_u16(stmt.fields.len() as u16, stmt.span.line);
        let intent_idx = self.add_constant_gc(|gc| Value::from_string(gc, intent_name));
        self.emit_op(Op::ProposeIntent, stmt.span.line);
        self.emit_u16(intent_idx, stmt.span.line);
        Ok(())
    }

    pub(crate) fn compile_next(&mut self, stmt: &NextStmt) -> Result<(), CompileError> {
        self.require_causal_feature(&stmt.span)?;
        self.compile_expr(&stmt.entity)?;
        self.compile_expr(&Expr::ComponentExpr(
            stmt.component_name.clone(),
            stmt.fields.clone(),
            None,
            stmt.span.clone(),
        ))?;
        self.emit_op(Op::StageCandidate, stmt.span.line);
        Ok(())
    }

    fn require_causal_feature(&self, span: &Span) -> Result<(), CompileError> {
        if self
            .features
            .iter()
            .any(|feature| feature == "causal_laws" || feature == "experimental-laws")
        {
            Ok(())
        } else {
            Err(CompileError {
                message: "RAD Causal Laws is experimental; pass `--experimental-laws`".to_string(),
                line: span.line,
                col: span.col,
            })
        }
    }
}
