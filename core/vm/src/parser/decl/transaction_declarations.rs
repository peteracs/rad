impl Parser {
    fn parse_transaction_decl(&mut self) -> Result<FnDecl, ParseError> {
        let span = self.span();
        self.advance(); // soft keyword `transaction`
        let name = self.expect_ident_text()?;
        self.expect(TokenType::LParen)?;
        let mut params = Vec::new();
        let mut param_types = Vec::new();
        while !self.check(TokenType::RParen) {
            params.push(self.expect_ident_text()?);
            if self.check(TokenType::Colon) {
                self.advance();
                param_types.push(Some(self.parse_type()?));
            } else {
                param_types.push(None);
            }
            if self.check(TokenType::Comma) {
                self.advance();
            } else if !self.check(TokenType::RParen) {
                return Err(ParseError {
                    message: "Expected `,` or `)` in transaction parameter list".to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
        }
        self.expect(TokenType::RParen)?;
        let ownership_writes = self.parse_optional_ownership_writes()?;

        let transaction_span = self.span();
        self.expect(TokenType::LBrace)?;
        let mut changes_only = None;
        let mut requires = Vec::new();
        let mut ensures = Vec::new();
        let mut body_stmts = Vec::new();
        let mut post_commit = None;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            if self.check_ident_text("requires") {
                self.advance();
                requires.push(self.parse_expr()?);
                continue;
            }
            if self.check_ident_text("ensures") {
                self.advance();
                ensures.push(self.parse_expr()?);
                continue;
            }
            if self.check_ident_text("changes_only") {
                if changes_only.is_some() {
                    return Err(ParseError {
                        message: "A transaction must declare `changes_only` exactly once"
                            .to_string(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                self.advance();
                self.expect(TokenType::LBracket)?;
                let mut targets = Vec::new();
                while !self.check(TokenType::RBracket) {
                    targets.push(self.parse_ownership_target()?);
                    if self.check(TokenType::Comma) {
                        self.advance();
                    } else if !self.check(TokenType::RBracket) {
                        return Err(ParseError {
                            message: "Expected `,` or `]` in `changes_only`".to_string(),
                            line: self.peek().line,
                            col: self.peek().col,
                        });
                    }
                }
                self.expect(TokenType::RBracket)?;
                if targets.is_empty() {
                    return Err(ParseError {
                        message: "`changes_only` requires at least one authority".to_string(),
                        line: transaction_span.line,
                        col: transaction_span.col,
                    });
                }
                if targets.len() > u16::MAX as usize {
                    return Err(ParseError {
                        message: format!(
                            "`changes_only` supports at most {} authorities",
                            u16::MAX
                        ),
                        line: transaction_span.line,
                        col: transaction_span.col,
                    });
                }
                changes_only = Some(targets);
                continue;
            }
            if self.check_ident_text("post_commit") {
                if post_commit.is_some() {
                    return Err(ParseError {
                        message: "A transaction may declare only one `post_commit` block"
                            .to_string(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                self.advance();
                post_commit = Some(self.parse_block()?);
                if !self.check(TokenType::RBrace) {
                    return Err(ParseError {
                        message: "`post_commit` must be the final transaction section".to_string(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                continue;
            }
            body_stmts.push(self.parse_statement()?);
        }
        self.expect(TokenType::RBrace)?;

        if requires.is_empty() {
            return Err(ParseError {
                message: "A transaction requires at least one `requires` contract".to_string(),
                line: span.line,
                col: span.col,
            });
        }
        if ensures.is_empty() {
            return Err(ParseError {
                message: "A transaction requires at least one `ensures` contract".to_string(),
                line: span.line,
                col: span.col,
            });
        }
        let body_span = transaction_span.clone();
        let transaction = Stmt::Transaction(TransactionStmt {
            id: self.next_id(),
            span: transaction_span,
            name: name.clone(),
            changes_only: changes_only.ok_or_else(|| ParseError {
                message: "A transaction requires one `changes_only [...]` contract".to_string(),
                line: span.line,
                col: span.col,
            })?,
            requires,
            body: Block {
                id: self.next_id(),
                span: body_span.clone(),
                stmts: body_stmts,
            },
            ensures,
            post_commit,
        });
        self.ensure_fn_param_alignment(&span, params.len(), param_types.len())?;
        Ok(FnDecl {
            id: self.next_id(),
            span,
            name,
            is_pub: false,
            type_params: Vec::new(),
            param_muts: vec![false; params.len()],
            params,
            param_types,
            return_type: Some(TypeExpr::Named("nil".to_string())),
            body: Block {
                id: self.next_id(),
                span: body_span,
                stmts: vec![transaction],
            },
            is_pure: false,
            is_async: false,
            effects: Vec::new(),
            ownership_writes,
        })
    }
}
