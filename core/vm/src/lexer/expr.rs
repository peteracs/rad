use super::*;

impl<'a> Lexer<'a> {
    pub(super) fn peek(&self) -> char {
        self.source[self.pos..].chars().next().unwrap_or('\0')
    }

    pub(super) fn peek_next(&self) -> char {
        let mut chars = self.source[self.pos..].chars();
        chars.next();
        chars.next().unwrap_or('\0')
    }

    pub(super) fn advance(&mut self) -> char {
        let ch = self.peek();
        if ch == '\0' {
            return '\0';
        }
        self.pos += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        ch
    }

    pub(super) fn skip_ws_comments(
        &mut self,
        mut tokens: Option<&mut Vec<Token>>,
    ) -> Result<(), LexerError> {
        loop {
            let start_pos = self.pos;
            let start_line = self.line;
            let start_col = self.col;

            while self.pos < self.source.len() && self.peek().is_whitespace() {
                self.advance();
            }
            if self.pos > start_pos {
                if let Some(ref mut toks) = tokens {
                    if self.preserve_comments {
                        toks.push(Token {
                            ty: TokenType::Whitespace,
                            value: TokenValue::Str(self.source[start_pos..self.pos].to_string()),
                            line: start_line,
                            col: start_col,
                            span: (start_pos, self.pos),
                        });
                    }
                }
            }

            if self.pos + 1 < self.source.len() && self.peek() == '/' && self.peek_next() == '/' {
                let c_start = self.pos;
                let c_line = self.line;
                let c_col = self.col;
                while self.pos < self.source.len() && self.peek() != '\n' {
                    self.advance();
                }
                if let Some(ref mut toks) = tokens {
                    if self.preserve_comments {
                        toks.push(Token {
                            ty: TokenType::Comment,
                            value: TokenValue::Str(self.source[c_start..self.pos].to_string()),
                            line: c_line,
                            col: c_col,
                            span: (c_start, self.pos),
                        });
                    }
                }
                continue;
            }
            if self.pos + 1 < self.source.len() && self.peek() == '/' && self.peek_next() == '*' {
                let c_start = self.pos;
                let c_line = self.line;
                let c_col = self.col;
                self.advance();
                self.advance();
                let mut depth = 1u32;
                while self.pos < self.source.len() && depth > 0 {
                    if self.peek() == '/' && self.peek_next() == '*' {
                        self.advance();
                        self.advance();
                        depth += 1;
                    } else if self.peek() == '*' && self.peek_next() == '/' {
                        self.advance();
                        self.advance();
                        depth -= 1;
                    } else {
                        self.advance();
                    }
                }
                if depth > 0 {
                    return Err(LexerError {
                        message: "Unterminated block comment".into(),
                        line: c_line,
                        col: c_col,
                    });
                }
                if let Some(ref mut toks) = tokens {
                    if self.preserve_comments {
                        toks.push(Token {
                            ty: TokenType::Comment,
                            value: TokenValue::Str(self.source[c_start..self.pos].to_string()),
                            line: c_line,
                            col: c_col,
                            span: (c_start, self.pos),
                        });
                    }
                }
                continue;
            }
            break;
        }
        Ok(())
    }

    pub(super) fn read_string(&mut self, start_pos: usize) -> Result<Token, LexerError> {
        let line = self.line;
        let col = self.col;
        self.advance(); // consume opening "
        let mut text = String::new();
        loop {
            if self.pos >= self.source.len() {
                return Err(LexerError {
                    message: "Unterminated string literal".into(),
                    line,
                    col,
                });
            }
            let ch = self.peek();
            if ch == '\n' {
                return Err(LexerError {
                    message: "Unterminated string literal".into(),
                    line,
                    col,
                });
            }
            if ch == '"' {
                self.advance();
                break;
            }
            if ch == '\\' {
                self.advance();
                if self.pos >= self.source.len() {
                    return Err(LexerError {
                        message: "Unterminated string literal".into(),
                        line,
                        col,
                    });
                }
                let esc = self.advance();
                match esc {
                    'n' => text.push('\n'),
                    't' => text.push('\t'),
                    'r' => text.push('\r'),
                    '\\' => text.push('\\'),
                    '"' => text.push('"'),
                    '0' => text.push('\0'),
                    _ => {
                        text.push('\\');
                        text.push(esc);
                    }
                }
            } else {
                text.push(self.advance());
            }
        }
        Ok(Token {
            ty: TokenType::String,
            value: TokenValue::Str(text),
            line,
            col,
            span: (start_pos, self.pos),
        })
    }

    pub(super) fn read_number(&mut self, start_pos: usize) -> Result<Token, LexerError> {
        let line = self.line;
        let col = self.col;
        let mut text = String::new();
        let mut is_float = false;

        if self.peek() == '.' {
            is_float = true;
            text.push('0');
            text.push(self.advance());
            while self.pos < self.source.len() && self.peek().is_ascii_digit() {
                text.push(self.advance());
            }
        } else {
            while self.pos < self.source.len() && self.peek().is_ascii_digit() {
                text.push(self.advance());
            }
            if self.pos < self.source.len() && self.peek() == '.' {
                let next = self.peek_next();
                if next == '.' || next.is_alphabetic() || next == '_' {
                    let val: i64 = text.parse().unwrap();
                    return Ok(Token {
                        ty: TokenType::Int,
                        value: TokenValue::IntVal(val),
                        line,
                        col,
                        span: (start_pos, self.pos),
                    });
                }
                is_float = true;
                text.push(self.advance());
                while self.pos < self.source.len() && self.peek().is_ascii_digit() {
                    text.push(self.advance());
                }
            }
        }

        if self.pos < self.source.len() && (self.peek() == 'e' || self.peek() == 'E') {
            is_float = true;
            text.push(self.advance());
            if self.pos < self.source.len() && (self.peek() == '+' || self.peek() == '-') {
                text.push(self.advance());
            }
            if self.pos >= self.source.len() || !self.peek().is_ascii_digit() {
                return Err(LexerError {
                    message: format!("Invalid float literal '{}'", text),
                    line,
                    col,
                });
            }
            while self.pos < self.source.len() && self.peek().is_ascii_digit() {
                text.push(self.advance());
            }
        }

        if is_float {
            let val: f64 = text.parse().map_err(|_| LexerError {
                message: format!("Invalid float literal '{}'", text),
                line,
                col,
            })?;
            Ok(Token {
                ty: TokenType::Float,
                value: TokenValue::FloatVal(val),
                line,
                col,
                span: (start_pos, self.pos),
            })
        } else {
            let val: i64 = text.parse().map_err(|_| LexerError {
                message: format!("Invalid integer literal '{}'", text),
                line,
                col,
            })?;
            Ok(Token {
                ty: TokenType::Int,
                value: TokenValue::IntVal(val),
                line,
                col,
                span: (start_pos, self.pos),
            })
        }
    }
}
