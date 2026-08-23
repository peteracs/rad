use super::*;

impl Parser {
    pub(super) fn parse_type(&mut self) -> Result<TypeExpr, ParseError> {
        let base_type = self.parse_single_type()?;
        if self.check(TokenType::Pipe) {
            let mut types = vec![base_type];
            while self.check(TokenType::Pipe) {
                self.advance();
                types.push(self.parse_single_type()?);
            }
            Ok(TypeExpr::Union(types))
        } else {
            Ok(base_type)
        }
    }

    fn parse_single_type(&mut self) -> Result<TypeExpr, ParseError> {
        let tok = self.peek().clone();
        let base = match tok.ty {
            TokenType::System => {
                self.advance();
                "system".to_string()
            }
            // `pure fn(...) -> T`: a fn type whose values must be pure. The
            // only thing `pure` can begin in type position is a fn type.
            TokenType::Pure => {
                self.advance();
                return self.parse_fn_type(FnTypePurity::Pure);
            }
            // `readonly fn(...) -> T`: values may be pure or readonly.
            // `readonly` lexes as a plain identifier, so require the
            // lookahead to `fn` — a type NAMED `readonly` (if anyone ever
            // declares one) still parses through the identifier arm below.
            TokenType::Ident
                if matches!(&tok.value, crate::lexer::TokenValue::Str(s) if s == "readonly")
                    && self.peek_at(1).ty == TokenType::Fn =>
            {
                self.advance();
                return self.parse_fn_type(FnTypePurity::Readonly);
            }
            TokenType::Ident | TokenType::Entity | TokenType::State => {
                self.advance();
                let mut name = if tok.ty == TokenType::Entity {
                    "entity".to_string()
                } else if tok.ty == TokenType::State {
                    "state".to_string()
                } else {
                    self.token_str_value(&tok, "type name")?.to_string()
                };
                if self.check(TokenType::Dot) {
                    self.advance();
                    let member = self.expect_ident_text()?;
                    name = format!("{}.{}", name, member);
                }
                name
            }
            TokenType::Nil => {
                self.advance();
                "nil".to_string()
            }
            TokenType::Fn => {
                return self.parse_fn_type(FnTypePurity::Default);
            }
            TokenType::LParen => {
                self.advance();
                let mut types = Vec::new();
                while !self.check(TokenType::RParen) {
                    types.push(self.parse_type()?);
                    if self.check(TokenType::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(TokenType::RParen)?;
                return Ok(TypeExpr::Tuple(types));
            }
            _ => {
                let hint = if tok.ty == TokenType::String {
                    " (hint: event fields use `field: Type`, not `field: \"default\"`; for bare fields use `field` without a colon)"
                } else {
                    ""
                };
                return Err(ParseError {
                    message: format!(
                        "Expected type name, got {:?} ('{}'){}",
                        tok.ty, tok.value, hint
                    ),
                    line: tok.line,
                    col: tok.col,
                });
            }
        };
        if self.check(TokenType::Lt) {
            self.advance();
            let mut args = Vec::new();
            while !self.check(TokenType::Gt) {
                args.push(self.parse_type()?);
                if self.check(TokenType::Comma) {
                    self.advance();
                }
            }
            self.expect(TokenType::Gt)?;
            Ok(TypeExpr::Generic(base, args))
        } else {
            Ok(TypeExpr::Named(base))
        }
    }

    fn parse_fn_type(&mut self, purity: FnTypePurity) -> Result<TypeExpr, ParseError> {
        self.expect(TokenType::Fn)?;
        if !self.check(TokenType::LParen) {
            let tok = self.peek().clone();
            return Err(ParseError {
                message: "Expected '(' after 'fn' in type position. Bare 'fn' is not a valid type — write e.g. fn() or fn(int) -> str".to_string(),
                line: tok.line,
                col: tok.col,
            });
        }
        self.expect(TokenType::LParen)?;
        let mut params = Vec::new();
        while !self.check(TokenType::RParen) {
            params.push(self.parse_type()?);
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RParen)?;
        let ret = if self.check(TokenType::Arrow) {
            self.advance();
            self.parse_type()?
        } else {
            TypeExpr::Named("void".to_string())
        };
        Ok(TypeExpr::FnType(params, Box::new(ret), purity))
    }
}
