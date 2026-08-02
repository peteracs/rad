use super::*;

impl Parser {
    pub(super) fn parse_block(&mut self) -> Result<Block, ParseError> {
        let span = self.span();
        self.expect(TokenType::LBrace)?;
        let mut stmts = Vec::new();
        let mut max_recovery_steps = 10000usize;
        while !self.check(TokenType::RBrace)
            && !self.check(TokenType::Eof)
            && max_recovery_steps > 0
        {
            max_recovery_steps -= 1;
            match self.parse_statement() {
                Ok(stmt) => stmts.push(stmt),
                Err(e) => {
                    self.push_error(e);
                    self.synchronize(&[
                        TokenType::Let,
                        TokenType::If,
                        TokenType::While,
                        TokenType::For,
                        TokenType::Return,
                        TokenType::Break,
                        TokenType::Continue,
                        TokenType::Emit,
                        TokenType::Schedule,
                        TokenType::Match,
                        TokenType::RBrace,
                    ]);
                    stmts.push(Stmt::Error(self.span()));
                }
            }
        }
        self.expect(TokenType::RBrace)?;
        Ok(Block {
            id: self.next_id(),
            span,
            stmts,
        })
    }

    pub(super) fn parse_statement(&mut self) -> Result<Stmt, ParseError> {
        if self.check_ident_text("settle") && self.peek_at(1).ty == TokenType::LBrace {
            return self.parse_settle();
        }
        if self.check_ident_text("propose") && self.peek_at(1).ty == TokenType::Ident {
            return self.parse_propose();
        }
        if self.check_ident_text("next") && self.peek_at(1).ty == TokenType::LParen {
            return self.parse_next();
        }
        if self.check(TokenType::Let) {
            return self.parse_let();
        }
        if self.check(TokenType::If) {
            return self.parse_if();
        }
        if self.check(TokenType::While) {
            return self.parse_while();
        }
        if self.check(TokenType::For) {
            return self.parse_for();
        }
        if self.check(TokenType::Return) {
            return self.parse_return();
        }
        if self.check(TokenType::Break) {
            let span = self.span();
            self.advance();
            return Ok(Stmt::Break(BreakStmt {
                id: self.next_id(),
                span,
            }));
        }
        if self.check(TokenType::Continue) {
            let span = self.span();
            self.advance();
            return Ok(Stmt::Continue(ContinueStmt {
                id: self.next_id(),
                span,
            }));
        }
        if self.check(TokenType::Emit) {
            return self.parse_emit();
        }
        if self.check(TokenType::Schedule) {
            return self.parse_schedule();
        }
        if self.check(TokenType::Match) {
            return Ok(Stmt::Match(self.parse_match_core()?));
        }
        if self.check(TokenType::Ident)
            && self.peek().value.as_str() == Some("update")
            && self.peek_at(1).ty == TokenType::LParen
        {
            return self.parse_update();
        }
        self.parse_assign_or_expr()
    }

    fn parse_update(&mut self) -> Result<Stmt, ParseError> {
        let span = self.span();
        self.advance(); // consume "update" ident
        self.expect(TokenType::LParen)?;
        let first = self.parse_expr()?;
        let (entity_expr, comp_name) = if self.check(TokenType::Comma) {
            self.advance();
            (Some(first), self.expect_ident_text()?)
        } else {
            let resource_name = match first {
                Expr::Ident(name, _) => name,
                _ => {
                    return Err(ParseError {
                        message: "update(resource) expects a resource type name identifier"
                            .to_string(),
                        line: self.peek().line,
                        col: self.peek().col,
                    })
                }
            };
            (None, resource_name)
        };
        self.expect(TokenType::RParen)?;
        self.expect(TokenType::LBrace)?;
        let mut field_updates = Vec::new();
        while !self.check(TokenType::RBrace) {
            let fname = self.expect_field_name()?;
            let index = if self.check(TokenType::LBracket) {
                self.advance();
                let idx = self.parse_expr()?;
                self.expect(TokenType::RBracket)?;
                if self.check(TokenType::LBracket) {
                    return Err(ParseError {
                        message: format!(
                            "Nested indexed update on '{}' is not supported — read the component first, then assign the whole element: `{}[i] = set_at(c.{}[i], j, v)`",
                            fname, fname, fname
                        ),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                Some(idx)
            } else {
                None
            };
            self.expect(TokenType::Assign)?;
            let fexpr = self.parse_expr()?;
            field_updates.push(FieldUpdate {
                name: fname,
                index,
                value: fexpr,
            });
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RBrace)?;
        Ok(Stmt::Update(UpdateStmt {
            id: self.next_id(),
            span,
            entity_expr,
            comp_name,
            field_updates,
        }))
    }

    fn parse_let(&mut self) -> Result<Stmt, ParseError> {
        let span = self.span();
        self.expect(TokenType::Let)?;
        let is_unique = if self.check(TokenType::Unique) {
            self.advance();
            true
        } else {
            false
        };
        let mutable = if self.check(TokenType::Mut) {
            self.advance();
            true
        } else {
            false
        };
        let recursive = if self.check(TokenType::Rec) {
            self.advance();
            true
        } else {
            false
        };

        // `let Some { value: hp } = e else { ... }` — variant pattern + mandatory else
        // (Must run before `let (a, b) = ...` tuple destructuring, which also starts with `(` after `let mut`.)
        if self.is_identifier_token(self.peek().ty)
            && (self.peek_at(1).ty == TokenType::LBrace || self.peek_at(1).ty == TokenType::LParen)
        {
            let variant_name = self.expect_ident_text()?;
            let mut bindings = Vec::new();
            let mut pattern_bindings = Vec::new();
            let mut has_rest = false;

            if self.check(TokenType::LBrace) {
                self.expect(TokenType::LBrace)?;
                let mut path_prefix = Vec::new();
                self.parse_match_pattern_entries(
                    &mut path_prefix,
                    &mut bindings,
                    &mut pattern_bindings,
                    &mut has_rest,
                )?;
                self.expect(TokenType::RBrace)?;
            } else if self.check(TokenType::LParen) {
                self.expect(TokenType::LParen)?;
                if variant_name == "Ok" || variant_name == "Some" {
                    let binding = self.expect_ident_text()?;
                    pattern_bindings.push(MatchBinding {
                        name: binding,
                        path: vec!["value".to_string()],
                    });
                } else if variant_name == "Err" {
                    let binding = self.expect_ident_text()?;
                    pattern_bindings.push(MatchBinding {
                        name: binding,
                        path: vec!["message".to_string()],
                    });
                } else if variant_name == "None" {
                    // None() has no bindings
                } else {
                    return Err(ParseError {
                        message: "Tuple-style let-else bindings are only supported for Ok, Err, Some, and None".to_string(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                self.expect(TokenType::RParen)?;
            }
            let type_annotation = if self.check(TokenType::Colon) {
                self.advance();
                Some(self.parse_type()?)
            } else {
                None
            };
            self.expect(TokenType::Assign)?;
            let subject = self.parse_expr()?;
            self.expect(TokenType::Else)?;
            let else_block = self.parse_block()?;
            return Ok(Stmt::LetElse(LetElseStmt {
                id: self.next_id(),
                span,
                mutable,
                type_annotation,
                variant_name,
                bindings,
                pattern_bindings,
                has_rest,
                subject,
                else_block,
            }));
        }

        let (names, tuple_destructure) = if self.check(TokenType::LParen) {
            self.advance();
            let mut names = Vec::new();
            while !self.check(TokenType::RParen) {
                if self.check(TokenType::Mut) {
                    return Err(ParseError {
                        message: "Granular mutability like `let (mut a, b)` is not supported. Use `let mut (a, b)` instead.".to_string(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                let n = self.expect_ident_text()?;
                // `_` is a discard, not a binding — `let (_, total, _) = r`
                // is the standard ignore-the-rest shape.
                if n != "_" && names.contains(&n) {
                    return Err(ParseError {
                        message: format!("Duplicate binding `{}` in tuple destructuring", n),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                names.push(n);
                if !self.check(TokenType::RParen) {
                    self.expect(TokenType::Comma)?;
                }
            }
            self.expect(TokenType::RParen)?;
            if names.is_empty() {
                return Err(ParseError {
                    message: "Expected at least one variable name in `let (...)`".to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            (names, true)
        } else {
            (vec![self.expect_ident_text()?], false)
        };

        if recursive && (tuple_destructure || names.len() != 1) {
            return Err(ParseError {
                message:
                    "`let rec` requires a single variable name (tuple destructuring is not allowed)"
                        .to_string(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        let type_annotation = if self.check(TokenType::Colon) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(TokenType::Assign)?;
        let value = self.parse_expr()?;
        Ok(Stmt::Let(LetStmt {
            id: self.next_id(),
            span,
            names,
            tuple_destructure,
            mutable,
            recursive,
            is_unique,
            is_pub: false,
            type_annotation,
            value,
        }))
    }

    fn parse_if(&mut self) -> Result<Stmt, ParseError> {
        let id = self.next_id();
        let span = self.span();
        self.expect(TokenType::If)?;
        let condition = self.parse_expr()?;
        let then_block = self.parse_block()?;
        let else_block = if self.check(TokenType::Else) {
            self.advance();
            if self.check(TokenType::If) {
                let if_stmt = self.parse_if()?;
                let blk_id = self.next_id();
                Some(Block {
                    id: blk_id,
                    span: Span::default(),
                    stmts: vec![if_stmt],
                })
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };
        Ok(Stmt::If(IfStmt {
            id,
            span,
            condition,
            then_block,
            else_block,
        }))
    }

    fn parse_while(&mut self) -> Result<Stmt, ParseError> {
        let span = self.span();
        self.expect(TokenType::While)?;
        let condition = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::While(WhileStmt {
            id: self.next_id(),
            span,
            condition,
            body,
        }))
    }

    fn parse_for(&mut self) -> Result<Stmt, ParseError> {
        let span = self.span();
        self.expect(TokenType::For)?;

        let has_parens = if self.check(TokenType::LParen) {
            self.advance();
            true
        } else {
            false
        };

        let mut bindings = Vec::new();
        let mut destructure_bindings = None;
        if self.check(TokenType::LBracket) {
            self.advance();
            let mut names = Vec::new();
            while !self.check(TokenType::RBracket) {
                let n = self.expect_ident_text()?;
                if n != "_" && names.contains(&n) {
                    return Err(ParseError {
                        message: format!("Duplicate binding `{}` in for-loop destructuring", n),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                names.push(n);
                if !self.check(TokenType::RBracket) {
                    self.expect(TokenType::Comma)?;
                }
            }
            self.expect(TokenType::RBracket)?;
            if names.is_empty() {
                return Err(ParseError {
                    message: "Expected at least one variable name in for-loop destructuring"
                        .to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            if self.check(TokenType::Comma) {
                return Err(ParseError {
                    message: "For-loop destructuring must be the only binding".to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            bindings.push("__fd_0".to_string());
            destructure_bindings = Some(names);
        } else {
            bindings.push(self.expect_ident_text()?);
            while self.check(TokenType::Comma) {
                self.advance();
                if has_parens && self.check(TokenType::RParen) {
                    break;
                }
                bindings.push(self.expect_ident_text()?);
            }
        }

        if has_parens {
            self.expect(TokenType::RParen)?;
        }

        self.expect(TokenType::In)?;
        let iterable = self.parse_expr()?;
        // `for x in xs where cond { body }` — filtered iteration, sugar for
        // wrapping the body in `if cond`. Reads like the query it is.
        let filter = if self.check_ident_text("where") {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        let mut body = self.parse_block()?;
        if let Some(cond) = filter {
            let block_span = body.span.clone();
            let guarded = Stmt::If(IfStmt {
                id: self.next_id(),
                span: span.clone(),
                condition: cond,
                then_block: body,
                else_block: None,
            });
            body = Block {
                id: self.next_id(),
                span: block_span,
                stmts: vec![guarded],
            };
        }
        Ok(Stmt::For(ForStmt {
            id: self.next_id(),
            span,
            bindings,
            destructure_bindings,
            iterable,
            body,
        }))
    }

    fn parse_return(&mut self) -> Result<Stmt, ParseError> {
        let span = self.span();
        self.expect(TokenType::Return)?;
        let value = if !self.check_any(&[TokenType::RBrace, TokenType::Eof]) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Stmt::Return(ReturnStmt {
            id: self.next_id(),
            span,
            value,
        }))
    }

    fn parse_emit(&mut self) -> Result<Stmt, ParseError> {
        let span = self.span();
        self.expect(TokenType::Emit)?;
        let mut event_name = self.expect_ident_text()?;
        if self.check(TokenType::Dot) {
            self.advance();
            event_name.push('.');
            event_name.push_str(&self.expect_ident_text()?);
        }
        self.expect(TokenType::LBrace)?;
        let mut fields = Vec::new();
        while !self.check(TokenType::RBrace) {
            let fname = self.expect_field_name()?;
            self.expect(TokenType::Colon)?;
            let fval = self.parse_expr()?;
            fields.push((fname, fval));
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RBrace)?;
        // `emit E { .. } after N` — delayed delivery, N flush cycles out
        let delay = if self.check_ident_text("after") {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Stmt::Emit(EmitStmt {
            id: self.next_id(),
            span,
            event_name,
            fields,
            delay,
        }))
    }

    fn parse_schedule(&mut self) -> Result<Stmt, ParseError> {
        let span = self.span();
        self.expect(TokenType::Schedule)?;
        // Soft keyword: `schedule serial [ ... ]` runs the listed systems one
        // at a time in topological order (dogfood feature seq 83). Safe to
        // sniff — a plain `schedule` is always followed by `[`.
        let serial = if self.check(TokenType::Ident) && self.peek().value.as_str() == Some("serial")
        {
            self.advance();
            true
        } else {
            false
        };
        self.expect(TokenType::LBracket)?;
        let mut systems = Vec::new();
        while !self.check(TokenType::RBracket) {
            if self.check(TokenType::System) {
                self.advance();
                self.expect(TokenType::DColon)?;
                let mut path = vec![self.expect_ident_text()?];
                while self.check(TokenType::DColon) {
                    self.advance();
                    path.push(self.expect_ident_text()?);
                }
                systems.push(crate::simulate_syntax::system_ref_qualified_string(&path));
            } else {
                let mut name = self.expect_ident_text()?;
                if self.check(TokenType::Dot) {
                    self.advance();
                    name.push('.');
                    name.push_str(&self.expect_ident_text()?);
                }
                systems.push(name);
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RBracket)?;
        Ok(Stmt::Schedule(ScheduleStmt {
            id: self.next_id(),
            span,
            systems,
            serial,
        }))
    }

    pub(super) fn parse_match_core(&mut self) -> Result<MatchStmt, ParseError> {
        let span = self.span();
        self.expect(TokenType::Match)?;
        let subject = self.parse_expr()?;
        self.expect(TokenType::LBrace)?;
        let mut cases = Vec::new();
        while !self.check(TokenType::RBrace) {
            let start_span = self.span();
            let mut has_component_pattern: Option<(String, Option<String>)> = None;
            let (state_path, is_wildcard, literal_pattern) = match self.peek().ty {
                TokenType::Ident => {
                    let mut path = vec![self.expect_ident_text()?];
                    if path[0] == "has" && self.check(TokenType::Ident) {
                        let comp_name = self.expect_ident_text()?;
                        let binding = if self.check(TokenType::LParen) {
                            self.advance();
                            let b = self.expect_ident_text()?;
                            self.expect(TokenType::RParen)?;
                            Some(b)
                        } else {
                            None
                        };
                        has_component_pattern = Some((comp_name, binding));
                        (vec![], false, None)
                    } else if path[0] == "_" {
                        (path, true, None)
                    } else {
                        while self.check(TokenType::Dot) || self.check(TokenType::DColon) {
                            self.advance();
                            path.push(self.expect_ident_text()?);
                        }
                        (path, false, None)
                    }
                }
                TokenType::Int | TokenType::String | TokenType::True | TokenType::False => {
                    let lit = self.parse_primary()?;
                    (vec![], false, Some(lit))
                }
                _ => {
                    return Err(ParseError {
                        message: "Expected match pattern: identifier, `_`, or literal".to_string(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
            };
            let state_name = state_path.last().cloned().unwrap_or_default();
            let mut bindings = Vec::new();
            let mut pattern_bindings = Vec::new();
            let mut has_rest = false;
            let mut is_bare_variant = false;
            if is_wildcard || literal_pattern.is_some() {
                if self.check(TokenType::LBrace) {
                    return Err(ParseError {
                        message:
                            "Wildcard and literal match arms cannot use `{ ... }` destructuring"
                                .to_string(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
            } else if self.check(TokenType::LBrace) {
                self.advance();
                let mut path_prefix = Vec::new();
                self.parse_match_pattern_entries(
                    &mut path_prefix,
                    &mut bindings,
                    &mut pattern_bindings,
                    &mut has_rest,
                )?;
                self.expect(TokenType::RBrace)?;
            } else if self.check(TokenType::LParen) {
                self.advance();
                if state_name == "Ok" || state_name == "Some" {
                    let binding = self.expect_ident_text()?;
                    pattern_bindings.push(MatchBinding {
                        name: binding,
                        path: vec!["value".to_string()],
                    });
                } else if state_name == "Err" {
                    let binding = self.expect_ident_text()?;
                    pattern_bindings.push(MatchBinding {
                        name: binding,
                        path: vec!["message".to_string()],
                    });
                } else if state_name == "None" {
                    // None() has no bindings
                } else {
                    return Err(ParseError {
                        message:
                            "Tuple-style match arms are only supported for Ok, Err, Some, and None"
                                .to_string(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                self.expect(TokenType::RParen)?;
            } else {
                is_bare_variant = true;
            }
            let guard = if self.check_any(&[TokenType::When, TokenType::If]) {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.expect(TokenType::FatArrow)?;
            // `pat => expr` is sugar for `pat => { expr }` — in expression
            // matches the bare expression IS the arm's value, and guards
            // (`pat when cond => expr`) compose with it.
            let body = if self.check(TokenType::LBrace) {
                self.parse_block()?
            } else {
                let arm_span = self.span();
                let value = self.parse_expr()?;
                Block {
                    id: self.next_id(),
                    span: arm_span.clone(),
                    stmts: vec![Stmt::Expr(ExprStmt {
                        id: self.next_id(),
                        span: arm_span,
                        expr: value,
                    })],
                }
            };
            let pattern = if let Some((comp, bind)) = has_component_pattern.take() {
                Pattern::HasComponent {
                    component: comp,
                    binding: bind,
                }
            } else if is_wildcard {
                Pattern::Wildcard
            } else if let Some(lit) = literal_pattern {
                Pattern::Literal(lit)
            } else {
                Pattern::Variant {
                    path: state_path,
                    bindings,
                    pattern_bindings,
                    has_rest,
                    is_bare_variant,
                }
            };

            cases.push(MatchCase {
                id: self.next_id(),
                span: start_span,
                pattern,
                guard,
                body,
            });
        }
        self.expect(TokenType::RBrace)?;
        Ok(MatchStmt {
            id: self.next_id(),
            span,
            subject,
            cases,
        })
    }

    fn parse_assign_or_expr(&mut self) -> Result<Stmt, ParseError> {
        let expr = self.parse_expr()?;
        let span = expr.span().clone();
        if self.check(TokenType::Assign) {
            self.advance();
            let value = self.parse_expr()?;
            return Ok(Stmt::Assign(AssignStmt {
                id: self.next_id(),
                span,
                target: expr,
                value,
            }));
        } else if let Expr::Binary(_, BinOp::Shl, _, _) = &expr {
            // `xs << v` at statement level is list append (the expression
            // parser now owns `<<` for int shifts, so we rewrite here).
            // A left-spine chain `xs << a << b` appends in order:
            // `xs = push(push(xs, a), b)`.
            let mut items = Vec::new();
            let mut head = expr;
            while let Expr::Binary(lhs, BinOp::Shl, rhs, _) = head {
                items.push(*rhs);
                head = *lhs;
            }
            items.reverse();
            let target = head;
            // Appending through an index auto-vivifies: `m[k] << v` seeds
            // a missing key with `[]` (the bucket-fill idiom). Plain
            // `xs << v` reads the binding as before.
            let mut value = match &target {
                Expr::Index(obj, idx, ispan) => Expr::Call(
                    Box::new(Expr::Ident("get_or".to_string(), ispan.clone())),
                    vec![
                        (**obj).clone(),
                        (**idx).clone(),
                        Expr::ListLit(vec![], ispan.clone()),
                    ],
                    ispan.clone(),
                ),
                _ => target.clone(),
            };
            for item in items {
                value = Expr::Call(
                    Box::new(Expr::Ident("push".to_string(), span.clone())),
                    vec![value, item],
                    span.clone(),
                );
            }
            return Ok(Stmt::Assign(AssignStmt {
                id: self.next_id(),
                span,
                target,
                value,
            }));
        }
        Ok(Stmt::Expr(ExprStmt {
            id: self.next_id(),
            span,
            expr,
        }))
    }

    fn parse_match_pattern_entries(
        &mut self,
        path_prefix: &mut Vec<String>,
        bindings: &mut Vec<String>,
        pattern_bindings: &mut Vec<MatchBinding>,
        has_rest: &mut bool,
    ) -> Result<(), ParseError> {
        while !self.check(TokenType::RBrace) {
            if self.check(TokenType::DotDot) {
                if !self.compat_v0_5_dx_enabled() {
                    return Err(ParseError {
                        message: "Error[E2503]: Match rest binding '..' requires --compat-v0.5-dx"
                            .to_string(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                if *has_rest {
                    return Err(ParseError {
                        message: "Error[E2503]: Match rest binding '..' can appear at most once"
                            .to_string(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                *has_rest = true;
                self.advance();
                if self.check(TokenType::Comma) {
                    self.advance();
                    if !self.check(TokenType::RBrace) {
                        return Err(ParseError {
                            message:
                                "Error[E2503]: Match rest binding '..' must be the final entry"
                                    .to_string(),
                            line: self.peek().line,
                            col: self.peek().col,
                        });
                    }
                }
                continue;
            }
            if *has_rest {
                return Err(ParseError {
                    message: "Error[E2503]: Match rest binding '..' must be the final entry"
                        .to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }

            let field_name = self.expect_field_name()?;
            path_prefix.push(field_name.clone());
            if self.check(TokenType::Colon) {
                self.advance();
                if self.check(TokenType::LBrace) {
                    self.advance();
                    self.parse_match_pattern_entries(
                        path_prefix,
                        bindings,
                        pattern_bindings,
                        has_rest,
                    )?;
                    self.expect(TokenType::RBrace)?;
                } else {
                    let alias_name = self.expect_ident_text()?;
                    pattern_bindings.push(MatchBinding {
                        name: alias_name.clone(),
                        path: path_prefix.clone(),
                    });
                    if path_prefix.len() == 1 && alias_name == field_name {
                        bindings.push(alias_name);
                    }
                }
            } else {
                // Shorthand binds a variable with the field's own name —
                // impossible to reference later when the field is a keyword
                // (`on` won't lex as an identifier), so demand an alias.
                if crate::lexer::keyword_type_of(&field_name).is_some() {
                    return Err(ParseError {
                        message: format!(
                            "Pattern field '{}' is a reserved word and cannot bind by shorthand — use `{}: some_name` to bind it",
                            field_name, field_name
                        ),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                let bind_name = field_name;
                pattern_bindings.push(MatchBinding {
                    name: bind_name.clone(),
                    path: path_prefix.clone(),
                });
                if path_prefix.len() == 1 {
                    bindings.push(bind_name);
                }
            }
            path_prefix.pop();

            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        Ok(())
    }
}
