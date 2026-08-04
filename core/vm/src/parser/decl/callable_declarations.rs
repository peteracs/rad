impl Parser {
fn parse_on_handler(&mut self) -> Result<OnHandler, ParseError> {
        let span = self.span();
        let is_async = if self.check(TokenType::Async) {
            self.advance();
            true
        } else {
            false
        };
        self.expect(TokenType::On)?;
        let mut event_name = self.expect_ident_text()?;
        if self.check(TokenType::Dot) {
            self.advance();
            event_name.push('.');
            event_name.push_str(&self.expect_ident_text()?);
        }
        let once = if self.check(TokenType::Once) {
            self.advance();
            true
        } else {
            false
        };
        self.expect(TokenType::LParen)?;
        let param_name = self.expect_ident_text()?;
        self.expect(TokenType::RParen)?;
        let guard = if self.check(TokenType::When)
            || (self.check(TokenType::Ident) && self.peek().value.as_str() == Some("where"))
        {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        let has_guard = guard.is_some();
        let mut body = self.parse_block()?;
        if let Some(cond) = guard {
            let cond_span = cond.span().clone();
            let mut then_stmts = Vec::new();
            if once {
                then_stmts.push(Stmt::OnceGuardPass(cond_span.clone()));
            }
            then_stmts.extend(body.stmts);
            let then_block = Block {
                id: body.id,
                span: body.span,
                stmts: then_stmts,
            };
            let wrapped_if = Stmt::If(IfStmt {
                id: self.next_id(),
                span: cond_span.clone(),
                condition: cond,
                then_block,
                else_block: None,
            });
            body = Block {
                id: self.next_id(),
                span: cond_span,
                stmts: vec![wrapped_if],
            };
        }
        Ok(OnHandler {
            id: self.next_id(),
            span,
            event_name,
            param_name,
            body,
            once,
            is_async,
            has_guard,
        })
    }

    fn parse_fn_decl(&mut self) -> Result<FnDecl, ParseError> {
        let span = self.span();
        let is_async = if self.check(TokenType::Async) {
            self.advance();
            true
        } else {
            false
        };
        self.expect(TokenType::Fn)?;
        let name = self.expect_ident_text()?;
        let type_params = self.parse_type_params()?;
        self.expect(TokenType::LParen)?;
        let mut params = Vec::new();
        let mut param_muts = Vec::new();
        let mut param_types = Vec::new();
        while !self.check(TokenType::RParen) {
            let is_mut = self.check(TokenType::Mut);
            if is_mut {
                self.advance();
            }
            param_muts.push(is_mut);
            params.push(self.expect_ident_text()?);
            if self.check(TokenType::Colon) {
                self.advance();
                let ty = self.parse_type()?;
                param_types.push(Some(ty));
            } else {
                param_types.push(None);
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RParen)?;
        let return_type = if self.check(TokenType::Arrow) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        self.ensure_fn_param_alignment(&span, params.len(), param_types.len())?;
        Ok(FnDecl {
            id: self.next_id(),
            span,
            name,
            type_params,
            params,
            param_muts,
            param_types,
            return_type,
            body,
            is_pure: false,
            is_pub: false,
            is_async,
            effects: vec![],
        })
    }

    fn parse_type_params(&mut self) -> Result<Vec<String>, ParseError> {
        let mut type_params = Vec::new();
        if self.check(TokenType::Lt) {
            self.advance();
            while !self.check(TokenType::Gt) {
                type_params.push(self.expect_ident_text()?);
                if self.check(TokenType::Comma) {
                    self.advance();
                }
            }
            self.expect(TokenType::Gt)?;
        }
        Ok(type_params)
    }

    fn parse_test_decl(&mut self) -> Result<TestDecl, ParseError> {
        let span = self.span();
        self.expect(TokenType::Ident)?; // consume "test"

        let name = if self.check(TokenType::String) {
            let tok = self.peek().clone();
            self.advance();
            self.token_str_value(&tok, "test name")?.to_string()
        } else {
            self.expect_ident_text()?
        };

        // `for` lexes as a keyword token, never as an Ident: matching it as
        // an Ident made the documented property form unparseable
        // ("Expected LBrace, got For").
        let is_property = self.check(TokenType::For);

        let mut generators = Vec::new();
        if is_property {
            self.advance(); // consume "for"
            loop {
                let gname = self.expect_ident_text()?;
                self.expect(TokenType::In)?;
                let gexpr = self.parse_expr()?;
                generators.push((gname, gexpr));
                if self.check(TokenType::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        let mut body = self.parse_block()?;

        // Desugar the property form here, like `for … where` does: the body
        // runs once per generated value, i.e. wrapped in one loop per
        // generator (first generator outermost, cartesian product). The
        // checker then types each variable as the generator's ELEMENT type
        // and the compiler emits ordinary loops — no downstream special
        // case, so `generators` is handed on empty.
        for (gname, gexpr) in generators.into_iter().rev() {
            let loop_span = body.span.clone();
            body = Block {
                id: self.next_id(),
                span: loop_span.clone(),
                stmts: vec![Stmt::For(ForStmt {
                    id: self.next_id(),
                    span: loop_span,
                    bindings: vec![gname],
                    destructure_bindings: None,
                    iterable: gexpr,
                    body,
                })],
            };
        }

        Ok(TestDecl {
            id: self.next_id(),
            span,
            name,
            body,
            is_property,
            generators: Vec::new(),
        })
    }

fn parse_pure_fn(&mut self) -> Result<FnDecl, ParseError> {
        self.expect(TokenType::Pure)?;
        let mut decl = self.parse_fn_decl()?;
        decl.is_pure = true;
        Ok(decl)
    }

    fn effect_name_at(&self, offset: usize) -> Option<&'static str> {
        let tok = self.peek_at(offset);
        match tok.ty {
            TokenType::Event => Some("event"),
            TokenType::Ident => match tok.value.as_str()? {
                "io" => Some("io"),
                "ecs" => Some("ecs"),
                "readonly" => Some("readonly"),
                "event" => Some("event"),
                _ => None,
            },
            _ => None,
        }
    }

    fn is_effect_annotated_fn(&self) -> bool {
        let mut offset = 0;
        if self.effect_name_at(offset).is_none() {
            return false;
        }
        while self.effect_name_at(offset).is_some() {
            offset += 1;
        }
        self.peek_at(offset).ty == TokenType::Fn
    }

    fn parse_effect_fn(&mut self) -> Result<FnDecl, ParseError> {
        let mut effects = Vec::new();
        while let Some(effect) = self.effect_name_at(0) {
            effects.push(effect.to_string());
            self.advance();
        }
        let mut decl = self.parse_fn_decl()?;
        decl.effects = effects;
        Ok(decl)
    }

    fn parse_system_decl(&mut self) -> Result<SystemDecl, ParseError> {
        let span = self.span();
        self.expect(TokenType::System)?;
        let name = self.expect_ident_text()?;
        self.expect(TokenType::LParen)?;
        let mut params = Vec::new();
        let mut accum_params: Vec<String> = Vec::new();
        while !self.check(TokenType::RParen) {
            let first = self.expect_ident_text()?;
            if !self.check(TokenType::Colon) {
                // Bare component name: a type-only filter param. The system
                // matches entities carrying it without binding the data —
                // the tag-component idiom (`system recompute(StatsDirty, ...)`).
                let mut ptype = first;
                if self.check(TokenType::Dot) {
                    self.advance();
                    ptype.push('.');
                    ptype.push_str(&self.expect_ident_text()?);
                }
                params.push((format!("_q{}", params.len()), false, ptype));
                if self.check(TokenType::Comma) {
                    self.advance();
                }
                continue;
            }
            let pname = first;
            self.expect(TokenType::Colon)?;
            let mut is_accum = false;
            // `p: accum R` — writable like `mut`, but parallel workers'
            // contributions are FOLDED per numeric field instead of
            // last-write-wins (dogfood feature seq 83 IDEA 02). Soft
            // keyword: only when another identifier (the type) follows, so
            // a type literally named `accum` still works as `p: accum`.
            let is_mut = if self.check(TokenType::Mut) {
                self.advance();
                true
            } else if self.check(TokenType::Ident)
                && self.peek().value.as_str() == Some("accum")
                && self.peek_at(1).ty == TokenType::Ident
            {
                self.advance();
                is_accum = true;
                true
            } else {
                false
            };
            let mut ptype = self.expect_ident_text()?;
            if self.check(TokenType::Dot) {
                self.advance();
                ptype.push('.');
                ptype.push_str(&self.expect_ident_text()?);
            }
            if is_accum {
                accum_params.push(pname.clone());
            }
            params.push((pname, is_mut, ptype));
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RParen)?;
        let mut after = Vec::new();
        let mut before = Vec::new();
        while self.check_ident_text("after") || self.check_ident_text("before") {
            if self.check_ident_text("after") {
                self.advance();
                let mut name = self.expect_ident_text()?;
                if self.check(TokenType::Dot) {
                    self.advance();
                    name.push('.');
                    name.push_str(&self.expect_ident_text()?);
                }
                after.push(name);
                while self.check(TokenType::Comma) {
                    self.advance();
                    let mut name = self.expect_ident_text()?;
                    if self.check(TokenType::Dot) {
                        self.advance();
                        name.push('.');
                        name.push_str(&self.expect_ident_text()?);
                    }
                    after.push(name);
                }
            } else if self.check_ident_text("before") {
                self.advance();
                let mut name = self.expect_ident_text()?;
                if self.check(TokenType::Dot) {
                    self.advance();
                    name.push('.');
                    name.push_str(&self.expect_ident_text()?);
                }
                before.push(name);
                while self.check(TokenType::Comma) {
                    self.advance();
                    let mut name = self.expect_ident_text()?;
                    if self.check(TokenType::Dot) {
                        self.advance();
                        name.push('.');
                        name.push_str(&self.expect_ident_text()?);
                    }
                    before.push(name);
                }
            }
        }
        let body = self.parse_block()?;
        Ok(SystemDecl {
            id: self.next_id(),
            span,
            name,
            is_pub: false,
            params,
            accum_params,
            body,
            after,
            before,
        })
    }
}
