impl Parser {
    fn parse_ownership_target(&mut self) -> Result<String, ParseError> {
        if self.check(TokenType::Star) {
            self.advance();
            return Ok("*".to_string());
        }
        if self.check(TokenType::String) {
            let token = self.peek().clone();
            self.advance();
            return Ok(self
                .token_str_value(&token, "authority target")?
                .to_string());
        }
        let mut target = self.expect_ident_text()?;
        if self.check(TokenType::Dot) {
            self.advance();
            target.push('.');
            target.push_str(&self.expect_ident_text()?);
        }
        Ok(target)
    }

    fn parse_optional_ownership_writes(&mut self) -> Result<Vec<String>, ParseError> {
        if !self.check_ident_text("writes")
            || self.peek_at(1).ty != TokenType::Ident
            || self.peek_at(1).value.as_str() != Some("owned")
        {
            return Ok(Vec::new());
        }
        self.advance();
        self.advance();
        if self.check(TokenType::LBracket) {
            self.advance();
            let mut targets = Vec::new();
            while !self.check(TokenType::RBracket) {
                targets.push(self.parse_ownership_target()?);
                if self.check(TokenType::Comma) {
                    self.advance();
                } else if !self.check(TokenType::RBracket) {
                    return Err(ParseError {
                        message: "Expected `,` or `]` in owned write grant".to_string(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
            }
            self.expect(TokenType::RBracket)?;
            return Ok(targets);
        }
        if self.check(TokenType::Ident) || self.check(TokenType::Star) {
            return Ok(vec![self.parse_ownership_target()?]);
        }
        Ok(vec!["*".to_string()])
    }

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
        let ownership_writes = self.parse_optional_ownership_writes()?;
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
            ownership_writes,
            contracts: CallableContracts::default(),
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
        let ownership_writes = self.parse_optional_ownership_writes()?;
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
            ownership_writes,
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

    fn parse_test_decl(&mut self, shared_world: bool) -> Result<TestDecl, ParseError> {
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

        let ownership_writes = self.parse_optional_ownership_writes()?;
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
            shared_world,
            generators: Vec::new(),
            ownership_writes,
        })
    }

    fn parse_model_decl(&mut self) -> Result<ModelDecl, ParseError> {
        let span = self.span();
        self.expect(TokenType::Ident)?; // `model`
        let name = self.expect_ident_text()?;
        let ownership_writes = self.parse_optional_ownership_writes()?;
        self.expect(TokenType::LBrace)?;

        let mut commands = Vec::new();
        let mut invariants = Vec::new();
        let mut temporal = Vec::new();
        let mut runs = 100u32;
        let mut max_commands = 100u32;
        let mut seed = 0x5241_444d_4f44_454cu64;

        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            if self.check_ident_text("commands") {
                self.advance();
                self.expect(TokenType::LBracket)?;
                while !self.check(TokenType::RBracket) {
                    commands.push(self.parse_expr()?);
                    if self.check(TokenType::Comma) {
                        self.advance();
                    } else if !self.check(TokenType::RBracket) {
                        return Err(ParseError {
                            message: "Expected ',' or ']' in model command list".to_string(),
                            line: self.peek().line,
                            col: self.peek().col,
                        });
                    }
                }
                self.expect(TokenType::RBracket)?;
                continue;
            }
            if self.check_ident_text("invariant") {
                self.advance();
                invariants.push(self.parse_block()?);
                continue;
            }
            if self.check_ident_text("temporal") {
                self.advance();
                self.expect(TokenType::LBrace)?;
                while !self.check(TokenType::RBrace) {
                    let clause = if self.check_ident_text("always") {
                        self.advance();
                        TemporalClause::Always(self.expect_ident_text()?)
                    } else if self.check_ident_text("eventually") {
                        self.advance();
                        TemporalClause::Eventually(self.expect_ident_text()?)
                    } else if self.check_ident_text("exactly_once") {
                        self.advance();
                        TemporalClause::ExactlyOnce(self.expect_ident_text()?)
                    } else {
                        let left = self.expect_ident_text()?;
                        if self.check_ident_text("until") {
                            self.advance();
                            TemporalClause::Until {
                                condition: left,
                                terminal: self.expect_ident_text()?,
                            }
                        } else if self.check_ident_text("eventually") {
                            self.advance();
                            TemporalClause::LeadsTo {
                                trigger: left,
                                consequence: self.expect_ident_text()?,
                            }
                        } else if self.check_ident_text("never_after") {
                            self.advance();
                            TemporalClause::NeverAfter {
                                prohibited: left,
                                terminal: self.expect_ident_text()?,
                            }
                        } else if self.check_ident_text("eventually_within") {
                            self.advance();
                            let consequence = self.expect_ident_text()?;
                            let token = self.peek().clone();
                            self.expect(TokenType::Int)?;
                            let raw = token.value.as_int().ok_or_else(|| ParseError {
                                message: "eventually_within requires an integer bound".to_string(),
                                line: token.line,
                                col: token.col,
                            })?;
                            let bound = u32::try_from(raw).map_err(|_| ParseError {
                                message: "eventually_within bound must fit u32".to_string(),
                                line: token.line,
                                col: token.col,
                            })?;
                            if bound == 0 {
                                return Err(ParseError {
                                    message: "eventually_within bound must be positive".to_string(),
                                    line: token.line,
                                    col: token.col,
                                });
                            }
                            TemporalClause::EventuallyWithin {
                                trigger: left,
                                consequence,
                                bound,
                            }
                        } else {
                            return Err(ParseError {
                                message: "Expected 'until', 'eventually', 'never_after', or 'eventually_within' in temporal clause".to_string(),
                                line: self.peek().line,
                                col: self.peek().col,
                            });
                        }
                    };
                    temporal.push(clause);
                    if self.check(TokenType::Comma) {
                        self.advance();
                    }
                }
                self.expect(TokenType::RBrace)?;
                continue;
            }
            if self.check_ident_text("runs")
                || self.check_ident_text("max_commands")
                || self.check_ident_text("seed")
            {
                let key = self
                    .peek()
                    .value
                    .as_str()
                    .expect("model configuration key is an identifier")
                    .to_string();
                self.advance();
                if self.check(TokenType::Colon) {
                    self.advance();
                }
                let token = self.peek().clone();
                self.expect(TokenType::Int)?;
                let raw = token.value.as_int().ok_or_else(|| ParseError {
                    message: "Model runs, max_commands, and seed require integer literals"
                        .to_string(),
                    line: token.line,
                    col: token.col,
                })?;
                if raw < 0 {
                    return Err(ParseError {
                        message: "Model runs, max_commands, and seed cannot be negative"
                            .to_string(),
                        line: token.line,
                        col: token.col,
                    });
                }
                if key == "runs" {
                    runs = u32::try_from(raw).map_err(|_| ParseError {
                        message: "Model run count exceeds u32".to_string(),
                        line: token.line,
                        col: token.col,
                    })?;
                } else if key == "max_commands" {
                    max_commands = u32::try_from(raw).map_err(|_| ParseError {
                        message: "Model command bound exceeds u32".to_string(),
                        line: token.line,
                        col: token.col,
                    })?;
                } else {
                    seed = raw as u64;
                }
                continue;
            }
            return Err(ParseError {
                message: format!(
                    "Unknown model section '{}'; expected commands, invariant, temporal, runs, max_commands, or seed",
                    self.peek().value
                ),
                line: self.peek().line,
                col: self.peek().col,
            });
        }
        self.expect(TokenType::RBrace)?;
        if commands.is_empty() {
            return Err(ParseError {
                message: format!("Model '{}' must declare at least one command", name),
                line: span.line,
                col: span.col,
            });
        }
        if invariants.is_empty() && temporal.is_empty() {
            return Err(ParseError {
                message: format!("Model '{}' must declare an invariant or temporal clause", name),
                line: span.line,
                col: span.col,
            });
        }
        if runs == 0 || max_commands == 0 {
            return Err(ParseError {
                message: "Model runs and max_commands must be greater than zero".to_string(),
                line: span.line,
                col: span.col,
            });
        }
        Ok(ModelDecl {
            id: self.next_id(),
            span,
            name,
            commands,
            invariants,
            temporal,
            runs,
            max_commands,
            seed,
            ownership_writes,
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
        let mut authority_reads = Vec::new();
        let mut authority_writes = Vec::new();
        let mut ownership_writes = Vec::new();
        let mut authority_emits = Vec::new();
        let mut authority_io = None;
        let mut authority_async = None;
        while !self.check(TokenType::RParen) {
            let first = if self.check(TokenType::Async) {
                self.advance();
                "async".to_string()
            } else {
                self.expect_ident_text()?
            };
            if first == "writes" && self.check_ident_text("owned") {
                self.advance();
                if self.check(TokenType::LBracket) {
                    self.advance();
                    while !self.check(TokenType::RBracket) {
                        ownership_writes.push(self.parse_ownership_target()?);
                        if self.check(TokenType::Comma) {
                            self.advance();
                        } else if !self.check(TokenType::RBracket) {
                            return Err(ParseError {
                                message: "Expected `,` or `]` in owned write grant".to_string(),
                                line: self.peek().line,
                                col: self.peek().col,
                            });
                        }
                    }
                    self.expect(TokenType::RBracket)?;
                } else if self.check(TokenType::Ident) || self.check(TokenType::Star) {
                    ownership_writes.push(self.parse_ownership_target()?);
                } else {
                    ownership_writes.push("*".to_string());
                }
                if self.check(TokenType::Comma) {
                    self.advance();
                }
                continue;
            }
            if (first == "reads" || first == "writes" || first == "emits")
                && (self.check(TokenType::Ident)
                    || self.check(TokenType::Star)
                    || self.check(TokenType::String))
            {
                let authority = if self.check(TokenType::Star) {
                    self.advance();
                    "*".to_string()
                } else if self.check(TokenType::String) {
                    let token = self.peek().clone();
                    self.advance();
                    self.token_str_value(&token, "system authority")?.to_string()
                } else {
                    let mut name = self.expect_ident_text()?;
                    if self.check(TokenType::Dot) {
                        self.advance();
                        name.push('.');
                        name.push_str(&self.expect_ident_text()?);
                    }
                    name
                };
                if first == "reads" {
                    authority_reads.push(authority);
                } else if first == "writes" {
                    authority_writes.push(authority);
                } else {
                    authority_emits.push(authority);
                }
                if self.check(TokenType::Comma) {
                    self.advance();
                }
                continue;
            }
            if first == "io" || first == "async" {
                let value = if self.check(TokenType::True) {
                    self.advance();
                    true
                } else if self.check(TokenType::False) {
                    self.advance();
                    false
                } else {
                    return Err(ParseError {
                        message: format!("Expected `true` or `false` after system `{first}` permission"),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                };
                let slot = if first == "io" {
                    &mut authority_io
                } else {
                    &mut authority_async
                };
                if slot.replace(value).is_some() {
                    return Err(ParseError {
                        message: format!("System `{first}` permission is declared more than once"),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                if self.check(TokenType::Comma) {
                    self.advance();
                }
                continue;
            }
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
            authority_reads,
            authority_writes,
            authority_emits,
            authority_io: authority_io.unwrap_or(false),
            authority_async: authority_async.unwrap_or(false),
            ownership_writes,
            body,
            after,
            before,
            contracts: CallableContracts::default(),
        })
    }
}
