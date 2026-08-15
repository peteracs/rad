
    use super::*;

    #[test]
    fn lex_basic() {
        let mut lex = Lexer::new("let x = 42");
        let (tokens, errors) = lex.tokenize();
        assert!(errors.is_empty());
        assert_eq!(tokens[0].ty, TokenType::Let);
        assert_eq!(tokens[1].ty, TokenType::Ident);
        assert_eq!(tokens[1].value.as_str(), Some("x"));
        assert_eq!(tokens[2].ty, TokenType::Assign);
        assert_eq!(tokens[3].ty, TokenType::Int);
        assert_eq!(tokens[3].value.as_int(), Some(42));
        assert_eq!(tokens[4].ty, TokenType::Eof);
    }

    #[test]
    fn structured_layout_restores_module_local_lines() {
        let source = "let first = 1\nlet second = 2\n";
        let mut layout = SourceLayout::single("first.rad");
        layout.push("let first = 1\n".len(), "second.rad");
        let tokens = Lexer::new_with_source_layout(source, &layout)
            .unwrap()
            .tokenize()
            .0;
        let lets = tokens
            .iter()
            .filter(|token| token.ty == TokenType::Let)
            .map(|token| token.line)
            .collect::<Vec<_>>();
        assert_eq!(lets, vec![1, 1]);

        let ordinary = Lexer::new(source).tokenize().0;
        let ordinary_lines = ordinary
            .iter()
            .filter(|token| token.ty == TokenType::Let)
            .map(|token| token.line)
            .collect::<Vec<_>>();
        assert_eq!(ordinary_lines, vec![1, 2]);
    }

    #[test]
    fn lex_keywords() {
        let mut lex =
            Lexer::new("component entity resource state system event pure once async await type");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Component);
        assert_eq!(tokens[2].ty, TokenType::Resource);
        assert_eq!(tokens[5].ty, TokenType::Event);
        assert_eq!(tokens[6].ty, TokenType::Pure);
        assert_eq!(tokens[7].ty, TokenType::Once);
        assert_eq!(tokens[8].ty, TokenType::Async);
        assert_eq!(tokens[9].ty, TokenType::Await);
        assert_eq!(tokens[10].ty, TokenType::Type);
    }

    #[test]
    fn lex_operators() {
        let mut lex = Lexer::new("|> :: -> => == != <= >=");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::PipeOp);
        assert_eq!(tokens[1].ty, TokenType::DColon);
        assert_eq!(tokens[2].ty, TokenType::Arrow);
        assert_eq!(tokens[3].ty, TokenType::FatArrow);
        assert_eq!(tokens[4].ty, TokenType::Eq);
        assert_eq!(tokens[5].ty, TokenType::Neq);
        assert_eq!(tokens[6].ty, TokenType::Lte);
        assert_eq!(tokens[7].ty, TokenType::Gte);
    }

    #[test]
    fn lex_symbolic_logical_operators() {
        let mut lex = Lexer::new("&& || & | |>");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::And);
        assert_eq!(tokens[1].ty, TokenType::Or);
        assert_eq!(tokens[2].ty, TokenType::Amp);
        assert_eq!(tokens[3].ty, TokenType::Pipe);
        assert_eq!(tokens[4].ty, TokenType::PipeOp);
    }

    #[test]
    fn lex_string_escape() {
        let mut lex = Lexer::new(r#""hello\nworld""#);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::String);
        assert_eq!(tokens[0].value.as_str(), Some("hello\nworld"));
    }

    #[test]
    fn lex_fstring_position_starts_at_f() {
        let mut lex = Lexer::new(r#"f"hello""#);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[0].line, 1);
        assert_eq!(tokens[0].col, 1);
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("hello"));
        assert_eq!(tokens[2].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_leading_dot_float() {
        let mut lex = Lexer::new(".5");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Float);
        assert_eq!(tokens[0].value.as_float(), Some(0.5));
    }

    #[test]
    fn lex_trailing_dot_float() {
        let mut lex = Lexer::new("5.");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Float);
        assert_eq!(tokens[0].value.as_float(), Some(5.0));
    }

    #[test]
    fn lex_scientific_notation() {
        let mut lex = Lexer::new("1e10 1.2e3 1.5e-4 -1.2E+3");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Float);
        assert_eq!(tokens[0].value.as_float(), Some(1e10));
        assert_eq!(tokens[1].ty, TokenType::Float);
        assert_eq!(tokens[1].value.as_float(), Some(1.2e3));
        assert_eq!(tokens[2].ty, TokenType::Float);
        assert_eq!(tokens[2].value.as_float(), Some(1.5e-4));
        assert_eq!(tokens[3].ty, TokenType::Minus);
        assert_eq!(tokens[4].ty, TokenType::Float);
        assert_eq!(tokens[4].value.as_float(), Some(1.2e3));
    }

    #[test]
    fn lex_leading_dot_scientific_notation() {
        let mut lex = Lexer::new(".5e2");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Float);
        assert_eq!(tokens[0].value.as_float(), Some(50.0));
    }

    #[test]
    fn lex_unterminated_block_comment_errors() {
        let mut lex = Lexer::new("/* unterminated");
        let err = lex.tokenize().1.into_iter().next().unwrap();
        assert!(err.message.contains("Unterminated block comment"));
    }

    #[test]
    fn lex_unterminated_nested_block_comment_errors() {
        let mut lex = Lexer::new("/* outer /* inner */");
        let err = lex.tokenize().1.into_iter().next().unwrap();
        assert!(err.message.contains("Unterminated block comment"));
    }

    #[test]
    fn lex_unterminated_string_after_escape_errors() {
        let mut lex = Lexer::new("\"abc\\");
        let err = lex.tokenize().1.into_iter().next().unwrap();
        assert!(err.message.contains("Unterminated string literal"));
    }

    #[test]
    fn lex_unterminated_fstring_after_escape_errors() {
        let mut lex = Lexer::new("f\"abc\\");
        let (_tokens, errors) = lex.tokenize();
        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("Unterminated f-string"));
    }

    #[test]
    fn lex_invalid_exponent_errors() {
        let mut lex = Lexer::new("1e+");
        let err = lex.tokenize().1.into_iter().next().unwrap();
        assert!(err.message.contains("Invalid float literal"));
    }

    #[test]
    fn lex_invalid_exponent_without_digits_errors() {
        let mut lex = Lexer::new("1e");
        let err = lex.tokenize().1.into_iter().next().unwrap();
        assert!(err.message.contains("Invalid float literal"));
    }

    #[test]
    fn lex_int_dot_ident_stays_separate_tokens() {
        let mut lex = Lexer::new("5.foo");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Int);
        assert_eq!(tokens[1].ty, TokenType::Dot);
        assert_eq!(tokens[2].ty, TokenType::Ident);
    }

    #[test]
    fn lex_negative_leading_dot_float() {
        let mut lex = Lexer::new("-.5");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Minus);
        assert_eq!(tokens[1].ty, TokenType::Float);
        assert_eq!(tokens[1].value.as_float(), Some(0.5));
    }

    #[test]
    fn lex_multiline_string_literal_errors() {
        let mut lex = Lexer::new("\"hello\nworld\"");
        let err = lex.tokenize().1.into_iter().next().unwrap();
        assert!(err.message.contains("Unterminated string literal"));
    }

    #[test]
    fn lex_multiline_fstring_succeeds() {
        let mut lex = Lexer::new("f\"hello\nworld\"");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("hello\nworld"));
        assert_eq!(tokens[2].ty, TokenType::FStringEnd);
    }

    // === Stress tests for reconstructed lexer/expr.rs ===

    #[test]
    fn lex_range_operator_dotdot() {
        let mut lex = Lexer::new("1..10");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Int);
        assert_eq!(tokens[0].value.as_int(), Some(1));
        assert_eq!(tokens[1].ty, TokenType::DotDot);
        assert_eq!(tokens[2].ty, TokenType::Int);
        assert_eq!(tokens[2].value.as_int(), Some(10));
    }

    #[test]
    fn lex_zero() {
        let mut lex = Lexer::new("0");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Int);
        assert_eq!(tokens[0].value.as_int(), Some(0));
    }

    #[test]
    fn lex_zero_dot_zero() {
        let mut lex = Lexer::new("0.0");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Float);
        assert_eq!(tokens[0].value.as_float(), Some(0.0));
    }

    #[test]
    fn lex_empty_string() {
        let mut lex = Lexer::new(r#""""#);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::String);
        assert_eq!(tokens[0].value.as_str(), Some(""));
    }

    #[test]
    fn lex_empty_fstring() {
        let mut lex = Lexer::new(r#"f"""#);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::FStringEnd);
        assert_eq!(tokens[2].ty, TokenType::Eof);
    }

    #[test]
    fn lex_fstring_with_braces() {
        let mut lex = Lexer::new(r#"f"hello {name}""#);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("hello "));
        assert_eq!(tokens[2].ty, TokenType::InterpolationStart);
        assert_eq!(tokens[3].ty, TokenType::Ident);
        assert_eq!(tokens[3].value.as_str(), Some("name"));
        assert_eq!(tokens[4].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[5].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_block_comment_simple() {
        let mut lex = Lexer::new("/* comment */ 42");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Int);
        assert_eq!(tokens[0].value.as_int(), Some(42));
    }

    #[test]
    fn lex_nested_block_comment_closed() {
        let mut lex = Lexer::new("/* outer /* inner */ */ 42");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Int);
        assert_eq!(tokens[0].value.as_int(), Some(42));
    }

    #[test]
    fn lex_line_comment_at_eof() {
        let mut lex = Lexer::new("42 // comment");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Int);
        assert_eq!(tokens[0].value.as_int(), Some(42));
        assert_eq!(tokens[1].ty, TokenType::Eof);
    }

    #[test]
    fn lex_line_comment_then_newline_then_code() {
        let mut lex = Lexer::new("// comment\n42");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Int);
        assert_eq!(tokens[0].value.as_int(), Some(42));
    }

    #[test]
    fn lex_only_whitespace() {
        let mut lex = Lexer::new("   \n\t\n   ");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].ty, TokenType::Eof);
    }

    #[test]
    fn lex_only_comment() {
        let mut lex = Lexer::new("// just a comment");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].ty, TokenType::Eof);
    }

    #[test]
    fn lex_string_all_escapes() {
        let mut lex = Lexer::new(r#""\n\t\r\\\"\0""#);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::String);
        assert_eq!(tokens[0].value.as_str(), Some("\n\t\r\\\"\0"));
    }

    #[test]
    fn lex_string_unknown_escape_passthrough() {
        let mut lex = Lexer::new(r#""\a""#);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::String);
        assert_eq!(tokens[0].value.as_str(), Some("\\a"));
    }

    #[test]
    fn lex_multiple_numbers_in_sequence() {
        let mut lex = Lexer::new("1 2.0 .3 4e5");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Int);
        assert_eq!(tokens[0].value.as_int(), Some(1));
        assert_eq!(tokens[1].ty, TokenType::Float);
        assert_eq!(tokens[1].value.as_float(), Some(2.0));
        assert_eq!(tokens[2].ty, TokenType::Float);
        assert_eq!(tokens[2].value.as_float(), Some(0.3));
        assert_eq!(tokens[3].ty, TokenType::Float);
        assert_eq!(tokens[3].value.as_float(), Some(4e5));
    }

    #[test]
    fn lex_question_and_pipe_operators() {
        let mut lex = Lexer::new("x? | y");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Ident);
        assert_eq!(tokens[1].ty, TokenType::Question);
        assert_eq!(tokens[2].ty, TokenType::Pipe);
        assert_eq!(tokens[3].ty, TokenType::Ident);
    }

    #[test]
    fn lex_method_call_on_int() {
        let mut lex = Lexer::new("42.to_str");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Int);
        assert_eq!(tokens[0].value.as_int(), Some(42));
        assert_eq!(tokens[1].ty, TokenType::Dot);
        assert_eq!(tokens[2].ty, TokenType::Ident);
    }

    #[test]
    fn lex_trailing_dot_eof_is_float() {
        let mut lex = Lexer::new("7.");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Float);
        assert_eq!(tokens[0].value.as_float(), Some(7.0));
    }

    #[test]
    fn lex_trailing_dot_then_operator() {
        let mut lex = Lexer::new("5.+3");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Float);
        assert_eq!(tokens[0].value.as_float(), Some(5.0));
        assert_eq!(tokens[1].ty, TokenType::Plus);
        assert_eq!(tokens[2].ty, TokenType::Int);
        assert_eq!(tokens[2].value.as_int(), Some(3));
    }

    #[test]
    fn lex_unterminated_string_eof() {
        let mut lex = Lexer::new("\"hello");
        let err = lex.tokenize().1.into_iter().next().unwrap();
        assert!(err.message.contains("Unterminated string literal"));
    }

    #[test]
    fn lex_unterminated_fstring_eof() {
        let mut lex = Lexer::new("f\"hello");
        let (_tokens, errors) = lex.tokenize();
        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("Unterminated f-string"));
    }

    #[test]
    fn lex_scientific_with_capital_e() {
        let mut lex = Lexer::new("1.5E2");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Float);
        assert_eq!(tokens[0].value.as_float(), Some(150.0));
    }

    #[test]
    fn lex_scientific_positive_exponent() {
        let mut lex = Lexer::new("2e+3");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Float);
        assert_eq!(tokens[0].value.as_float(), Some(2000.0));
    }

    #[test]
    fn lex_int_dot_dotdot_is_not_float() {
        let mut lex = Lexer::new("5..x");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Int);
        assert_eq!(tokens[0].value.as_int(), Some(5));
        assert_eq!(tokens[1].ty, TokenType::DotDot);
        assert_eq!(tokens[2].ty, TokenType::Ident);
    }

    #[test]
    fn lex_fstring_position_col() {
        let mut lex = Lexer::new(r#"  f"hi""#);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[0].col, 3);
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("hi"));
        assert_eq!(tokens[2].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_invalid_exponent_negative_sign_eof() {
        let mut lex = Lexer::new("1e-");
        let err = lex.tokenize().1.into_iter().next().unwrap();
        assert!(err.message.contains("Invalid float literal"));
    }

    #[test]
    fn lex_negative_exponent_capital_e() {
        let mut lex = Lexer::new("1E-2");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Float);
        assert_eq!(tokens[0].value.as_float(), Some(0.01));
    }

    #[test]
    fn lex_int_dot_paren() {
        let mut lex = Lexer::new("5.(3)");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Float);
        assert_eq!(tokens[0].value.as_float(), Some(5.0));
        assert_eq!(tokens[1].ty, TokenType::LParen);
    }

    #[test]
    fn lex_float_dot_float() {
        let mut lex = Lexer::new("5.0.1");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Float);
        assert_eq!(tokens[0].value.as_float(), Some(5.0));
        assert_eq!(tokens[1].ty, TokenType::Float);
        assert_eq!(tokens[1].value.as_float(), Some(0.1));
    }

    #[test]
    fn lex_large_integer() {
        let mut lex = Lexer::new("9999999999");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Int);
        assert_eq!(tokens[0].value.as_int(), Some(9999999999));
    }

    #[test]
    fn lex_adjacent_strings() {
        let mut lex = Lexer::new(r#""a""b""#);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::String);
        assert_eq!(tokens[0].value.as_str(), Some("a"));
        assert_eq!(tokens[1].ty, TokenType::String);
        assert_eq!(tokens[1].value.as_str(), Some("b"));
    }

    #[test]
    fn lex_comment_between_tokens() {
        let mut lex = Lexer::new("1 /* middle */ + 2");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Int);
        assert_eq!(tokens[1].ty, TokenType::Plus);
        assert_eq!(tokens[2].ty, TokenType::Int);
    }

    #[test]
    fn lex_int_range_no_space() {
        let mut lex = Lexer::new("0..10");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Int);
        assert_eq!(tokens[0].value.as_int(), Some(0));
        assert_eq!(tokens[1].ty, TokenType::DotDot);
        assert_eq!(tokens[2].ty, TokenType::Int);
        assert_eq!(tokens[2].value.as_int(), Some(10));
    }

    #[test]
    fn lex_struct_keyword() {
        let mut lex = Lexer::new("struct Point { x: 0.0 }");
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::Struct);
        assert_eq!(tokens[0].value.as_str(), Some("struct"));
        assert_eq!(tokens[1].ty, TokenType::Ident);
        assert_eq!(tokens[1].value.as_str(), Some("Point"));
        assert_eq!(tokens[2].ty, TokenType::LBrace);
    }

    // ---- Triple-quoted f-strings (f"""...""") ----
    //
    // Key design rule: in triple-quoted f-strings, bare `{` and `}` are LITERAL text.
    // Only `${expr}` triggers interpolation.  This is intentional so that embedded
    // code (C, JSON, JS, etc.) doesn't require brace-escaping.
    //
    // Contrast with regular f-strings (f"...") where both `{expr}` and `${expr}` interpolate,
    // and bare `{`/`}` must be doubled (`{{`/`}}`) or backslash-escaped (`\{`/`\}`).
    //
    // See `scan_fstring_content()` in lexer/stmt.rs for the implementation.

    #[test]
    fn lex_triple_fstring_basic() {
        let mut lex = Lexer::new(r#"f"""hello world""""#);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[0].value.as_bool(), Some(true));
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("hello world"));
        assert_eq!(tokens[2].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_triple_fstring_empty() {
        let mut lex = Lexer::new(r#"f"""""""""#);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[0].value.as_bool(), Some(true));
        assert_eq!(tokens[1].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_triple_fstring_multiline() {
        let src = "f\"\"\"\nline1\nline2\n\"\"\"";
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("\nline1\nline2\n"));
        assert_eq!(tokens[2].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_triple_fstring_bare_braces_are_literal() {
        let src = "f\"\"\"\nif (x) { y = 1; }\n\"\"\"";
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("\nif (x) { y = 1; }\n"));
        assert_eq!(tokens[2].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_triple_fstring_dollar_interpolation() {
        let src = "f\"\"\"\nhello ${name}\n\"\"\"";
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("\nhello "));
        assert_eq!(tokens[2].ty, TokenType::InterpolationStart);
        assert_eq!(tokens[3].ty, TokenType::Ident);
        assert_eq!(tokens[3].value.as_str(), Some("name"));
        assert_eq!(tokens[4].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[5].ty, TokenType::FStringFragment);
        assert_eq!(tokens[5].value.as_str(), Some("\n"));
        assert_eq!(tokens[6].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_triple_fstring_embedded_quotes() {
        let src = "f\"\"\"\nshe said \"hi\"\n\"\"\"";
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("\nshe said \"hi\"\n"));
        assert_eq!(tokens[2].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_triple_fstring_escaped_dollar() {
        let src = "f\"\"\"cost is \\$5\"\"\"";
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("cost is $5"));
        assert_eq!(tokens[2].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_triple_fstring_unterminated_errors() {
        let mut lex = Lexer::new("f\"\"\"hello world");
        let (_tokens, errors) = lex.tokenize();
        assert!(!errors.is_empty());
        assert!(errors[0]
            .message
            .contains("Unterminated triple-quoted f-string"));
    }

    #[test]
    fn lex_triple_fstring_seven_quotes_is_empty_triple_fstring() {
        let src = "f\"\"\"\"\"\"\"";
        let mut lex = Lexer::new(src);
        let (tokens, _errors) = lex.tokenize();
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[0].value.as_bool(), Some(true));
        assert_eq!(tokens[1].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_triple_fstring_with_inner_quotes_greedy_close() {
        let src = "f\"\"\"she said \"ok\"\"\"\"";
        let mut lex = Lexer::new(src);
        let (tokens, _errors) = lex.tokenize();
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("she said \"ok"));
        assert_eq!(tokens[2].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_triple_fstring_escaped_braces() {
        let src = "f\"\"\"\\{not interp\\}\"\"\"";
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("{not interp}"));
        assert_eq!(tokens[2].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_triple_fstring_backslash_at_eof() {
        let mut lex = Lexer::new("f\"\"\"hello\\");
        let (_tokens, errors) = lex.tokenize();
        assert!(!errors.is_empty());
        assert!(errors[0]
            .message
            .contains("Unterminated triple-quoted f-string"));
    }

    #[test]
    fn lex_triple_fstring_nested_braces_in_interpolation() {
        let src = "f\"\"\"${map{\"k\": 1}}\"\"\"";
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::InterpolationStart);
        assert_eq!(tokens[2].ty, TokenType::Ident);
        assert_eq!(tokens[2].value.as_str(), Some("map"));
        assert_eq!(tokens[3].ty, TokenType::LBrace);
        assert_eq!(tokens[4].ty, TokenType::String);
        assert_eq!(tokens[4].value.as_str(), Some("k"));
        assert_eq!(tokens[5].ty, TokenType::Colon);
        assert_eq!(tokens[6].ty, TokenType::Int);
        assert_eq!(tokens[7].ty, TokenType::RBrace);
        assert_eq!(tokens[8].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[9].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_fstring_string_with_brace_in_interpolation() {
        let src = r#"f"${" } "}""#;
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::InterpolationStart);
        assert_eq!(tokens[2].ty, TokenType::String);
        assert_eq!(tokens[2].value.as_str(), Some(" } "));
        assert_eq!(tokens[3].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[4].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_fstring_escaped_double_braces() {
        let mut lex = Lexer::new(r#"f"{{literal}}""#);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("{literal}"));
        assert_eq!(tokens[2].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_fstring_backslash_brace_escapes() {
        let mut lex = Lexer::new(r#"f"\{literal\}""#);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("{literal}"));
        assert_eq!(tokens[2].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_fstring_dollar_interpolation() {
        let mut lex = Lexer::new(r#"f"hello ${name} world""#);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("hello "));
        assert_eq!(tokens[2].ty, TokenType::InterpolationStart);
        assert_eq!(tokens[3].ty, TokenType::Ident);
        assert_eq!(tokens[3].value.as_str(), Some("name"));
        assert_eq!(tokens[4].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[5].ty, TokenType::FStringFragment);
        assert_eq!(tokens[5].value.as_str(), Some(" world"));
        assert_eq!(tokens[6].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_triple_fstring_escaped_dollar_brace_is_literal() {
        let src = "f\"\"\"\\${not_interp}\"\"\"";
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("${not_interp}"));
        assert_eq!(tokens[2].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_nested_fstring_in_interpolation() {
        let src = r#"f"${f"inner"}""#;
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart); // outer f"
        assert_eq!(tokens[1].ty, TokenType::InterpolationStart); // ${
        assert_eq!(tokens[2].ty, TokenType::FStringStart); // inner f"
        assert_eq!(tokens[3].ty, TokenType::FStringFragment); // "inner"
        assert_eq!(tokens[3].value.as_str(), Some("inner"));
        assert_eq!(tokens[4].ty, TokenType::FStringEnd); // inner "
        assert_eq!(tokens[5].ty, TokenType::InterpolationEnd); // }
        assert_eq!(tokens[6].ty, TokenType::FStringEnd); // outer "
    }

    #[test]
    fn lex_fstring_block_comment_with_brace_in_interpolation() {
        let src = r#"f"${foo(/* } */ x)}""#;
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::InterpolationStart);
        assert_eq!(tokens[2].ty, TokenType::Ident);
        assert_eq!(tokens[2].value.as_str(), Some("foo"));
        assert_eq!(tokens[3].ty, TokenType::LParen);
        assert_eq!(tokens[4].ty, TokenType::Ident);
        assert_eq!(tokens[4].value.as_str(), Some("x"));
        assert_eq!(tokens[5].ty, TokenType::RParen);
        assert_eq!(tokens[6].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[7].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_fstring_double_colon_path_in_interpolation() {
        // `::` inside an interpolation is a path separator, not the start
        // of a format spec: `f"{variant_of(Door::Locked)}"` used to cut the
        // interpolation at the first `:` and treat `:Locked)` as the spec.
        let src = r#"f"{variant_of(Door::Locked)}""#;
        let mut lex = Lexer::new(src);
        let (tokens, errors) = lex.tokenize();
        assert!(errors.is_empty(), "lex errors: {:?}", errors);
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::InterpolationStart);
        assert_eq!(tokens[2].ty, TokenType::Ident);
        assert_eq!(tokens[2].value.as_str(), Some("variant_of"));
        assert_eq!(tokens[3].ty, TokenType::LParen);
        assert_eq!(tokens[4].ty, TokenType::Ident);
        assert_eq!(tokens[4].value.as_str(), Some("Door"));
        assert_eq!(tokens[5].ty, TokenType::DColon);
        assert_eq!(tokens[6].ty, TokenType::Ident);
        assert_eq!(tokens[6].value.as_str(), Some("Locked"));
        assert_eq!(tokens[7].ty, TokenType::RParen);
        assert_eq!(tokens[8].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[9].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_fstring_format_spec_after_double_colon_path() {
        // a real format spec after a `::` path must still be recognized
        let src = r#"f"{val(Color::Red):>6}""#;
        let mut lex = Lexer::new(src);
        let (tokens, errors) = lex.tokenize();
        assert!(errors.is_empty(), "lex errors: {:?}", errors);
        let dcolon = tokens.iter().filter(|t| t.ty == TokenType::DColon).count();
        assert_eq!(dcolon, 1, "path separator must lex as DColon");
        let end = tokens
            .iter()
            .find(|t| t.ty == TokenType::InterpolationEnd)
            .expect("interpolation must terminate");
        assert_eq!(end.value.as_str(), Some(">6"), "spec must be `>6`");
    }

    #[test]
    fn lex_fstring_dollar_interpolation_double_colon() {
        let src = r#"f"${variant_of(Door::Locked)}""#;
        let mut lex = Lexer::new(src);
        let (tokens, errors) = lex.tokenize();
        assert!(errors.is_empty(), "lex errors: {:?}", errors);
        assert!(tokens.iter().any(|t| t.ty == TokenType::DColon));
        assert!(tokens.iter().any(|t| t.ty == TokenType::InterpolationEnd));
    }

    #[test]
    fn lex_triple_fstring_nested_fstring_in_dollar_interpolation() {
        let src = "f\"\"\"${f\"\"\"nested\"\"\"}\"\"\"";
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[0].value.as_bool(), Some(true));
        assert_eq!(tokens[1].ty, TokenType::InterpolationStart);
        assert_eq!(tokens[2].ty, TokenType::FStringStart);
        assert_eq!(tokens[2].value.as_bool(), Some(true));
        assert_eq!(tokens[3].ty, TokenType::FStringFragment);
        assert_eq!(tokens[3].value.as_str(), Some("nested"));
        assert_eq!(tokens[4].ty, TokenType::FStringEnd);
        assert_eq!(tokens[5].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[6].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_fstring_unterminated_interpolation_at_eof() {
        let src = r#"f"${x"#;
        let mut lex = Lexer::new(src);
        let (tokens, _errors) = lex.tokenize();
        let has_interp_start = tokens.iter().any(|t| t.ty == TokenType::InterpolationStart);
        assert!(has_interp_start);
        let has_eof = tokens.iter().any(|t| t.ty == TokenType::Eof);
        assert!(has_eof);
    }

    #[test]
    fn lex_fstring_multiple_interpolations() {
        let src = r#"f"{a} and {b}""#;
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::InterpolationStart);
        assert_eq!(tokens[2].ty, TokenType::Ident);
        assert_eq!(tokens[2].value.as_str(), Some("a"));
        assert_eq!(tokens[3].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[4].ty, TokenType::FStringFragment);
        assert_eq!(tokens[4].value.as_str(), Some(" and "));
        assert_eq!(tokens[5].ty, TokenType::InterpolationStart);
        assert_eq!(tokens[6].ty, TokenType::Ident);
        assert_eq!(tokens[6].value.as_str(), Some("b"));
        assert_eq!(tokens[7].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[8].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_fstring_adjacent_interpolations_no_literal_between() {
        let src = r#"f"{a}{b}""#;
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::InterpolationStart);
        assert_eq!(tokens[2].ty, TokenType::Ident);
        assert_eq!(tokens[3].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[4].ty, TokenType::InterpolationStart);
        assert_eq!(tokens[5].ty, TokenType::Ident);
        assert_eq!(tokens[6].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[7].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_fstring_format_spec_simple() {
        let src = r#"f"{x:.2f}""#;
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::InterpolationStart);
        assert_eq!(tokens[2].ty, TokenType::Ident);
        assert_eq!(tokens[2].value.as_str(), Some("x"));
        assert_eq!(tokens[3].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[3].value.as_str(), Some(".2f"));
        assert_eq!(tokens[4].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_fstring_format_spec_padding() {
        let src = r#"f"{name:>20}""#;
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::InterpolationStart);
        assert_eq!(tokens[2].ty, TokenType::Ident);
        assert_eq!(tokens[3].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[3].value.as_str(), Some(">20"));
    }