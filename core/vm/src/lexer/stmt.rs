use super::*;

impl<'a> Lexer<'a> {
    fn push_simple_token(
        tokens: &mut Vec<Token>,
        ty: TokenType,
        line: u32,
        col: u32,
        start_pos: usize,
        end_pos: usize,
    ) {
        tokens.push(Token {
            ty,
            value: TokenValue::None,
            line,
            col,
            span: (start_pos, end_pos),
        });
    }

    /// Scans the body of an f-string (everything between the opening and closing quotes).
    ///
    /// **Interpolation rules differ by quoting style:**
    ///
    /// | Syntax            | `{expr}` | `${expr}` | Bare `{` / `}` |
    /// |-------------------|----------|-----------|-----------------|
    /// | `f"..."`          | interp   | interp    | must escape `{{`/`}}` or error |
    /// | `f"""..."""`       | literal  | interp    | literal (no escaping needed)   |
    ///
    /// Triple-quoted f-strings treat bare braces as literal text so they can embed
    /// code/JSON/C without escaping every brace. Use `${expr}` to interpolate.
    fn scan_fstring_content(
        &mut self,
        triple: bool,
        tokens: &mut Vec<Token>,
        errors: &mut Vec<LexerError>,
    ) {
        let mut text = String::new();
        let frag_line = self.line;
        let frag_col = self.col;
        let frag_start = self.pos;

        loop {
            if self.pos >= self.source.len() {
                if !text.is_empty() {
                    tokens.push(Token {
                        ty: TokenType::FStringFragment,
                        value: TokenValue::Str(text),
                        line: frag_line,
                        col: frag_col,
                        span: (frag_start, self.pos),
                    });
                }
                errors.push(LexerError {
                    message: if triple {
                        "Unterminated triple-quoted f-string".into()
                    } else {
                        "Unterminated f-string".into()
                    },
                    line: frag_line,
                    col: frag_col,
                });
                self.mode_stack.pop();
                return;
            }

            let ch = self.peek();

            if !triple && ch == '"' {
                if !text.is_empty() {
                    tokens.push(Token {
                        ty: TokenType::FStringFragment,
                        value: TokenValue::Str(text),
                        line: frag_line,
                        col: frag_col,
                        span: (frag_start, self.pos),
                    });
                }
                let end_line = self.line;
                let end_col = self.col;
                let end_start = self.pos;
                self.advance();
                Self::push_simple_token(
                    tokens,
                    TokenType::FStringEnd,
                    end_line,
                    end_col,
                    end_start,
                    self.pos,
                );
                self.mode_stack.pop();
                return;
            }

            if triple && ch == '"' {
                let rest = &self.source[self.pos..];
                if rest.starts_with("\"\"\"") {
                    if !text.is_empty() {
                        tokens.push(Token {
                            ty: TokenType::FStringFragment,
                            value: TokenValue::Str(text),
                            line: frag_line,
                            col: frag_col,
                            span: (frag_start, self.pos),
                        });
                    }
                    let end_line = self.line;
                    let end_col = self.col;
                    let end_start = self.pos;
                    self.advance();
                    self.advance();
                    self.advance();
                    Self::push_simple_token(
                        tokens,
                        TokenType::FStringEnd,
                        end_line,
                        end_col,
                        end_start,
                        self.pos,
                    );
                    self.mode_stack.pop();
                    return;
                }
            }

            if ch == '$' && self.pos + 1 < self.source.len() {
                let next = self.source[self.pos + 1..].chars().next().unwrap_or('\0');
                if next == '{' {
                    if !text.is_empty() {
                        tokens.push(Token {
                            ty: TokenType::FStringFragment,
                            value: TokenValue::Str(text),
                            line: frag_line,
                            col: frag_col,
                            span: (frag_start, self.pos),
                        });
                    }
                    let interp_line = self.line;
                    let interp_col = self.col;
                    let interp_start = self.pos;
                    self.advance(); // $
                    self.advance(); // {
                    Self::push_simple_token(
                        tokens,
                        TokenType::InterpolationStart,
                        interp_line,
                        interp_col,
                        interp_start,
                        self.pos,
                    );
                    self.mode_stack
                        .push(LexerMode::Interpolation { brace_depth: 1 });
                    return;
                }
            }

            // In triple-quoted f-strings, bare `{` is literal text (not interpolation).
            // Only `${` triggers interpolation — see the `$` handler above.
            // This lets users embed C/JSON/JS without escaping every brace.
            if !triple && ch == '{' {
                if self.pos + 1 < self.source.len() {
                    let next = self.source[self.pos + 1..].chars().next().unwrap_or('\0');
                    if next == '{' {
                        text.push('{');
                        self.advance();
                        self.advance();
                        continue;
                    }
                }
                if !text.is_empty() {
                    tokens.push(Token {
                        ty: TokenType::FStringFragment,
                        value: TokenValue::Str(text),
                        line: frag_line,
                        col: frag_col,
                        span: (frag_start, self.pos),
                    });
                }
                let interp_line = self.line;
                let interp_col = self.col;
                let interp_start = self.pos;
                self.advance(); // {
                Self::push_simple_token(
                    tokens,
                    TokenType::InterpolationStart,
                    interp_line,
                    interp_col,
                    interp_start,
                    self.pos,
                );
                self.mode_stack
                    .push(LexerMode::Interpolation { brace_depth: 1 });
                return;
            }

            // Likewise, bare `}` is literal in triple mode (no error for unescaped `}`).
            if !triple && ch == '}' {
                if self.pos + 1 < self.source.len() {
                    let next = self.source[self.pos + 1..].chars().next().unwrap_or('\0');
                    if next == '}' {
                        text.push('}');
                        self.advance();
                        self.advance();
                        continue;
                    }
                }
                if !text.is_empty() {
                    tokens.push(Token {
                        ty: TokenType::FStringFragment,
                        value: TokenValue::Str(text),
                        line: frag_line,
                        col: frag_col,
                        span: (frag_start, self.pos),
                    });
                }
                errors.push(LexerError {
                    message: "Unescaped '}' in f-string; use '}}' for a literal brace".into(),
                    line: self.line,
                    col: self.col,
                });
                self.advance();
                return;
            }

            if ch == '\\' {
                self.advance();
                if self.pos >= self.source.len() {
                    if !text.is_empty() {
                        tokens.push(Token {
                            ty: TokenType::FStringFragment,
                            value: TokenValue::Str(text),
                            line: frag_line,
                            col: frag_col,
                            span: (frag_start, self.pos),
                        });
                    }
                    errors.push(LexerError {
                        message: if triple {
                            "Unterminated triple-quoted f-string".into()
                        } else {
                            "Unterminated f-string".into()
                        },
                        line: frag_line,
                        col: frag_col,
                    });
                    self.mode_stack.pop();
                    return;
                }
                let esc = self.advance();
                match esc {
                    'n' => text.push('\n'),
                    't' => text.push('\t'),
                    'r' => text.push('\r'),
                    '\\' => text.push('\\'),
                    '"' => text.push('"'),
                    '0' => text.push('\0'),
                    '$' => text.push('$'),
                    '{' => text.push('{'),
                    '}' => text.push('}'),
                    _ => {
                        text.push('\\');
                        text.push(esc);
                    }
                }
                continue;
            }

            text.push(self.advance());
        }
    }

    pub fn tokenize(&mut self) -> (Vec<Token>, Vec<LexerError>) {
        let mut tokens = Vec::new();
        let mut errors = Vec::new();
        loop {
            let mode = self.current_mode();

            if let LexerMode::FString { triple } = mode {
                self.scan_fstring_content(triple, &mut tokens, &mut errors);
                continue;
            }

            if let LexerMode::Interpolation { brace_depth } = mode {
                if let Err(e) = self.skip_ws_comments(Some(&mut tokens)) {
                    errors.push(e);
                    if self.pos < self.source.len() {
                        self.advance();
                    }
                    continue;
                }
                if self.pos >= self.source.len() {
                    break;
                }
                let line = self.line;
                let col = self.col;
                let start_pos = self.pos;
                let ch = self.peek();

                if ch == '{' {
                    self.advance();
                    if let Some(LexerMode::Interpolation {
                        brace_depth: ref mut d,
                    }) = self.mode_stack.last_mut()
                    {
                        *d += 1;
                    }
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::LBrace,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                    continue;
                }
                // `::` is the path separator (`Door::Locked`, `system::Name`),
                // not the format-spec delimiter: fall through to normal
                // tokenization so it lexes as DColon. Only a single `:`
                // starts a spec.
                let next_is_colon = ch == ':'
                    && self.source[self.pos + ':'.len_utf8()..]
                        .chars()
                        .next()
                        .is_some_and(|c| c == ':');
                if ch == ':' && !next_is_colon && brace_depth <= 1 {
                    self.advance(); // consume ':'
                    let spec_start = self.pos;
                    let mut depth = 1u32;
                    while self.pos < self.source.len() && depth > 0 {
                        let sc = self.peek();
                        if sc == '{' {
                            depth += 1;
                            self.advance();
                        } else if sc == '}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            self.advance();
                        } else {
                            self.advance();
                        }
                    }
                    let spec = self.source[spec_start..self.pos].to_string();
                    if self.pos < self.source.len() && self.peek() == '}' {
                        self.advance();
                    }
                    tokens.push(Token {
                        ty: TokenType::InterpolationEnd,
                        value: if spec.is_empty() {
                            TokenValue::None
                        } else {
                            TokenValue::Str(spec)
                        },
                        line,
                        col,
                        span: (start_pos, self.pos),
                    });
                    self.mode_stack.pop();
                    continue;
                }
                if ch == '}' {
                    if brace_depth <= 1 {
                        self.advance();
                        Self::push_simple_token(
                            &mut tokens,
                            TokenType::InterpolationEnd,
                            line,
                            col,
                            start_pos,
                            self.pos,
                        );
                        self.mode_stack.pop();
                        continue;
                    } else {
                        self.advance();
                        if let Some(LexerMode::Interpolation {
                            brace_depth: ref mut d,
                        }) = self.mode_stack.last_mut()
                        {
                            *d -= 1;
                        }
                        Self::push_simple_token(
                            &mut tokens,
                            TokenType::RBrace,
                            line,
                            col,
                            start_pos,
                            self.pos,
                        );
                        continue;
                    }
                }
                if ch == '\\' && self.pos + 1 < self.source.len() {
                    let next_ch = self.source[self.pos + 1..].chars().next().unwrap_or('\0');
                    if next_ch == '"' {
                        self.advance(); // consume '\'
                        self.advance(); // consume '"'
                        let mut text = String::new();
                        loop {
                            if self.pos >= self.source.len() {
                                errors.push(LexerError {
                                    message: "Unterminated \\\"...\\\" string in interpolation"
                                        .into(),
                                    line,
                                    col,
                                });
                                break;
                            }
                            let sc = self.peek();
                            if sc == '\\' && self.pos + 1 < self.source.len() {
                                let after =
                                    self.source[self.pos + 1..].chars().next().unwrap_or('\0');
                                if after == '"' {
                                    self.advance(); // consume '\'
                                    self.advance(); // consume '"'
                                    break;
                                }
                                self.advance(); // consume '\'
                                let esc = self.advance();
                                match esc {
                                    'n' => text.push('\n'),
                                    't' => text.push('\t'),
                                    'r' => text.push('\r'),
                                    '\\' => text.push('\\'),
                                    '0' => text.push('\0'),
                                    _ => {
                                        text.push('\\');
                                        text.push(esc);
                                    }
                                }
                                continue;
                            }
                            if sc == '\n' {
                                errors.push(LexerError {
                                    message: "Unterminated \\\"...\\\" string in interpolation"
                                        .into(),
                                    line,
                                    col,
                                });
                                break;
                            }
                            text.push(self.advance());
                        }
                        tokens.push(Token {
                            ty: TokenType::String,
                            value: TokenValue::Str(text),
                            line,
                            col,
                            span: (start_pos, self.pos),
                        });
                        continue;
                    }
                }
                // Fall through to normal tokenization below
            } else {
                // Normal mode
                if let Err(e) = self.skip_ws_comments(Some(&mut tokens)) {
                    errors.push(e);
                    if self.pos < self.source.len() {
                        self.advance();
                    }
                    continue;
                }
                if self.pos >= self.source.len() {
                    break;
                }
            }

            let line = self.line;
            let col = self.col;
            let start_pos = self.pos;
            let ch = self.peek();

            if ch == '"' {
                match self.read_string(self.pos) {
                    Ok(token) => tokens.push(token),
                    Err(e) => {
                        errors.push(e);
                        Self::push_simple_token(
                            &mut tokens,
                            TokenType::Error,
                            line,
                            col,
                            start_pos,
                            self.pos,
                        );
                    }
                }
            } else if ch.is_ascii_digit() || (ch == '.' && self.peek_next().is_ascii_digit()) {
                match self.read_number(self.pos) {
                    Ok(token) => tokens.push(token),
                    Err(e) => {
                        errors.push(e);
                        Self::push_simple_token(
                            &mut tokens,
                            TokenType::Error,
                            line,
                            col,
                            start_pos,
                            self.pos,
                        );
                    }
                }
            } else if ch.is_alphabetic() || ch == '_' {
                match self.read_ident(self.pos) {
                    Ok(token) => tokens.push(token),
                    Err(e) => {
                        errors.push(e);
                        Self::push_simple_token(
                            &mut tokens,
                            TokenType::Error,
                            line,
                            col,
                            start_pos,
                            self.pos,
                        );
                    }
                }
            } else if ch == '{' {
                self.advance();
                Self::push_simple_token(
                    &mut tokens,
                    TokenType::LBrace,
                    line,
                    col,
                    start_pos,
                    self.pos,
                );
            } else if ch == '}' {
                self.advance();
                Self::push_simple_token(
                    &mut tokens,
                    TokenType::RBrace,
                    line,
                    col,
                    start_pos,
                    self.pos,
                );
            } else if ch == '(' {
                self.advance();
                Self::push_simple_token(
                    &mut tokens,
                    TokenType::LParen,
                    line,
                    col,
                    start_pos,
                    self.pos,
                );
            } else if ch == ')' {
                self.advance();
                Self::push_simple_token(
                    &mut tokens,
                    TokenType::RParen,
                    line,
                    col,
                    start_pos,
                    self.pos,
                );
            } else if ch == '[' {
                self.advance();
                Self::push_simple_token(
                    &mut tokens,
                    TokenType::LBracket,
                    line,
                    col,
                    start_pos,
                    self.pos,
                );
            } else if ch == ']' {
                self.advance();
                Self::push_simple_token(
                    &mut tokens,
                    TokenType::RBracket,
                    line,
                    col,
                    start_pos,
                    self.pos,
                );
            } else if ch == ',' {
                self.advance();
                Self::push_simple_token(
                    &mut tokens,
                    TokenType::Comma,
                    line,
                    col,
                    start_pos,
                    self.pos,
                );
            } else if ch == '.' {
                self.advance();
                if self.peek() == '.' {
                    self.advance();
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::DotDot,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                } else {
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::Dot,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                }
            } else if ch == ':' {
                self.advance();
                if self.peek() == ':' {
                    self.advance();
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::DColon,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                } else {
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::Colon,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                }
            } else if ch == '-' {
                self.advance();
                if self.peek() == '>' {
                    self.advance();
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::Arrow,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                } else {
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::Minus,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                }
            } else if ch == '=' {
                self.advance();
                if self.peek() == '=' {
                    self.advance();
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::Eq,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                } else if self.peek() == '>' {
                    self.advance();
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::FatArrow,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                } else {
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::Assign,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                }
            } else if ch == '|' {
                self.advance();
                if self.peek() == '|' {
                    self.advance();
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::Or,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                } else if self.peek() == '>' {
                    self.advance();
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::PipeOp,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                } else {
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::Pipe,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                }
            } else if ch == '?' {
                self.advance();
                Self::push_simple_token(
                    &mut tokens,
                    TokenType::Question,
                    line,
                    col,
                    start_pos,
                    self.pos,
                );
            } else if ch == '!' {
                self.advance();
                if self.peek() == '=' {
                    self.advance();
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::Neq,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                } else {
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::Bang,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                }
            } else if ch == '<' {
                self.advance();
                if self.peek() == '=' {
                    self.advance();
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::Lte,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                } else if self.peek() == '<' {
                    self.advance();
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::LessLess,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                } else {
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::Lt,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                }
            } else if ch == '>' {
                self.advance();
                if self.peek() == '=' {
                    self.advance();
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::Gte,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                } else {
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::Gt,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                }
            } else if ch == '+' {
                self.advance();
                Self::push_simple_token(
                    &mut tokens,
                    TokenType::Plus,
                    line,
                    col,
                    start_pos,
                    self.pos,
                );
            } else if ch == '*' {
                self.advance();
                Self::push_simple_token(
                    &mut tokens,
                    TokenType::Star,
                    line,
                    col,
                    start_pos,
                    self.pos,
                );
            } else if ch == '/' {
                self.advance();
                Self::push_simple_token(
                    &mut tokens,
                    TokenType::Slash,
                    line,
                    col,
                    start_pos,
                    self.pos,
                );
            } else if ch == '%' {
                self.advance();
                Self::push_simple_token(
                    &mut tokens,
                    TokenType::Percent,
                    line,
                    col,
                    start_pos,
                    self.pos,
                );
            } else if ch == '&' {
                self.advance();
                if self.peek() == '&' {
                    self.advance();
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::And,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                } else {
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::Amp,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                }
            } else if ch == '^' {
                self.advance();
                Self::push_simple_token(
                    &mut tokens,
                    TokenType::Caret,
                    line,
                    col,
                    start_pos,
                    self.pos,
                );
            } else if ch == '~' {
                self.advance();
                Self::push_simple_token(
                    &mut tokens,
                    TokenType::Tilde,
                    line,
                    col,
                    start_pos,
                    self.pos,
                );
            } else if ch == '\\' {
                if self.peek_next() == '\\' {
                    let mut text = String::new();
                    let mut first = true;
                    loop {
                        let line_start = self.pos + 2; // skip \\
                        self.advance();
                        self.advance();
                        while self.pos < self.source.len() && self.peek() != '\n' {
                            self.advance();
                        }
                        if !first {
                            text.push('\n');
                        }
                        text.push_str(&self.source[line_start..self.pos]);
                        first = false;

                        let saved_pos = self.pos;
                        let saved_line = self.line;
                        let saved_col = self.col;

                        while self.pos < self.source.len() {
                            let c = self.peek();
                            if matches!(c, ' ' | '\r' | '\t' | '\n') {
                                self.advance();
                            } else {
                                break;
                            }
                        }

                        if self.peek() == '\\' && self.peek_next() == '\\' {
                            continue;
                        } else {
                            self.pos = saved_pos;
                            self.line = saved_line;
                            self.col = saved_col;
                            break;
                        }
                    }
                    tokens.push(Token {
                        ty: TokenType::String,
                        value: TokenValue::Str(text),
                        line,
                        col,
                        span: (start_pos, self.pos),
                    });
                } else {
                    errors.push(LexerError {
                        message: format!("Unexpected character '{}'", ch),
                        line,
                        col,
                    });
                    self.advance();
                    Self::push_simple_token(
                        &mut tokens,
                        TokenType::Error,
                        line,
                        col,
                        start_pos,
                        self.pos,
                    );
                }
            } else {
                errors.push(LexerError {
                    message: format!("Unexpected character '{}'", ch),
                    line,
                    col,
                });
                self.advance();
                Self::push_simple_token(
                    &mut tokens,
                    TokenType::Error,
                    line,
                    col,
                    start_pos,
                    self.pos,
                );
            }
        }
        tokens.push(Token {
            ty: TokenType::Eof,
            value: TokenValue::None,
            line: self.line,
            col: self.col,
            span: (0, 0),
        });
        (tokens, errors)
    }
}
