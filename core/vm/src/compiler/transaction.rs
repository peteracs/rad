use super::*;

impl Compiler {
    pub(crate) fn compile_transaction(
        &mut self,
        transaction: &TransactionStmt,
    ) -> Result<(), CompileError> {
        let line = transaction.span.line;
        let canonical_name = self.resolve_canonical_name(&transaction.name);
        let name = self.add_constant_gc(|gc| Value::from_string(gc, canonical_name));
        let changes = transaction
            .changes_only
            .iter()
            .map(|target| self.resolve_canonical_name(target))
            .collect::<std::collections::BTreeSet<_>>();
        self.emit_op(Op::BeginTransaction, line);
        self.emit_u16(name, line);
        self.emit_u16(changes.len() as u16, line);
        for target in changes {
            let target = self.add_constant_gc(|gc| Value::from_string(gc, target));
            self.emit_u16(target, line);
        }

        self.current().settlement_depth += 1;
        for (index, condition) in transaction.requires.iter().enumerate() {
            self.compile_expr(condition)?;
            let label = format!("requires #{}", index + 1);
            let label = self.add_constant_gc(|gc| Value::from_string(gc, label));
            self.emit_op(Op::CheckTransaction, condition.span().line);
            self.emit_byte(0, condition.span().line);
            self.emit_u16(label, condition.span().line);
        }

        self.begin_scope();
        let body_result = self.compile_body(&transaction.body.stmts);
        self.end_scope(line);
        body_result?;

        for (index, condition) in transaction.ensures.iter().enumerate() {
            self.compile_expr(condition)?;
            let label = format!("ensures #{}", index + 1);
            let label = self.add_constant_gc(|gc| Value::from_string(gc, label));
            self.emit_op(Op::CheckTransaction, condition.span().line);
            self.emit_byte(1, condition.span().line);
            self.emit_u16(label, condition.span().line);
        }
        self.current().settlement_depth -= 1;
        self.emit_op(Op::EndTransaction, line);
        self.emit_byte(u8::from(transaction.post_commit.is_some()), line);

        if let Some(post_commit) = &transaction.post_commit {
            self.begin_scope();
            let result = self.compile_body(&post_commit.stmts);
            self.end_scope(line);
            result?;
            self.emit_op(Op::EndPostCommit, line);
        }
        Ok(())
    }
}
