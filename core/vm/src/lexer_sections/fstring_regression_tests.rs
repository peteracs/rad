

    #[test]
    fn lex_fstring_format_spec_zero_pad_hex() {
        let src = r#"f"{val:#06x}""#;
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[3].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[3].value.as_str(), Some("#06x"));
    }

    #[test]
    fn lex_fstring_format_spec_empty_after_colon() {
        let src = r#"f"{x:}""#;
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[3].ty, TokenType::InterpolationEnd);
        assert!(
            tokens[3].value.as_str().is_none(),
            "empty spec should be None"
        );
    }

    #[test]
    fn lex_fstring_format_spec_dollar_brace() {
        let src = r#"f"${x:.3f}""#;
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::InterpolationStart);
        assert_eq!(tokens[2].ty, TokenType::Ident);
        assert_eq!(tokens[3].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[3].value.as_str(), Some(".3f"));
    }

    #[test]
    fn lex_fstring_no_format_spec() {
        let src = r#"f"{x}""#;
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        assert_eq!(tokens[3].ty, TokenType::InterpolationEnd);
        assert!(tokens[3].value.as_str().is_none(), "no spec should be None");
    }

    #[test]
    fn lex_triple_fstring_format_spec() {
        let src = "f\"\"\"\n${pi:.4f}\n\"\"\"";
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().0;
        let end_tok = tokens
            .iter()
            .find(|t| t.ty == TokenType::InterpolationEnd)
            .unwrap();
        assert_eq!(end_tok.value.as_str(), Some(".4f"));
    }

    // --- Tests for escaped-quote strings (\"..\") inside interpolation ---

    #[test]
    fn lex_fstring_escaped_quote_string_basic() {
        let src = r#"f"val={m[\"key\"]}""#;
        let mut lex = Lexer::new(src);
        let (tokens, errors) = lex.tokenize();
        assert!(errors.is_empty(), "errors: {:?}", errors);
        assert_eq!(tokens[0].ty, TokenType::FStringStart);
        assert_eq!(tokens[1].ty, TokenType::FStringFragment);
        assert_eq!(tokens[1].value.as_str(), Some("val="));
        assert_eq!(tokens[2].ty, TokenType::InterpolationStart);
        assert_eq!(tokens[3].ty, TokenType::Ident);
        assert_eq!(tokens[3].value.as_str(), Some("m"));
        assert_eq!(tokens[4].ty, TokenType::LBracket);
        assert_eq!(tokens[5].ty, TokenType::String);
        assert_eq!(tokens[5].value.as_str(), Some("key"));
        assert_eq!(tokens[6].ty, TokenType::RBracket);
        assert_eq!(tokens[7].ty, TokenType::InterpolationEnd);
        assert_eq!(tokens[8].ty, TokenType::FStringEnd);
    }

    #[test]
    fn lex_fstring_escaped_quote_string_empty() {
        let src = r#"f"{\"\"}""#;
        let mut lex = Lexer::new(src);
        let (tokens, errors) = lex.tokenize();
        assert!(errors.is_empty(), "errors: {:?}", errors);
        assert_eq!(tokens[2].ty, TokenType::String);
        assert_eq!(tokens[2].value.as_str(), Some(""));
    }

    #[test]
    fn lex_fstring_escaped_quote_string_with_spaces() {
        let src = r#"f"{m[\"hello world\"]}""#;
        let mut lex = Lexer::new(src);
        let (tokens, errors) = lex.tokenize();
        assert!(errors.is_empty(), "errors: {:?}", errors);
        let str_tok = tokens.iter().find(|t| t.ty == TokenType::String).unwrap();
        assert_eq!(str_tok.value.as_str(), Some("hello world"));
    }

    #[test]
    fn lex_fstring_escaped_quote_string_escape_sequences() {
        let src = r#"f"{\"a\tb\nc\"}""#;
        let mut lex = Lexer::new(src);
        let (tokens, errors) = lex.tokenize();
        assert!(errors.is_empty(), "errors: {:?}", errors);
        let str_tok = tokens.iter().find(|t| t.ty == TokenType::String).unwrap();
        assert_eq!(str_tok.value.as_str(), Some("a\tb\nc"));
    }

    #[test]
    fn lex_fstring_escaped_quote_string_literal_backslash() {
        let src = r#"f"{\"a\\\\b\"}""#;
        let mut lex = Lexer::new(src);
        let (tokens, errors) = lex.tokenize();
        assert!(errors.is_empty(), "errors: {:?}", errors);
        let str_tok = tokens.iter().find(|t| t.ty == TokenType::String).unwrap();
        assert_eq!(str_tok.value.as_str(), Some("a\\\\b"));
    }

    #[test]
    fn lex_fstring_escaped_quote_string_with_braces() {
        let src = r#"f"{\"hello{world}\"}""#;
        let mut lex = Lexer::new(src);
        let (tokens, errors) = lex.tokenize();
        assert!(errors.is_empty(), "errors: {:?}", errors);
        let str_tok = tokens.iter().find(|t| t.ty == TokenType::String).unwrap();
        assert_eq!(str_tok.value.as_str(), Some("hello{world}"));
    }

    #[test]
    fn lex_fstring_escaped_quote_string_multiple() {
        let src = r#"f"{\"a\" + \"b\"}""#;
        let mut lex = Lexer::new(src);
        let (tokens, errors) = lex.tokenize();
        assert!(errors.is_empty(), "errors: {:?}", errors);
        let strings: Vec<_> = tokens
            .iter()
            .filter(|t| t.ty == TokenType::String)
            .collect();
        assert_eq!(strings.len(), 2);
        assert_eq!(strings[0].value.as_str(), Some("a"));
        assert_eq!(strings[1].value.as_str(), Some("b"));
    }

    #[test]
    fn lex_fstring_escaped_quote_string_unterminated() {
        let src = r#"f"{\"abc}"#;
        let mut lex = Lexer::new(src);
        let (_tokens, errors) = lex.tokenize();
        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("Unterminated"));
    }

    #[test]
    fn lex_fstring_escaped_quote_string_unterminated_newline() {
        let src = "f\"{\\\"abc\n}\"";
        let mut lex = Lexer::new(src);
        let (_tokens, errors) = lex.tokenize();
        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("Unterminated"));
    }

    #[test]
    fn lex_fstring_escaped_quote_in_dollar_interpolation() {
        let src = r#"f"${m[\"key\"]}""#;
        let mut lex = Lexer::new(src);
        let (tokens, errors) = lex.tokenize();
        assert!(errors.is_empty(), "errors: {:?}", errors);
        let str_tok = tokens.iter().find(|t| t.ty == TokenType::String).unwrap();
        assert_eq!(str_tok.value.as_str(), Some("key"));
    }

    #[test]
    fn lex_fstring_escaped_quote_with_format_spec() {
        let src = r#"f"{\"hello\":>10}""#;
        let mut lex = Lexer::new(src);
        let (tokens, errors) = lex.tokenize();
        assert!(errors.is_empty(), "errors: {:?}", errors);
        let str_tok = tokens.iter().find(|t| t.ty == TokenType::String).unwrap();
        assert_eq!(str_tok.value.as_str(), Some("hello"));
        let end_tok = tokens
            .iter()
            .find(|t| t.ty == TokenType::InterpolationEnd)
            .unwrap();
        assert_eq!(end_tok.value.as_str(), Some(">10"));
    }

    #[test]
    fn lex_backslash_not_quote_in_interpolation_still_errors() {
        let src = r#"f"{\ }"#;
        let mut lex = Lexer::new(src);
        let (_tokens, errors) = lex.tokenize();
        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("Unexpected character"));
    }
