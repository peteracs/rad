use super::*;

impl Parser {
    pub(super) fn parse_intent_decl(&mut self) -> Result<IntentDecl, ParseError> {
        let span = self.span();
        self.advance(); // soft keyword `intent`
        let name = self.expect_ident_text()?;
        self.expect(TokenType::LBrace)?;
        let mut fields = Vec::new();
        while !self.check(TokenType::RBrace) {
            let field_span = self.span();
            let is_key = self.check_ident_text("key");
            if is_key {
                self.advance();
            }
            let field_name = self.expect_field_name()?;
            self.expect(TokenType::Colon)?;
            let type_annotation = self.parse_type()?;
            fields.push(IntentField {
                span: field_span,
                name: field_name,
                type_annotation,
                is_key,
            });
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RBrace)?;
        Ok(IntentDecl {
            id: self.next_id(),
            span,
            name,
            is_pub: false,
            fields,
        })
    }

    pub(super) fn parse_law_decl(&mut self) -> Result<LawDecl, ParseError> {
        let span = self.span();
        self.advance(); // soft keyword `law`
        let name = self.expect_ident_text()?;
        self.expect(TokenType::LParen)?;
        let mut params = Vec::new();
        let mut param_types = Vec::new();
        while !self.check(TokenType::RParen) {
            params.push(self.expect_ident_text()?);
            self.expect(TokenType::Colon)?;
            param_types.push(self.parse_type()?);
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RParen)?;
        let body = self.parse_block()?;
        Ok(LawDecl {
            id: self.next_id(),
            span,
            name,
            is_pub: false,
            params,
            param_types,
            body,
        })
    }

    pub(super) fn parse_resolver_decl(&mut self) -> Result<ResolverDecl, ParseError> {
        let span = self.span();
        self.advance(); // soft keyword `resolver`
        let name = self.expect_ident_text()?;
        if !self.check(TokenType::For) {
            return Err(ParseError {
                message: "Expected `for` after resolver name".to_string(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }
        self.advance();
        let intent_name = self.expect_ident_text()?;
        self.expect(TokenType::LParen)?;
        let key_param = self.expect_ident_text()?;
        self.expect(TokenType::Comma)?;
        let proposals_param = self.expect_ident_text()?;
        self.expect(TokenType::RParen)?;
        let body = self.parse_block()?;
        Ok(ResolverDecl {
            id: self.next_id(),
            span,
            name,
            is_pub: false,
            intent_name,
            key_param,
            proposals_param,
            body,
        })
    }

    pub(super) fn parse_constraint_decl(&mut self) -> Result<ConstraintDecl, ParseError> {
        let span = self.span();
        self.advance(); // soft keyword `constraint`
        let name = self.expect_ident_text()?;
        self.expect(TokenType::For)?;
        let component_name = self.expect_ident_text()?;
        self.expect(TokenType::LParen)?;
        let subject_param = self.expect_ident_text()?;
        self.expect(TokenType::Comma)?;
        let proposed_param = self.expect_ident_text()?;
        self.expect(TokenType::RParen)?;
        let mut watches = Vec::new();
        if self.check_ident_text("watches") {
            self.advance();
            loop {
                watches.push(self.expect_ident_text()?);
                if !self.check(TokenType::Comma) {
                    break;
                }
                self.advance();
            }
        }
        let body = self.parse_block()?;
        Ok(ConstraintDecl {
            id: self.next_id(),
            span,
            name,
            is_pub: false,
            component_name,
            subject_param,
            proposed_param,
            watches,
            body,
        })
    }

    pub(super) fn parse_settle(&mut self) -> Result<Stmt, ParseError> {
        let span = self.span();
        self.advance(); // soft keyword `settle`
        let body = self.parse_block()?;
        Ok(Stmt::Settle(SettleStmt {
            id: self.next_id(),
            span,
            body,
        }))
    }

    pub(super) fn parse_propose(&mut self) -> Result<Stmt, ParseError> {
        let span = self.span();
        self.advance(); // soft keyword `propose`
        let intent_name = self.expect_ident_text()?;
        let fields = self.parse_causal_fields()?;
        Ok(Stmt::Propose(ProposeStmt {
            id: self.next_id(),
            span,
            intent_name,
            fields,
        }))
    }

    pub(super) fn parse_next(&mut self) -> Result<Stmt, ParseError> {
        let span = self.span();
        self.advance(); // soft keyword `next`
        self.expect(TokenType::LParen)?;
        let entity = self.parse_expr()?;
        self.expect(TokenType::Comma)?;
        let component_name = self.expect_ident_text()?;
        let fields = self.parse_causal_fields()?;
        self.expect(TokenType::RParen)?;
        Ok(Stmt::Next(NextStmt {
            id: self.next_id(),
            span,
            entity,
            component_name,
            fields,
        }))
    }

    pub(super) fn parse_require(&mut self) -> Result<Stmt, ParseError> {
        let span = self.span();
        self.advance(); // soft keyword `require`
        let condition = self.parse_expr()?;
        self.expect(TokenType::Else)?;
        let code = self.expect_string_text()?;
        Ok(Stmt::Require(RequireStmt {
            id: self.next_id(),
            span,
            condition,
            code,
        }))
    }

    fn parse_causal_fields(&mut self) -> Result<Vec<(String, Expr)>, ParseError> {
        self.expect(TokenType::LBrace)?;
        let mut fields = Vec::new();
        while !self.check(TokenType::RBrace) {
            let name = self.expect_field_name()?;
            self.expect(TokenType::Colon)?;
            fields.push((name, self.parse_expr()?));
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RBrace)?;
        Ok(fields)
    }
}
