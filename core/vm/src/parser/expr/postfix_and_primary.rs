impl Parser {
fn parse_fn_expr(&mut self) -> Result<Expr, ParseError> {
        let span = self.span();
        self.expect(TokenType::Fn)?;
        self.expect(TokenType::LParen)?;
        let mut params = Vec::new();
        let mut param_muts = Vec::new();
        let mut param_types = Vec::new();
        let mut param_destructures = Vec::new();
        while !self.check(TokenType::RParen) {
            let is_mut = self.check(TokenType::Mut);
            if is_mut {
                self.advance();
            }
            param_muts.push(is_mut);
            if self.check(TokenType::LBracket) {
                self.advance();
                let mut names = Vec::new();
                while !self.check(TokenType::RBracket) {
                    let n = self.expect_ident_text()?;
                    if n != "_" && names.contains(&n) {
                        return Err(ParseError {
                            message: format!("Duplicate binding `{}` in closure destructuring", n),
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
                        message: "Expected at least one variable name in closure destructuring"
                            .to_string(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                let temp_name = format!("__dp_{}", params.len());
                params.push(temp_name);
                param_destructures.push(Some(names));
            } else {
                params.push(self.expect_ident_text()?);
                param_destructures.push(None);
            }
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
        Ok(Expr::FnExpr(
            params,
            param_muts,
            param_types,
            param_destructures,
            return_type,
            body,
            span,
        ))
    }

    fn parse_dollar_interpolated_string(
        &mut self,
        raw: &str,
        line: u32,
        col: u32,
    ) -> Result<Vec<FStringPart>, ParseError> {
        let mut parts = Vec::new();
        let mut lit = String::new();
        let chars: Vec<char> = raw.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == '$' {
                lit.push('$');
                i += 2;
            } else if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1] == '{' {
                if !lit.is_empty() {
                    parts.push(FStringPart::Lit(std::mem::take(&mut lit)));
                }
                i += 2;
                let mut expr_src = String::new();
                let mut depth = 1;
                while i < chars.len() && depth > 0 {
                    if chars[i] == '{' {
                        depth += 1;
                    }
                    if chars[i] == '}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    expr_src.push(chars[i]);
                    i += 1;
                }
                if depth != 0 {
                    return Err(ParseError {
                        message: "Unterminated string interpolation `${...}`".to_string(),
                        line,
                        col,
                    });
                }
                i += 1;
                let mut lexer = crate::lexer::Lexer::new_with_offset(&expr_src, line, col);
                let (tokens, lex_errors) = lexer.tokenize();
                if !lex_errors.is_empty() {
                    let e = &lex_errors[0];
                    return Err(ParseError {
                        message: format!("In string interpolation: {}", e.message),
                        line: e.line,
                        col: e.col,
                    });
                }
                let mut parser = Parser::new(tokens).with_options(self.options);
                let expr = parser.parse_expr().map_err(|e| ParseError {
                    message: format!("In string interpolation: {}", e.message),
                    line,
                    col,
                })?;
                if !parser.check(TokenType::Eof) {
                    return Err(ParseError {
                        message: "In string interpolation: unexpected trailing tokens".to_string(),
                        line,
                        col,
                    });
                }
                parts.push(FStringPart::Expr(Box::new(expr), None));
            } else {
                lit.push(chars[i]);
                i += 1;
            }
        }
        if !lit.is_empty() {
            parts.push(FStringPart::Lit(lit));
        }
        Ok(parts)
    }

    fn parse_query_expr(&mut self, span: Span) -> Result<Expr, ParseError> {
        self.expect(TokenType::LBrace)?;
        let mut components = Vec::new();
        while !self.check(TokenType::RBrace) {
            let is_mut = if self.check(TokenType::Mut) {
                self.advance();
                true
            } else {
                false
            };
            components.push((self.expect_ident_text()?, is_mut));
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RBrace)?;

        let mut select = Vec::new();
        if self.check(TokenType::Ident) && self.peek().value.as_str() == Some("select") {
            self.advance();
            loop {
                select.push(self.expect_ident_text()?);
                if self.check(TokenType::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        let filter = if self.check(TokenType::Ident) && self.peek().value.as_str() == Some("where")
        {
            self.advance();
            Some(Box::new(self.parse_expr()?))
        } else {
            None
        };

        Ok(Expr::QueryExpr(
            QueryExprNode {
                components,
                filter,
                select,
            },
            span,
        ))
    }

fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.check(TokenType::Dot) {
                self.advance();
                let name = self.expect_field_name()?;
                let span = expr.span().clone();
                expr = Expr::Field(Box::new(expr), name, span);

                if let Expr::Field(ref base, ref member, ref field_span) = expr {
                    if let Expr::Ident(_, _) = base.as_ref() {
                        let qualified = if let Expr::Ident(ns, _) = base.as_ref() {
                            format!("{}.{}", ns, member)
                        } else {
                            unreachable!()
                        };

                        if self.check(TokenType::DColon) {
                            self.advance();
                            let variant = self.expect_ident_text()?;
                            if self.looks_like_braced_field_init() {
                                self.advance();
                                let mut fields = Vec::new();
                                let mut rest = None;
                                while !self.check(TokenType::RBrace) {
                                    if self.check(TokenType::DotDot) {
                                        if rest.is_some() {
                                            return Err(ParseError {
                                                message: "Component update can contain at most one `..base` entry".to_string(),
                                                line: self.peek().line,
                                                col: self.peek().col,
                                            });
                                        }
                                        self.advance();
                                        rest = Some(self.parse_expr()?);
                                        if self.check(TokenType::Comma) {
                                            self.advance();
                                            if !self.check(TokenType::RBrace) {
                                                return Err(ParseError {
                                                    message: "Component update `..base` must be the final entry".to_string(),
                                                    line: self.peek().line,
                                                    col: self.peek().col,
                                                });
                                            }
                                        }
                                        continue;
                                    }
                                    let fname = self.expect_field_name()?;
                                    self.expect(TokenType::Colon)?;
                                    let fval = self.parse_expr()?;
                                    fields.push((fname, fval));
                                    if self.check(TokenType::Comma) {
                                        self.advance();
                                    }
                                }
                                self.expect(TokenType::RBrace)?;
                                expr = Expr::VariantExpr(
                                    qualified,
                                    variant,
                                    fields,
                                    field_span.clone(),
                                );
                                continue;
                            }
                            expr = Expr::StateRef(qualified, variant, field_span.clone());
                            continue;
                        }

                        if self.looks_like_braced_field_init() {
                            let comp_span = field_span.clone();
                            self.advance();
                            let mut fields = Vec::new();
                            let mut rest: Option<Box<Expr>> = None;
                            while !self.check(TokenType::RBrace) {
                                if self.check(TokenType::DotDot) {
                                    if rest.is_some() {
                                        return Err(ParseError {
                                            message: "Component update can contain at most one `..base` entry".to_string(),
                                            line: self.peek().line,
                                            col: self.peek().col,
                                        });
                                    }
                                    self.advance();
                                    rest = Some(Box::new(self.parse_expr()?));
                                    if self.check(TokenType::Comma) {
                                        self.advance();
                                        if !self.check(TokenType::RBrace) {
                                            return Err(ParseError {
                                                message: "Component update `..base` must be the final entry".to_string(),
                                                line: self.peek().line,
                                                col: self.peek().col,
                                            });
                                        }
                                    }
                                    continue;
                                }
                                let fname = self.expect_field_name()?;
                                self.expect(TokenType::Colon)?;
                                let fval = self.parse_expr()?;
                                fields.push((fname, fval));
                                if self.check(TokenType::Comma) {
                                    self.advance();
                                }
                            }
                            self.expect(TokenType::RBrace)?;
                            expr = Expr::ComponentExpr(qualified, fields, rest, comp_span);
                            continue;
                        }
                    }
                }
            } else if self.check(TokenType::LParen) {
                self.advance();
                let mut args = Vec::new();
                while !self.check(TokenType::RParen) {
                    if self.check(TokenType::DotDot) {
                        let span = self.span();
                        self.advance();
                        let spread_expr = self.parse_expr()?;
                        args.push(Expr::Spread(Box::new(spread_expr), span));
                    } else {
                        args.push(self.parse_expr()?);
                    }
                    if self.check(TokenType::Comma) {
                        self.advance();
                    }
                }
                self.expect(TokenType::RParen)?;
                let span = expr.span().clone();

                // DESUGAR Ok(x), Err(x), Some(x), None()
                if let Expr::Ident(name, _) = &expr {
                    if name == "Ok" && args.len() == 1 {
                        expr = Expr::VariantExpr(
                            "Result".to_string(),
                            "Ok".to_string(),
                            vec![("value".to_string(), args.pop().unwrap())],
                            span,
                        );
                        continue;
                    }
                    if name == "Err" && args.len() == 1 {
                        expr = Expr::VariantExpr(
                            "Result".to_string(),
                            "Err".to_string(),
                            vec![("message".to_string(), args.pop().unwrap())],
                            span,
                        );
                        continue;
                    }
                    if name == "Some" && args.len() == 1 {
                        expr = Expr::VariantExpr(
                            "Option".to_string(),
                            "Some".to_string(),
                            vec![("value".to_string(), args.pop().unwrap())],
                            span,
                        );
                        continue;
                    }
                    if name == "None" && args.is_empty() {
                        expr = Expr::VariantExpr(
                            "Option".to_string(),
                            "None".to_string(),
                            vec![],
                            span,
                        );
                        continue;
                    }
                }

                if let Expr::StateRef(machine, state, _) = &expr {
                    if machine == "Result" && state == "Ok" && args.len() == 1 {
                        expr = Expr::VariantExpr(
                            "Result".to_string(),
                            "Ok".to_string(),
                            vec![("value".to_string(), args.pop().unwrap())],
                            span,
                        );
                        continue;
                    }
                    if machine == "Result" && state == "Err" && args.len() == 1 {
                        expr = Expr::VariantExpr(
                            "Result".to_string(),
                            "Err".to_string(),
                            vec![("message".to_string(), args.pop().unwrap())],
                            span,
                        );
                        continue;
                    }
                    if machine == "Option" && state == "Some" && args.len() == 1 {
                        expr = Expr::VariantExpr(
                            "Option".to_string(),
                            "Some".to_string(),
                            vec![("value".to_string(), args.pop().unwrap())],
                            span,
                        );
                        continue;
                    }
                    if machine == "Option" && state == "None" && args.is_empty() {
                        expr = Expr::VariantExpr(
                            "Option".to_string(),
                            "None".to_string(),
                            vec![],
                            span,
                        );
                        continue;
                    }
                }

                expr = Expr::Call(Box::new(expr), args, span);
            } else if self.check(TokenType::LBracket) {
                self.advance();
                let index = self.parse_expr()?;
                self.expect(TokenType::RBracket)?;
                let span = expr.span().clone();
                expr = Expr::Index(Box::new(expr), Box::new(index), span);
            } else if self.check(TokenType::Question) {
                self.advance();
                let span = expr.span().clone();
                expr = Expr::Try(Box::new(expr), span);
            } else {
                break;
            }
        }
        Ok(expr)
    }

    pub(super) fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let tok = self.peek().clone();

        // `.field` accessor shorthand: a one-argument closure that projects
        // the field — `mods |> map(.flat)` instead of
        // `mods |> map(fn(m) { return m.flat })`. Chains too: `.stats.hp`.
        if tok.ty == TokenType::Dot {
            let span = self.span();
            self.advance();
            let param = "__acc".to_string();
            let mut body_expr = Expr::Field(
                Box::new(Expr::Ident(param.clone(), span.clone())),
                self.expect_field_name()?,
                span.clone(),
            );
            while self.check(TokenType::Dot) {
                self.advance();
                body_expr =
                    Expr::Field(Box::new(body_expr), self.expect_field_name()?, span.clone());
            }
            let body = Block {
                id: self.next_id(),
                span: span.clone(),
                stmts: vec![Stmt::Return(ReturnStmt {
                    id: self.next_id(),
                    span: span.clone(),
                    value: Some(body_expr),
                })],
            };
            return Ok(Expr::FnExpr(
                vec![param],
                vec![false],
                vec![None],
                vec![None],
                None,
                body,
                span,
            ));
        }

        match tok.ty {
            TokenType::Int => {
                self.advance();
                Ok(Expr::IntLit(
                    self.token_int_value(&tok, "integer literal")?,
                    Span {
                        line: tok.line,
                        col: tok.col,
                        file: self.file_id,
                    },
                ))
            }
            TokenType::Float => {
                self.advance();
                Ok(Expr::FloatLit(
                    self.token_float_value(&tok, "float literal")?,
                    Span {
                        line: tok.line,
                        col: tok.col,
                        file: self.file_id,
                    },
                ))
            }
            TokenType::String => {
                self.advance();
                let span = Span {
                    line: tok.line,
                    col: tok.col,
                    file: self.file_id,
                };
                let raw = self.token_str_value(&tok, "string literal")?.to_string();
                if raw.contains("${") {
                    let parts = self.parse_dollar_interpolated_string(&raw, tok.line, tok.col)?;
                    if parts.iter().all(|p| matches!(p, FStringPart::Lit(_))) {
                        let s: String = parts
                            .iter()
                            .map(|p| match p {
                                FStringPart::Lit(s) => s.as_str(),
                                _ => "",
                            })
                            .collect();
                        Ok(Expr::StrLit(s, span))
                    } else {
                        Ok(Expr::FStringExpr(parts, span))
                    }
                } else {
                    Ok(Expr::StrLit(raw, span))
                }
            }
            TokenType::True | TokenType::False => {
                self.advance();
                Ok(Expr::BoolLit(
                    self.token_bool_value(&tok, "boolean literal")?,
                    Span {
                        line: tok.line,
                        col: tok.col,
                        file: self.file_id,
                    },
                ))
            }
            TokenType::Nil => {
                self.advance();
                Ok(Expr::NilLit(Span {
                    line: tok.line,
                    col: tok.col,
                    file: self.file_id,
                }))
            }
            TokenType::System => {
                self.advance();
                let start = Span {
                    line: tok.line,
                    col: tok.col,
                    file: self.file_id,
                };
                self.expect(TokenType::DColon)?;
                let mut path = vec![self.expect_ident_text()?];
                while self.check(TokenType::DColon) {
                    self.advance();
                    path.push(self.expect_ident_text()?);
                }
                Ok(Expr::SystemRef(path, start))
            }
            TokenType::Entity => {
                self.advance();
                let span = Span {
                    line: tok.line,
                    col: tok.col,
                    file: self.file_id,
                };
                let name = if self.check(TokenType::LBrace) {
                    None
                } else {
                    let prev = self.suppress_braced_init;
                    self.suppress_braced_init = true;
                    let name_expr = self.parse_expr();
                    self.suppress_braced_init = prev;
                    Some(Box::new(name_expr?))
                };
                self.expect(TokenType::LBrace)?;
                let mut components = Vec::new();
                while !self.check(TokenType::RBrace) {
                    if self.looks_like_component_init() {
                        components.push(ComponentEntry::Init(self.parse_component_init()?));
                    } else {
                        components.push(ComponentEntry::Expr(self.parse_expr()?));
                    }
                    if self.check(TokenType::Comma) {
                        self.advance();
                    }
                }
                self.expect(TokenType::RBrace)?;
                Ok(Expr::EntityLiteral(name, components, span))
            }
            TokenType::Ident | TokenType::State => {
                self.advance();
                let name = self.token_str_value(&tok, "identifier")?.to_string();
                let span = Span {
                    line: tok.line,
                    col: tok.col,
                    file: self.file_id,
                };

                if name == "query" && self.check(TokenType::LBrace) {
                    return self.parse_query_expr(span);
                }

                if self.check(TokenType::DColon) {
                    self.advance();
                    let variant = self.expect_ident_text()?;
                    if self.looks_like_braced_field_init() {
                        self.advance();
                        let mut fields = Vec::new();
                        let mut rest = None;
                        while !self.check(TokenType::RBrace) {
                            if self.check(TokenType::DotDot) {
                                if rest.is_some() {
                                    return Err(ParseError {
                                        message: "Component update can contain at most one `..base` entry".to_string(),
                                        line: self.peek().line,
                                        col: self.peek().col,
                                    });
                                }
                                self.advance();
                                let base = self.parse_expr()?;
                                rest = Some(Box::new(base));
                                if self.check(TokenType::Comma) {
                                    self.advance();
                                    if !self.check(TokenType::RBrace) {
                                        return Err(ParseError {
                                            message:
                                                "Component update `..base` must be the final entry"
                                                    .to_string(),
                                            line: self.peek().line,
                                            col: self.peek().col,
                                        });
                                    }
                                }
                                continue;
                            }
                            let fname = self.expect_field_name()?;
                            self.expect(TokenType::Colon)?;
                            let fval = self.parse_expr()?;
                            fields.push((fname, fval));
                            if self.check(TokenType::Comma) {
                                self.advance();
                            }
                        }
                        self.expect(TokenType::RBrace)?;
                        return Ok(Expr::VariantExpr(name, variant, fields, span));
                    }

                    if name == "Option" && variant == "None" && !self.check(TokenType::LParen) {
                        return Ok(Expr::VariantExpr(
                            "Option".to_string(),
                            "None".to_string(),
                            vec![],
                            span,
                        ));
                    }

                    return Ok(Expr::StateRef(name, variant, span));
                }

                if self.looks_like_braced_field_init() {
                    self.advance();
                    let mut fields = Vec::new();
                    let mut rest = None;
                    while !self.check(TokenType::RBrace) {
                        if self.check(TokenType::DotDot) {
                            if rest.is_some() {
                                return Err(ParseError {
                                    message:
                                        "Component update can contain at most one `..base` entry"
                                            .to_string(),
                                    line: self.peek().line,
                                    col: self.peek().col,
                                });
                            }
                            self.advance();
                            let base = self.parse_expr()?;
                            rest = Some(Box::new(base));
                            if self.check(TokenType::Comma) {
                                self.advance();
                                if !self.check(TokenType::RBrace) {
                                    return Err(ParseError {
                                        message:
                                            "Component update `..base` must be the final entry"
                                                .to_string(),
                                        line: self.peek().line,
                                        col: self.peek().col,
                                    });
                                }
                            }
                            continue;
                        }
                        let fname = self.expect_field_name()?;
                        self.expect(TokenType::Colon)?;
                        let fval = self.parse_expr()?;
                        fields.push((fname, fval));
                        if self.check(TokenType::Comma) {
                            self.advance();
                        }
                    }
                    self.expect(TokenType::RBrace)?;
                    return Ok(Expr::ComponentExpr(name, fields, rest, span));
                }

                if name == "None" && !self.check(TokenType::LParen) {
                    return Ok(Expr::VariantExpr(
                        "Option".to_string(),
                        "None".to_string(),
                        vec![],
                        span,
                    ));
                }

                Ok(Expr::Ident(name, span))
            }
            TokenType::LBracket => {
                self.advance();
                let span = Span {
                    line: tok.line,
                    col: tok.col,
                    file: self.file_id,
                };
                let mut elements = Vec::new();
                while !self.check(TokenType::RBracket) {
                    elements.push(self.parse_expr()?);
                    if self.check(TokenType::Comma) {
                        self.advance();
                    }
                }
                self.expect(TokenType::RBracket)?;
                Ok(Expr::ListLit(elements, span))
            }
            TokenType::LParen => {
                self.advance();
                let span = Span {
                    line: tok.line,
                    col: tok.col,
                    file: self.file_id,
                };
                if self.check(TokenType::RParen) {
                    self.advance();
                    return Ok(Expr::TupleLit(Vec::new(), span));
                }
                let expr = self.parse_expr()?;
                if self.check(TokenType::Comma) {
                    self.advance();
                    let mut exprs = vec![expr];
                    while !self.check(TokenType::RParen) {
                        exprs.push(self.parse_expr()?);
                        if self.check(TokenType::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.expect(TokenType::RParen)?;
                    Ok(Expr::TupleLit(exprs, span))
                } else {
                    self.expect(TokenType::RParen)?;
                    Ok(expr)
                }
            }
            TokenType::Match => {
                let m = self.parse_match_core()?;
                let span = m.span.clone();
                Ok(Expr::MatchExpr(Box::new(m), span))
            }
            TokenType::If => self.parse_if_expr(),
            TokenType::LBrace => self.parse_map_literal(),
            TokenType::FStringStart => self.parse_fstring_expr(),
            TokenType::Fn => self.parse_fn_expr(),
            _ => {
                let err = ParseError {
                    message: format!("Unexpected token {:?} ('{}')", tok.ty, tok.value),
                    line: tok.line,
                    col: tok.col,
                };
                Err(err)
            }
        }
    }

    /// `if cond { a } else { b }` in expression position. Branches are
    /// single expressions and `else` is mandatory (an if-expression
    /// always has a value); chains continue with `else if`.
    fn parse_if_expr(&mut self) -> Result<Expr, ParseError> {
        let span = self.span();
        self.expect(TokenType::If)?;
        let cond = self.parse_expr()?;
        self.expect(TokenType::LBrace)?;
        let then_e = self.parse_expr()?;
        self.expect(TokenType::RBrace)?;
        if !self.check(TokenType::Else) {
            let tok = self.peek();
            return Err(ParseError {
                message:
                    "if-expression requires an `else` branch (every branch must produce a value)"
                        .to_string(),
                line: tok.line,
                col: tok.col,
            });
        }
        self.advance(); // else
        let else_e = if self.check(TokenType::If) {
            self.parse_if_expr()?
        } else {
            self.expect(TokenType::LBrace)?;
            let e = self.parse_expr()?;
            self.expect(TokenType::RBrace)?;
            e
        };
        Ok(Expr::IfExpr(
            Box::new(cond),
            Box::new(then_e),
            Box::new(else_e),
            span,
        ))
    }

    fn parse_map_literal(&mut self) -> Result<Expr, ParseError> {
        let span = self.span();
        self.expect(TokenType::LBrace)?;
        let mut entries = Vec::new();
        while !self.check(TokenType::RBrace) {
            let key = self.parse_expr()?;
            self.expect(TokenType::Colon)?;
            let value = self.parse_expr()?;
            entries.push((key, value));
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RBrace)?;
        Ok(Expr::MapLit(entries, span))
    }

    fn parse_fstring_expr(&mut self) -> Result<Expr, ParseError> {
        let tok = self.advance(); // consume FStringStart
        let span = Span {
            line: tok.line,
            col: tok.col,
            file: self.file_id,
        };
        let mut parts = Vec::new();
        loop {
            match self.peek().ty {
                TokenType::FStringFragment => {
                    let frag = self.advance();
                    let text = self
                        .token_str_value(&frag, "f-string fragment")?
                        .to_string();
                    if !text.is_empty() {
                        parts.push(FStringPart::Lit(text));
                    }
                }
                TokenType::InterpolationStart => {
                    self.advance();
                    let expr = self.parse_expr()?;
                    let end_tok = self.expect(TokenType::InterpolationEnd)?;
                    let spec = end_tok.value.as_str().map(|s| s.to_string());
                    parts.push(FStringPart::Expr(Box::new(expr), spec));
                }
                TokenType::FStringEnd => {
                    self.advance();
                    break;
                }
                TokenType::Eof => {
                    return Err(ParseError {
                        message: "Unterminated f-string".into(),
                        line: tok.line,
                        col: tok.col,
                    });
                }
                _ => {
                    return Err(ParseError {
                        message: format!("Unexpected token in f-string: {:?}", self.peek().ty),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
            }
        }
        Ok(Expr::FStringExpr(parts, span))
    }
}
