use crate::ast::*;
use crate::lexer::{reserved_keyword_rename_hints, Token, TokenType};

mod causal;
mod decl;
mod expr;
mod recovery;
mod stmt;
#[cfg(test)]
mod tests;
mod types;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub line: u32,
    pub col: u32,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[line {}, col {}] {}", self.line, self.col, self.message)
    }
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    errors: Vec<ParseError>,
    id_gen: NodeIdGen,
    file_id: Option<FileId>,
    options: ParserOptions,
    suppress_braced_init: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ParserOptions {
    pub compat_v0_5_dx: bool,
}

impl Parser {
    fn is_identifier_token(&self, ty: TokenType) -> bool {
        matches!(ty, TokenType::Ident | TokenType::State | TokenType::Entity)
    }

    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            errors: Vec::new(),
            id_gen: NodeIdGen::new(),
            file_id: None,
            options: ParserOptions::default(),
            suppress_braced_init: false,
        }
    }

    pub fn with_options(mut self, options: ParserOptions) -> Self {
        self.options = options;
        self
    }

    pub fn with_file_id(mut self, file_id: FileId) -> Self {
        self.file_id = Some(file_id);
        self
    }

    fn next_id(&mut self) -> NodeId {
        self.id_gen.next()
    }

    pub fn push_error(&mut self, err: ParseError) {
        if err.message.contains("got Error") || err.message.contains("Unexpected token Error") {
            // Skip cascading errors from lexer
            return;
        }
        self.errors.push(err);
    }

    pub fn errors(&self) -> &[ParseError] {
        &self.errors
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek_at(&self, offset: usize) -> &Token {
        let idx = self.pos + offset;
        if idx >= self.tokens.len() {
            self.tokens
                .last()
                .expect("parser expects EOF sentinel token in stream")
        } else {
            &self.tokens[idx]
        }
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        self.pos += 1;
        tok
    }

    fn expect(&mut self, ty: TokenType) -> Result<Token, ParseError> {
        let tok = self.peek().clone();
        if tok.ty != ty {
            return Err(ParseError {
                message: format!("Expected {:?}, got {:?} ('{}')", ty, tok.ty, tok.value),
                line: tok.line,
                col: tok.col,
            });
        }
        Ok(self.advance())
    }

    fn token_str_value<'a>(&self, tok: &'a Token, expected: &str) -> Result<&'a str, ParseError> {
        tok.value.as_str().ok_or_else(|| ParseError {
            message: format!("Expected {} token value, got {:?}", expected, tok.value),
            line: tok.line,
            col: tok.col,
        })
    }

    fn token_int_value(&self, tok: &Token, expected: &str) -> Result<i64, ParseError> {
        tok.value.as_int().ok_or_else(|| ParseError {
            message: format!("Expected {} token value, got {:?}", expected, tok.value),
            line: tok.line,
            col: tok.col,
        })
    }

    fn token_float_value(&self, tok: &Token, expected: &str) -> Result<f64, ParseError> {
        tok.value.as_float().ok_or_else(|| ParseError {
            message: format!("Expected {} token value, got {:?}", expected, tok.value),
            line: tok.line,
            col: tok.col,
        })
    }

    fn token_bool_value(&self, tok: &Token, expected: &str) -> Result<bool, ParseError> {
        tok.value.as_bool().ok_or_else(|| ParseError {
            message: format!("Expected {} token value, got {:?}", expected, tok.value),
            line: tok.line,
            col: tok.col,
        })
    }

    fn expect_ident_text(&mut self) -> Result<String, ParseError> {
        let tok = self.peek().clone();
        if !self.is_identifier_token(tok.ty) {
            let is_keyword = matches!(
                tok.ty,
                TokenType::Component
                    | TokenType::Entity
                    | TokenType::State
                    | TokenType::System
                    | TokenType::Event
                    | TokenType::On
                    | TokenType::Fn
                    | TokenType::Pure
                    | TokenType::If
                    | TokenType::Else
                    | TokenType::While
                    | TokenType::For
                    | TokenType::In
                    | TokenType::Return
                    | TokenType::Break
                    | TokenType::Continue
                    | TokenType::Let
                    | TokenType::Mut
                    | TokenType::Use
                    | TokenType::Type
                    | TokenType::Match
                    | TokenType::Emit
                    | TokenType::Schedule
                    | TokenType::And
                    | TokenType::Or
                    | TokenType::Not
                    | TokenType::Nil
                    | TokenType::True
                    | TokenType::False
                    | TokenType::Rec
                    | TokenType::Indexed
                    | TokenType::Unique
            );
            let message = if is_keyword {
                let keyword_text = tok.value.to_string();
                let hints = reserved_keyword_rename_hints(&keyword_text);
                let hint_text = if hints.is_empty() {
                    String::new()
                } else {
                    format!(" Try: {}", hints.join(", "))
                };
                format!(
                    "Expected identifier, got reserved keyword '{}' (rename this symbol).{}",
                    keyword_text, hint_text
                )
            } else {
                format!("Expected identifier, got {:?} ('{}')", tok.ty, tok.value)
            };
            return Err(ParseError {
                message,
                line: tok.line,
                col: tok.col,
            });
        }
        let tok = self.advance();
        Ok(self.token_str_value(&tok, "identifier")?.to_string())
    }

    /// Field-name positions (component/struct/resource/event field decls,
    /// literal fields before `:`, `.field` access, update-block targets) are
    /// grammatically unambiguous, so reserved words like `on` or `state` are
    /// accepted as plain names there. Literal keywords (`true`/`false`/`nil`)
    /// stay reserved.
    fn expect_field_name(&mut self) -> Result<String, ParseError> {
        let tok = self.peek().clone();
        if !self.is_identifier_token(tok.ty)
            && !matches!(tok.ty, TokenType::True | TokenType::False | TokenType::Nil)
        {
            if let Some(word) = tok.value.as_str() {
                if crate::lexer::keyword_type_of(word) == Some(tok.ty) {
                    self.advance();
                    return Ok(word.to_string());
                }
            }
        }
        self.expect_ident_text()
    }

    fn expect_string_text(&mut self) -> Result<String, ParseError> {
        let tok = self.expect(TokenType::String)?;
        Ok(self.token_str_value(&tok, "string literal")?.to_string())
    }

    fn check(&self, ty: TokenType) -> bool {
        self.peek().ty == ty
    }

    fn check_any(&self, types: &[TokenType]) -> bool {
        types.contains(&self.peek().ty)
    }

    fn check_ident_text(&self, text: &str) -> bool {
        let tok = self.peek();
        self.is_identifier_token(tok.ty) && tok.value.as_str() == Some(text)
    }

    fn synchronize(&mut self, recovery_tokens: &[TokenType]) {
        while !self.check(TokenType::Eof) {
            if self.check_any(recovery_tokens) {
                break;
            }
            self.advance();
        }
    }

    pub(crate) fn looks_like_component_init(&self) -> bool {
        let cur = self.peek().ty;
        if !self.is_identifier_token(cur) {
            return false;
        }
        let next = self.peek_at(1).ty;
        matches!(next, TokenType::LBrace | TokenType::Dot | TokenType::DColon)
    }

    fn looks_like_braced_field_init(&self) -> bool {
        if self.suppress_braced_init {
            return false;
        }
        if !self.check(TokenType::LBrace) {
            return false;
        }
        let first = self.peek_at(1);
        let n1 = first.ty;
        let n2 = self.peek_at(2).ty;
        // Keyword field names (`Style { on: 1 }`) count as field starts —
        // mirror expect_field_name's acceptance rule.
        let n1_field_name = self.is_identifier_token(n1)
            || (!matches!(n1, TokenType::True | TokenType::False | TokenType::Nil)
                && first
                    .value
                    .as_str()
                    .is_some_and(|w| crate::lexer::keyword_type_of(w) == Some(n1)));
        n1 == TokenType::RBrace
            || n1 == TokenType::DotDot
            || (n1_field_name && n2 == TokenType::Colon)
    }

    fn span(&self) -> Span {
        let t = self.peek();
        Span {
            line: t.line,
            col: t.col,
            file: self.file_id,
        }
    }

    fn compat_v0_5_dx_enabled(&self) -> bool {
        self.options.compat_v0_5_dx
    }

    fn ensure_fn_param_alignment(
        &self,
        span: &Span,
        params_len: usize,
        param_types_len: usize,
    ) -> Result<(), ParseError> {
        if params_len != param_types_len {
            return Err(ParseError {
                message: format!(
                    "Internal parser invariant violated: function has {} params but {} type annotations",
                    params_len, param_types_len
                ),
                line: span.line,
                col: span.col,
            });
        }
        Ok(())
    }
}
