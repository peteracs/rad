use super::*;

pub(super) fn keyword_token_type(word: &str) -> Option<TokenType> {
    match word {
        "component" => Some(TokenType::Component),
        "struct" => Some(TokenType::Struct),
        "entity" => Some(TokenType::Entity),
        "resource" => Some(TokenType::Resource),
        "state" => Some(TokenType::State),
        "system" => Some(TokenType::System),
        "event" => Some(TokenType::Event),
        "on" => Some(TokenType::On),
        "emit" => Some(TokenType::Emit),
        "fn" => Some(TokenType::Fn),
        "let" => Some(TokenType::Let),
        "mut" => Some(TokenType::Mut),
        "if" => Some(TokenType::If),
        "else" => Some(TokenType::Else),
        "while" => Some(TokenType::While),
        "for" => Some(TokenType::For),
        "in" => Some(TokenType::In),
        "return" => Some(TokenType::Return),
        "true" => Some(TokenType::True),
        "false" => Some(TokenType::False),
        "nil" => Some(TokenType::Nil),
        "schedule" => Some(TokenType::Schedule),
        "and" => Some(TokenType::And),
        "or" => Some(TokenType::Or),
        "not" => Some(TokenType::Not),
        "match" => Some(TokenType::Match),
        "when" => Some(TokenType::When),
        "use" => Some(TokenType::Use),
        "break" => Some(TokenType::Break),
        "continue" => Some(TokenType::Continue),
        "type" => Some(TokenType::Type),
        "pure" => Some(TokenType::Pure),
        "once" => Some(TokenType::Once),
        "async" => Some(TokenType::Async),
        "await" => Some(TokenType::Await),
        "rec" => Some(TokenType::Rec),
        "pub" => Some(TokenType::Pub),
        "as" => Some(TokenType::As),
        "indexed" => Some(TokenType::Indexed),
        "unique" => Some(TokenType::Unique),
        _ => None,
    }
}

pub fn reserved_keyword_rename_hints(word: &str) -> Vec<String> {
    let mut hints: Vec<String> = match word {
        "match" => vec![
            "case_value".to_string(),
            "matched".to_string(),
            "match_".to_string(),
        ],
        "type" => vec!["kind".to_string(), "shape".to_string(), "type_".to_string()],
        "use" => vec![
            "module_path".to_string(),
            "import_ref".to_string(),
            "use_".to_string(),
        ],
        "schedule" => vec![
            "sched".to_string(),
            "plan".to_string(),
            "schedule_".to_string(),
        ],
        "emit" => vec![
            "event_name".to_string(),
            "signal".to_string(),
            "emit_".to_string(),
        ],
        "on" => vec![
            "handler".to_string(),
            "listener".to_string(),
            "on_".to_string(),
        ],
        "fn" => vec!["func".to_string(), "compute".to_string(), "fn_".to_string()],
        "let" => vec!["value".to_string(), "item".to_string(), "let_".to_string()],
        "mut" => vec![
            "mutable".to_string(),
            "stateful".to_string(),
            "mut_".to_string(),
        ],
        "component" => vec![
            "comp".to_string(),
            "part".to_string(),
            "component_".to_string(),
        ],
        "struct" => vec![
            "record".to_string(),
            "data".to_string(),
            "struct_".to_string(),
        ],
        "system" => vec![
            "svc".to_string(),
            "pipeline".to_string(),
            "system_".to_string(),
        ],
        "event" => vec![
            "evt".to_string(),
            "notice".to_string(),
            "event_".to_string(),
        ],
        "state" => vec![
            "status".to_string(),
            "phase".to_string(),
            "state_".to_string(),
        ],
        "entity" => vec![
            "record".to_string(),
            "node".to_string(),
            "entity_".to_string(),
        ],
        "resource" => vec![
            "global_data".to_string(),
            "shared_state".to_string(),
            "resource_".to_string(),
        ],
        "if" => vec![
            "condition".to_string(),
            "flag".to_string(),
            "if_".to_string(),
        ],
        "else" => vec![
            "fallback".to_string(),
            "alternate".to_string(),
            "else_".to_string(),
        ],
        "for" => vec!["item".to_string(), "iter".to_string(), "for_".to_string()],
        "while" => vec![
            "looping".to_string(),
            "active".to_string(),
            "while_".to_string(),
        ],
        "break" => vec![
            "stop_now".to_string(),
            "exit_loop".to_string(),
            "break_".to_string(),
        ],
        "continue" => vec![
            "keep_going".to_string(),
            "next_item".to_string(),
            "continue_".to_string(),
        ],
        "return" => vec![
            "result".to_string(),
            "output".to_string(),
            "return_".to_string(),
        ],
        "and" => vec!["both".to_string(), "all_ok".to_string(), "and_".to_string()],
        "or" => vec![
            "either".to_string(),
            "any_ok".to_string(),
            "or_".to_string(),
        ],
        "not" => vec![
            "negated".to_string(),
            "inverse".to_string(),
            "not_".to_string(),
        ],
        "true" => vec![
            "is_true".to_string(),
            "flag_true".to_string(),
            "true_".to_string(),
        ],
        "false" => vec![
            "is_false".to_string(),
            "flag_false".to_string(),
            "false_".to_string(),
        ],
        "nil" => vec![
            "empty_value".to_string(),
            "none_value".to_string(),
            "nil_".to_string(),
        ],
        "once" => vec![
            "single".to_string(),
            "one_time".to_string(),
            "once_".to_string(),
        ],
        "pure" => vec![
            "safe".to_string(),
            "deterministic".to_string(),
            "pure_".to_string(),
        ],
        "when" => vec![
            "predicate".to_string(),
            "guard".to_string(),
            "when_".to_string(),
        ],
        "in" => vec![
            "inside".to_string(),
            "source".to_string(),
            "in_".to_string(),
        ],
        "as" => vec!["alias".to_string(), "named".to_string(), "as_".to_string()],
        "rec" => vec![
            "recursive".to_string(),
            "recur".to_string(),
            "rec_".to_string(),
        ],
        "indexed" => vec![
            "fast_lookup".to_string(),
            "indexed_".to_string(),
            "keyed".to_string(),
        ],
        "unique" => vec![
            "single_owner".to_string(),
            "owned".to_string(),
            "unique_".to_string(),
        ],
        _ => vec![],
    };
    if hints.is_empty() || !hints.iter().any(|h| h == &format!("{word}_")) {
        hints.push(format!("{word}_"));
    }
    hints
}

impl<'a> Lexer<'a> {
    pub(super) fn read_ident(&mut self, start_pos: usize) -> Result<Token, LexerError> {
        let line = self.line;
        let col = self.col;
        let mut text = String::new();
        while self.pos < self.source.len() && (self.peek().is_alphanumeric() || self.peek() == '_')
        {
            text.push(self.advance());
        }
        // f"..." or f"""...""" — enter f-string scanning mode.
        // The `triple` flag changes interpolation rules: see scan_fstring_content()
        // in stmt.rs for details (triple uses only ${} for interpolation; bare {} are literal).
        if text == "f" && self.pos < self.source.len() && self.peek() == '"' {
            let rest = &self.source[self.pos..];
            let triple = rest.starts_with("\"\"\"");
            if triple {
                self.advance(); // "
                self.advance(); // "
                self.advance(); // "
            } else {
                self.advance(); // "
            }
            self.mode_stack.push(LexerMode::FString { triple });
            return Ok(Token {
                ty: TokenType::FStringStart,
                value: TokenValue::Bool(triple),
                line,
                col,
                span: (start_pos, self.pos),
            });
        }
        if let Some(kw) = keyword_token_type(&text) {
            match kw {
                TokenType::True => Ok(Token {
                    ty: kw,
                    value: TokenValue::Bool(true),
                    line,
                    col,
                    span: (start_pos, self.pos),
                }),
                TokenType::False => Ok(Token {
                    ty: kw,
                    value: TokenValue::Bool(false),
                    line,
                    col,
                    span: (start_pos, self.pos),
                }),
                _ => Ok(Token {
                    ty: kw,
                    value: TokenValue::Str(text),
                    line,
                    col,
                    span: (start_pos, self.pos),
                }),
            }
        } else {
            Ok(Token {
                ty: TokenType::Ident,
                value: TokenValue::Str(text),
                line,
                col,
                span: (start_pos, self.pos),
            })
        }
    }
}
