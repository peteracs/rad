use crate::lexer::{Lexer, Token, TokenType};

const INDENT: &str = "    ";

fn is_decl_keyword(word: &str) -> bool {
    matches!(
        word,
        "component"
            | "entity"
            | "system"
            | "event"
            | "fn"
            | "state"
            | "type"
            | "on"
            | "intent"
            | "law"
            | "resolver"
    )
}

/// Token-level bracket accounting for one line: how many closing
/// brackets LEAD the line (they dedent the line itself), and the net
/// open-minus-close across it (it indents what follows). Lexed, so
/// braces inside strings, f-strings, and comments never count — the
/// char-counting this replaces mis-indented everything after a line
/// like `print("{")`.
fn line_depth_info(line: &str) -> (usize, i32) {
    let mut lexer = Lexer::new(line);
    lexer.preserve_comments = true;
    let (tokens, errors) = lexer.tokenize();
    if !errors.is_empty() {
        // unlexable fragment: leave depth alone, never guess
        return (0, 0);
    }
    let mut leading_closes = 0usize;
    let mut counting_leading = true;
    let mut net = 0i32;
    for tok in &tokens {
        match tok.ty {
            TokenType::Whitespace | TokenType::Comment => continue,
            TokenType::Eof => break,
            TokenType::LBrace | TokenType::LParen | TokenType::LBracket => {
                net += 1;
                counting_leading = false;
            }
            TokenType::RBrace | TokenType::RParen | TokenType::RBracket => {
                net -= 1;
                if counting_leading {
                    leading_closes += 1;
                }
            }
            _ => {
                counting_leading = false;
            }
        }
    }
    (leading_closes, net)
}

/// If the line opens a `/* … */` block comment that is still open at
/// end-of-line, return the nesting depth left open (the lexer allows
/// nested block comments). The formatter works line by line, so without
/// this the continuation lines of a multi-line comment lex as ordinary
/// code and get re-spaced — `*/` became `* /`, which unterminates the
/// comment and breaks the whole file.
fn unclosed_block_comment_depth(line: &str) -> Option<u32> {
    let mut lexer = Lexer::new(line);
    lexer.preserve_comments = true;
    let (tokens, errors) = lexer.tokenize();
    if !errors
        .iter()
        .any(|e| e.message.contains("Unterminated block comment"))
    {
        return None;
    }
    // Clean lexing stopped right before the offending `/*`: the first
    // occurrence at or after the last emitted token is the comment start
    // (whitespace is tokenized here, so nothing hides in between).
    let scan_from = tokens
        .iter()
        .filter(|t| t.ty != TokenType::Eof)
        .map(|t| t.span.1)
        .max()
        .unwrap_or(0);
    let open = scan_from + line.get(scan_from..)?.find("/*")?;
    Some(block_comment_depth_after(&line[open..], 0))
}

/// Track nested `/*`/`*/` markers across one line of comment text,
/// mirroring the lexer's scan (quotes have no meaning inside a comment).
fn block_comment_depth_after(line: &str, mut depth: u32) -> u32 {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            depth += 1;
            i += 2;
        } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            depth = depth.saturating_sub(1);
            i += 2;
        } else {
            i += 1;
        }
    }
    depth
}

fn count_unescaped_triple_quotes(s: &str) -> usize {
    let mut count = 0;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'"' && bytes[i + 1] == b'"' && bytes[i + 2] == b'"' {
            count += 1;
            i += 3;
        } else {
            i += 1;
        }
    }
    count
}

pub fn format_rad(source: &str) -> String {
    // a UTF-8 BOM is encoding metadata, not source: format without it,
    // re-attach it at the end so the file's encoding intent survives
    let (bom, source) = match source.strip_prefix('\u{feff}') {
        Some(rest) => ("\u{feff}", rest),
        None => ("", source),
    };
    let lines: Vec<&str> = source.split('\n').collect();
    let mut result = Vec::new();
    // Render levels as a STACK, one entry per open bracket ({, (, [).
    // Every bracket opened on the same line maps to the SAME level+1,
    // so `filter(fn(e) {` indents its body one step, not three — and
    // the closing `})` pops back to the statement's level exactly.
    let mut levels: Vec<usize> = Vec::new();
    let mut prev_kind: Option<&'static str> = None;
    let mut prev_blank = false;
    let mut in_triple_fstring = false;
    // open nesting depth of a multi-line `/* … */` block comment; its
    // continuation lines are comment TEXT and must pass through verbatim
    let mut block_comment_depth = 0u32;

    for raw_line in lines {
        let stripped = raw_line.trim();

        if block_comment_depth > 0 {
            result.push(raw_line.trim_end().to_string());
            block_comment_depth = block_comment_depth_after(raw_line, block_comment_depth);
            prev_kind = Some("comment");
            prev_blank = false;
            continue;
        }

        if in_triple_fstring {
            result.push(raw_line.trim_end().to_string());
            let tq = count_unescaped_triple_quotes(raw_line);
            if tq > 0 {
                in_triple_fstring = false;
            }
            continue;
        }

        if stripped.is_empty() {
            if !prev_blank && !result.is_empty() {
                result.push(String::new());
                prev_blank = true;
            }
            continue;
        }
        prev_blank = false;

        if stripped.starts_with("//") {
            let render = levels.last().copied().unwrap_or(0);
            result.push(format!("{}{}", INDENT.repeat(render), stripped));
            prev_kind = Some("comment");
            continue;
        }

        if let Some(depth) = unclosed_block_comment_depth(stripped) {
            // the line opens a block comment that runs past end-of-line;
            // emit it untouched (the triple-quote scan below must not see
            // comment text either) and switch to verbatim mode
            let render = levels.last().copied().unwrap_or(0);
            result.push(format!("{}{}", INDENT.repeat(render), stripped));
            block_comment_depth = depth;
            prev_kind = Some("comment");
            continue;
        }

        let (leading_closes, net) = line_depth_info(stripped);
        // closers at the head of the line dedent the line itself
        for _ in 0..leading_closes {
            levels.pop();
        }
        let base_render = levels.last().copied().unwrap_or(0);
        // A line that begins with the pipeline operator `|>` continues the
        // previous statement; it is neither a new statement nor a bracketed
        // body, so the level stack does not account for it. Indent it one
        // step past the statement it continues — the style every multi-line
        // pipeline in the docs uses. Without this the continuation rendered
        // at the statement's own column (column 0 at top level), so a pipe
        // stage looked like a new statement and `fmt --check` rejected the
        // guide's own examples. Bracket bookkeeping below keys off `render`,
        // so a closure opened on a `|>` line still indents past the pipe.
        let is_pipe_continuation = stripped.starts_with("|>");
        let render = if is_pipe_continuation {
            base_render + 1
        } else {
            base_render
        };

        let kind = classify(stripped);
        if let Some(pk) = prev_kind {
            if (kind == "decl" || kind == "fn")
                && render == 0
                && pk != "blank"
                && pk != "comment"
                && !result.is_empty()
                && !result.last().unwrap().is_empty()
            {
                result.push(String::new());
            }
        }

        result.push(format!(
            "{}{}",
            INDENT.repeat(render),
            normalize_spacing(stripped)
        ));

        let tq = count_unescaped_triple_quotes(stripped);
        if tq % 2 == 1 {
            in_triple_fstring = true;
        }

        // brackets left open by this line all render their contents at
        // one level past this line; extra closers pop further
        let remaining = net + leading_closes as i32;
        if remaining > 0 {
            for _ in 0..remaining {
                levels.push(render + 1);
            }
        } else {
            for _ in 0..(-remaining) {
                levels.pop();
            }
        }
        prev_kind = Some(kind);
    }

    while !result.is_empty() && result.last().unwrap().is_empty() {
        result.pop();
    }
    result.push(String::new());

    // keep the file's own newline convention: a CRLF repo must not be
    // declared dirty over invisible bytes (this WAS the bulk of the
    // repo-wide `fmt --check` noise)
    let body = if source.contains("\r\n") {
        result.join("\r\n")
    } else {
        result.join("\n")
    };
    format!("{}{}", bom, body)
}

fn classify(line: &str) -> &'static str {
    if line.is_empty() {
        return "blank";
    }
    if line.starts_with("//") {
        return "comment";
    }

    let first_word = line
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .next()
        .unwrap_or("");
    if is_decl_keyword(first_word) {
        return "decl";
    }
    if line.starts_with("pure ") {
        return "fn";
    }
    if line.starts_with("fn ") {
        return "fn";
    }
    "stmt"
}

/// Can this token appear as a base name of a generic type (`list<…>`,
/// `entity<…>`)? The parser's type grammar accepts identifiers plus the
/// `entity`/`state`/`system` keywords.
fn is_type_base(ty: TokenType) -> bool {
    matches!(
        ty,
        TokenType::Ident | TokenType::Entity | TokenType::State | TokenType::System
    )
}

/// Does `tokens[open]` (a `<` whose previous significant token is a type
/// base) open a generic type argument list rather than a comparison?
/// Returns the index of the matching `>`.
///
/// Generics and comparisons lex identically (`Lt`/`Gt`), so this is a
/// heuristic — but a safe one, because spacing never changes how the
/// parser reads them (types only exist in type positions). It requires:
/// a matching `>` on the same line, strictly type-like tokens between
/// (identifiers, `,`, `.`, `|` unions, `nil`, nested `<…>`), and a
/// follower that could not continue an expression, so `a < b` and
/// `len(xs) < n` keep their comparison spacing.
fn generic_close_index(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut j = open;
    while j < tokens.len() {
        let tok = &tokens[j];
        match tok.ty {
            TokenType::Lt => depth += 1,
            TokenType::Gt => {
                depth -= 1;
                if depth == 0 {
                    // adjacent `>>` at depth zero is a right-shift whose
                    // first half we almost claimed — splitting the pair
                    // would change the program
                    if let Some(next) = tokens.get(j + 1) {
                        if next.ty == TokenType::Gt && tok.span.1 == next.span.0 {
                            return None;
                        }
                    }
                    let follower = tokens[j + 1..]
                        .iter()
                        .find(|t| !matches!(t.ty, TokenType::Whitespace | TokenType::Comment));
                    // anything that starts an operand means the `>` was a
                    // comparison (`x < y > z`, `f(a<b, c>d)`)
                    if let Some(f) = follower {
                        if matches!(
                            f.ty,
                            TokenType::Int
                                | TokenType::Float
                                | TokenType::String
                                | TokenType::FStringStart
                                | TokenType::Ident
                                | TokenType::True
                                | TokenType::False
                                | TokenType::Nil
                                | TokenType::Entity
                                | TokenType::State
                                | TokenType::System
                                | TokenType::Not
                                | TokenType::Bang
                                | TokenType::Tilde
                                | TokenType::Minus
                        ) {
                            return None;
                        }
                    }
                    return Some(j);
                }
            }
            TokenType::Comma
            | TokenType::Dot
            | TokenType::Pipe
            | TokenType::Whitespace
            | TokenType::Nil => {}
            ty if is_type_base(ty) => {}
            _ => return None,
        }
        j += 1;
    }
    None
}

fn normalize_spacing(line: &str) -> String {
    let mut lexer = Lexer::new(line);
    lexer.preserve_comments = true;
    let (tokens, errors) = lexer.tokenize();
    if !errors.is_empty() {
        return line.to_string(); // fallback on lex error
    }

    let mut out = String::new();

    // last significant token (skipping whitespace/comments), used to
    // tell unary minus (`-1`, `(-x`, `a + -b`) from binary subtraction
    let mut prev_sig: Option<TokenType> = None;

    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        if tok.ty == TokenType::Eof {
            break;
        }

        // Adjacent `>` `>` is the right-shift operator (lexed as two tokens
        // so nested generics keep working) — format it as one unit; spacing
        // the pair apart would break the adjacency rule and the code.
        if tok.ty == TokenType::Gt {
            if let Some(next) = tokens.get(i + 1) {
                if next.ty == TokenType::Gt && tok.span.1 == next.span.0 {
                    if !out.ends_with(' ') && !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(">>");
                    out.push(' ');
                    prev_sig = Some(TokenType::Gt);
                    i += 2;
                    continue;
                }
            }
        }

        // Generic type argument lists — `Result<int, str>`, `list<list<int>>`,
        // `fn name<T>(…)` — must stay tight; spacing their angle brackets
        // like comparisons corrupted every typed signature. Emitting the
        // whole run here also heals previously mangled `Result < int, str >`.
        if tok.ty == TokenType::Lt && prev_sig.is_some_and(is_type_base) {
            if let Some(close) = generic_close_index(&tokens, i) {
                while out.ends_with(' ') {
                    out.pop();
                }
                let mut pending_space = false;
                for t in &tokens[i..=close] {
                    match t.ty {
                        TokenType::Whitespace | TokenType::Comment => pending_space = true,
                        TokenType::Lt => {
                            out.push('<');
                            pending_space = false;
                        }
                        TokenType::Gt => {
                            out.push('>');
                            pending_space = false;
                        }
                        TokenType::Dot => {
                            out.push('.');
                            pending_space = false;
                        }
                        TokenType::Comma => {
                            out.push_str(", ");
                            pending_space = false;
                        }
                        _ => {
                            if pending_space
                                && !out.ends_with(' ')
                                && !out.ends_with('<')
                                && !out.ends_with('.')
                            {
                                out.push(' ');
                            }
                            out.push_str(&line[t.span.0..t.span.1]);
                            pending_space = false;
                        }
                    }
                }
                i = close + 1;
                // a space between the closing `>` and a following `,`/`)`/`]`/`(`
                // is a leftover of the old operator spacing — drop it
                if tokens.get(i).is_some_and(|t| t.ty == TokenType::Whitespace)
                    && tokens.get(i + 1).is_some_and(|t| {
                        matches!(
                            t.ty,
                            TokenType::Comma
                                | TokenType::RParen
                                | TokenType::RBracket
                                | TokenType::LParen
                        )
                    })
                {
                    i += 1;
                }
                prev_sig = Some(TokenType::Gt);
                continue;
            }
        }

        let text = &line[tok.span.0..tok.span.1];

        if matches!(
            tok.ty,
            TokenType::String
                | TokenType::FStringStart
                | TokenType::FStringFragment
                | TokenType::InterpolationStart
                | TokenType::InterpolationEnd
                | TokenType::FStringEnd
                | TokenType::Comment
        ) {
            out.push_str(text);
            if tok.ty != TokenType::Comment {
                prev_sig = Some(tok.ty);
            }
            i += 1;
            continue;
        }

        if tok.ty == TokenType::Whitespace {
            // a gap in front of a trailing comment is the AUTHOR'S
            // alignment — keep it verbatim (the corpus aligns comment
            // columns; collapsing them was the #1 source of fmt noise)
            let next_is_comment = tokens
                .get(i + 1)
                .is_some_and(|t| t.ty == TokenType::Comment);
            if next_is_comment && !out.is_empty() {
                // the run replaces any single space an operator/comma
                // rule already emitted — otherwise alignment grows by
                // one column per fmt pass
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push_str(text);
            } else if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            i += 1;
            continue;
        }

        // minus is binary only after something that can end an operand
        let minus_is_unary = tok.ty == TokenType::Minus
            && !matches!(
                prev_sig,
                Some(
                    TokenType::Int
                        | TokenType::Float
                        | TokenType::Ident
                        | TokenType::String
                        | TokenType::FStringEnd
                        | TokenType::True
                        | TokenType::False
                        | TokenType::Nil
                        | TokenType::RParen
                        | TokenType::RBracket
                        | TokenType::RBrace
                )
            );

        let needs_space_before = match tok.ty {
            TokenType::Minus if minus_is_unary => false,
            TokenType::Assign
            | TokenType::Eq
            | TokenType::Neq
            | TokenType::Lt
            | TokenType::Gt
            | TokenType::Lte
            | TokenType::Gte
            | TokenType::Plus
            | TokenType::Minus
            | TokenType::Star
            | TokenType::Slash
            | TokenType::Percent
            | TokenType::Amp
            | TokenType::Caret
            | TokenType::PipeOp
            | TokenType::LessLess
            | TokenType::FatArrow
            | TokenType::Arrow
            | TokenType::LBrace => !out.ends_with(' ') && !out.is_empty(),
            _ => false,
        };

        if needs_space_before {
            out.push(' ');
        }

        out.push_str(text);

        let needs_space_after = match tok.ty {
            TokenType::Minus if minus_is_unary => false,
            TokenType::Assign
            | TokenType::Eq
            | TokenType::Neq
            | TokenType::Lt
            | TokenType::Gt
            | TokenType::Lte
            | TokenType::Gte
            | TokenType::Plus
            | TokenType::Minus
            | TokenType::Star
            | TokenType::Slash
            | TokenType::Percent
            | TokenType::Amp
            | TokenType::Caret
            | TokenType::PipeOp
            | TokenType::LessLess
            | TokenType::FatArrow
            | TokenType::Arrow
            | TokenType::Comma
            | TokenType::Colon => true,
            _ => false,
        };

        if needs_space_after {
            out.push(' ');
        }
        prev_sig = Some(tok.ty);
        i += 1;
    }

    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::format_rad;

    #[test]
    fn basic_indentation() {
        let source = "component Health {\nhp: 100\n}\n";
        let result = format_rad(source);
        assert!(result.contains("  hp:"));
    }

    #[test]
    fn fn_indentation() {
        let source = "fn add(a, b) {\nreturn a + b\n}\n";
        let result = format_rad(source);
        assert!(result.contains("  return"));
    }

    #[test]
    fn nested_blocks() {
        let source = "fn test() {\nif true {\nprint(1)\n}\n}\n";
        let result = format_rad(source);
        let print_line = result
            .lines()
            .find(|l| l.contains("print"))
            .expect("print line");
        assert!(print_line.starts_with("    "));
    }

    #[test]
    fn pipeline_continuation_is_indented() {
        // A `|>` line continues the previous statement; its brackets balance
        // on the line (net 0), so the level stack never accounted for it and
        // it flattened to the statement's column — column 0 at top level.
        // It must indent one step past the statement it continues.
        let source =
            "let result = [1, 2, 3]\n|> filter(fn(x) { return x > 2 })\n|> map(fn(x) { return x * 10 })\n";
        let result = format_rad(source);
        assert!(
            result.contains("\n    |> filter("),
            "pipeline continuation should indent one level, got:\n{}",
            result
        );
        assert!(
            result.contains("\n    |> map("),
            "second continuation should indent too, got:\n{}",
            result
        );
        // Idempotent: formatting the formatted text is a fixed point.
        assert_eq!(format_rad(&result), result);
    }

    #[test]
    fn nested_pipeline_continuation_tracks_block_depth() {
        // Inside a block the continuation indents one past the statement's
        // own (non-zero) level, not to column 0 and not to a fixed depth.
        let source =
            "fn f(xs: list) -> int {\nlet total = xs\n|> filter(fn(x) { return x > 2 })\n|> reduce(0, fn(a, b) { return a + b })\nreturn total\n}\n";
        let result = format_rad(source);
        assert!(
            result.contains("\n        |> filter("),
            "continuation inside a fn body should indent to level 2 (8 spaces), got:\n{}",
            result
        );
        assert_eq!(format_rad(&result), result);
    }

    #[test]
    fn blank_line_between_decls() {
        let source = "component A {\nx: 1\n}\ncomponent B {\ny: 2\n}\n";
        let result = format_rad(source);
        assert!(result.contains("\n\n"));
    }

    #[test]
    fn operator_spacing() {
        let source = "let x=1+2\n";
        let result = format_rad(source);
        assert!(result.contains("= 1"));
    }

    #[test]
    fn pipe_spacing() {
        let source = "let x = [1]|>map\n";
        let result = format_rad(source);
        assert!(result.contains("|>"));
    }

    #[test]
    fn unary_minus_stays_attached() {
        // unary: no space between `-` and its operand
        assert!(format_rad("let x = -1\n").contains("= -1"));
        assert!(format_rad("print(-1)\n").contains("(-1)"));
        assert!(format_rad("let z = (a, -4)\n").contains(", -4)"));
        // binary stays spaced, even against a unary right operand
        let r = format_rad("let y = 3 - -2\n");
        assert!(r.contains("3 - -2"), "got: {}", r);
        let r = format_rad("let w = x-1\n");
        assert!(r.contains("x - 1"), "got: {}", r);
    }

    #[test]
    fn trailing_newline() {
        let source = "let x = 1";
        let result = format_rad(source);
        assert!(result.ends_with('\n'));
    }

    #[test]
    fn idempotent() {
        let source = "component Pos {\n  x: 0.0\n  y: 0.0\n}\n";
        let first = format_rad(source);
        let second = format_rad(&first);
        assert_eq!(first, second);
    }

    #[test]
    fn removes_excessive_blank_lines() {
        let source = "let x = 1\n\n\n\nlet y = 2\n";
        let result = format_rad(source);
        assert!(!result.contains("\n\n\n"));
    }

    #[test]
    fn preserves_comments() {
        let source = "// this is important\nlet x = 1\n";
        let result = format_rad(source);
        assert!(result.contains("// this is important"));
    }

    #[test]
    fn close_brace_dedent() {
        let source = "fn f() {\nlet x = 1\n}\n";
        let result = format_rad(source);
        let lines: Vec<&str> = result.trim().lines().collect();
        assert_eq!(lines.last().copied(), Some("}"));
    }

    #[test]
    fn four_space_unit() {
        let result = format_rad("fn f() {\nreturn 1\n}\n");
        assert!(result.contains("\n    return 1\n"), "got: {}", result);
    }

    #[test]
    fn multiline_closure_argument_continuation() {
        // the `})` closing a closure-in-call dedents to the let's level,
        // and the closure body indents one past it (paren + brace are
        // both tracked, so this used to land at column 0)
        let src = "fn f() {\nlet xs = q |> filter(fn(e) {\nreturn e > 0\n})\nreturn xs\n}\n";
        let result = format_rad(src);
        assert!(result.contains("\n    let xs"), "got: {}", result);
        assert!(
            result.contains("\n        return e > 0\n"),
            "got: {}",
            result
        );
        assert!(result.contains("\n    })\n"), "got: {}", result);
    }

    #[test]
    fn braces_inside_strings_do_not_indent() {
        let src = "fn f() {\nprint(\"{\")\nreturn 1\n}\n";
        let result = format_rad(src);
        assert!(result.contains("\n    return 1\n"), "got: {}", result);
        assert!(result.trim_end().ends_with("\n}"), "got: {}", result);
    }

    #[test]
    fn trailing_comment_alignment_preserved() {
        let src = "fn f() {\nlet x = 1      // aligned note\n}\n";
        let result = format_rad(src);
        assert!(
            result.contains("let x = 1      // aligned note"),
            "got: {}",
            result
        );
    }

    #[test]
    fn triple_fstring_content_preserved() {
        let source = "let code = f\"\"\"\n    if (x) {y=1;}\n\"\"\"\n";
        let result = format_rad(source);
        assert!(
            result.contains("    if (x) {y=1;}"),
            "Triple-fstring content was reformatted: {}",
            result,
        );
    }

    #[test]
    fn generic_type_annotations_stay_tight() {
        let src = "fn f(xs: list<int>) -> Result<list<int>, str> {\n    return Ok(xs)\n}\n";
        let result = format_rad(src);
        assert!(result.contains("(xs: list<int>)"), "got: {}", result);
        assert!(
            result.contains("-> Result<list<int>, str> {"),
            "got: {}",
            result
        );
        assert_eq!(format_rad(&result), result, "must be idempotent");
    }

    #[test]
    fn generic_fn_type_params_stay_tight() {
        let src = "fn generic_fn<T>(x: T) -> T {\n    return x\n}\nlet annotated: list<int> = [1, 2, 3]\nlet m: map<str, any> = { \"a\": 1 }\n";
        let result = format_rad(src);
        assert!(
            result.contains("fn generic_fn<T>(x: T) -> T {"),
            "got: {}",
            result
        );
        assert!(
            result.contains("let annotated: list<int> = [1, 2, 3]"),
            "got: {}",
            result
        );
        assert!(
            result.contains("let m: map<str, any> = {"),
            "got: {}",
            result
        );
        assert_eq!(format_rad(&result), result, "must be idempotent");
    }

    #[test]
    fn mangled_generic_spacing_heals() {
        // files corrupted by the old operator-spacing rule format back to
        // the tight spelling the author wrote
        let src = "fn f(xs: list < int > ) -> Result < list < int > , str > {\n    return Ok(xs)\n}\n\nfn generic_fn < T > (x: T) -> T {\n    return x\n}\n";
        let result = format_rad(src);
        assert!(result.contains("(xs: list<int>)"), "got: {}", result);
        assert!(
            result.contains("-> Result<list<int>, str> {"),
            "got: {}",
            result
        );
        assert!(
            result.contains("fn generic_fn<T>(x: T) -> T {"),
            "got: {}",
            result
        );
        assert_eq!(format_rad(&result), result, "must be idempotent");
    }

    #[test]
    fn comparisons_keep_operator_spacing() {
        let result =
            format_rad("let ok = a<b\nif n < 0 {\n    print(1)\n}\nlet cmp = len(xs) < n\nlet s = 8 >> 2\nlet chain = x < y > z\n");
        assert!(result.contains("let ok = a < b"), "got: {}", result);
        assert!(result.contains("if n < 0 {"), "got: {}", result);
        assert!(result.contains("len(xs) < n"), "got: {}", result);
        assert!(result.contains("8 >> 2"), "got: {}", result);
        assert!(result.contains("x < y > z"), "got: {}", result);
    }

    #[test]
    fn block_comment_lines_preserved_verbatim() {
        let src = "/* Header.\n   continuation: x=1*3\n\n   after the blank line\n*/\nlet x = 1\n";
        let result = format_rad(src);
        assert!(
            result.contains("   continuation: x=1*3\n"),
            "comment text was reformatted: {}",
            result
        );
        assert!(
            result.contains("\n\n   after the blank line\n"),
            "blank comment line was dropped: {}",
            result
        );
        assert!(result.contains("\n*/\n"), "got: {}", result);
        assert!(
            !result.contains("* /"),
            "block comment terminator was split: {}",
            result
        );
        assert_eq!(format_rad(&result), result, "must be idempotent");
    }

    #[test]
    fn nested_block_comment_close_tracked() {
        let src = "/* outer /* inner */ still comment\ntail */\nlet x = 1\n";
        let result = format_rad(src);
        assert!(result.contains("tail */\n"), "got: {}", result);
        assert!(result.contains("let x = 1"), "got: {}", result);
        assert_eq!(format_rad(&result), result, "must be idempotent");
    }

    #[test]
    fn code_line_opening_block_comment_preserved() {
        let src = "let x = 1 /* trailing note\nstill the note */\nlet y = 2\n";
        let result = format_rad(src);
        assert!(
            result.contains("let x = 1 /* trailing note\n"),
            "got: {}",
            result
        );
        assert!(result.contains("still the note */\n"), "got: {}", result);
        assert!(result.contains("let y = 2"), "got: {}", result);
    }
}
