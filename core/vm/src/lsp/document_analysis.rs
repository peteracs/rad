

/// Parse `system::` path starting at `pos`, reading only `chars[pos..limit)` (exclusive).
fn parse_system_ref_path_prefix(
    chars: &[char],
    mut pos: usize,
    limit: usize,
) -> (Vec<String>, usize) {
    let limit = limit.min(chars.len());
    let mut segments = Vec::new();
    while pos < limit {
        pos = skip_chars_ws(chars, pos);
        if pos >= limit {
            break;
        }
        if is_system_path_delimiter(chars[pos]) {
            break;
        }
        let start = pos;
        while pos < limit && (chars[pos].is_alphanumeric() || chars[pos] == '_') {
            pos += 1;
        }
        if start == pos {
            break;
        }
        segments.push(chars[start..pos].iter().collect());
        pos = skip_chars_ws(chars, pos);
        if pos + 1 < limit && chars[pos] == ':' && chars[pos + 1] == ':' {
            pos += 2;
            continue;
        }
        break;
    }
    (segments, pos)
}

/// First index at or after `after_parse` that ends the `system::` expression (`]` `,` `;` etc., ignoring leading whitespace).
fn system_ref_expression_end(chars: &[char], after_parse: usize) -> usize {
    let p = skip_chars_ws(chars, after_parse);
    if p < chars.len() && is_system_path_delimiter(chars[p]) {
        return p;
    }
    after_parse.max(p)
}

/// Rightmost `system::` reference containing `char_col`; path parsed up to (and including) the cursor for the last segment.
fn system_ref_path_at(line: &str, char_col: usize) -> Option<Vec<String>> {
    let chars: Vec<char> = line.chars().collect();
    if char_col > chars.len() {
        return None;
    }
    let needle: Vec<char> = "system::".chars().collect();
    let mut best_ps: Option<usize> = None;
    let mut i = 0;
    while i + needle.len() <= chars.len() {
        if chars[i..i + needle.len()] == needle[..] {
            let path_start = i + needle.len();
            if char_col < path_start {
                i += 1;
                continue;
            }
            let (_, end_parse) = parse_system_ref_path_prefix(&chars, path_start, chars.len());
            let end_expr = system_ref_expression_end(&chars, end_parse);
            if char_col >= path_start
                && char_col < end_expr
                && best_ps.map(|bps| path_start > bps).unwrap_or(true)
            {
                best_ps = Some(path_start);
            }
        }
        i += 1;
    }
    let ps = best_ps?;
    let limit = char_col.saturating_add(1).min(chars.len());
    let (segs, _) = parse_system_ref_path_prefix(&chars, ps, limit);
    if segs.is_empty() {
        None
    } else {
        Some(segs)
    }
}

/// When the cursor is inside a `system::` path, `(segments before the last `::`, partial last segment)` for completions.
fn system_path_completion_prefix(line: &str, char_col: usize) -> Option<(Vec<String>, String)> {
    let chars: Vec<char> = line.chars().collect();
    if char_col > chars.len() {
        return None;
    }
    let needle: Vec<char> = "system::".chars().collect();
    let mut best_ps: Option<usize> = None;
    let mut i = 0;
    while i + needle.len() <= chars.len() {
        if chars[i..i + needle.len()] == needle[..] {
            let path_start = i + needle.len();
            if char_col < path_start {
                i += 1;
                continue;
            }
            let (_, end_parse) = parse_system_ref_path_prefix(&chars, path_start, chars.len());
            let end_expr = system_ref_expression_end(&chars, end_parse);
            if char_col >= path_start
                && char_col < end_expr
                && best_ps.map(|bps| path_start > bps).unwrap_or(true)
            {
                best_ps = Some(path_start);
            }
        }
        i += 1;
    }
    let ps = best_ps?;
    let (mut segs, _) = parse_system_ref_path_prefix(&chars, ps, char_col);
    if segs.is_empty() {
        Some((Vec::new(), String::new()))
    } else {
        let partial = segs.pop().unwrap();
        Some((segs, partial))
    }
}

fn is_relation_document(text: &str) -> bool {
    text.lines().map(str::trim_start).any(|line| {
        line.starts_with("relation ")
            || line.starts_with("derive ")
            || line.starts_with("Insert(")
            || line.starts_with("insert(")
            || line.starts_with("Remove(")
            || line.starts_with("remove(")
            || line.starts_with("ReplaceBy(")
            || line.starts_with("replace_by(")
    })
}

fn relation_options(path: &Path, text: &str) -> crate::relation_frontend::FrontendOptions {
    let module_id = relation_module_directive(text)
        .map(str::to_string)
        .unwrap_or_else(|| {
            let stem = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("facts");
            let sanitized = stem
                .chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric() || character == '_' {
                        character
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            format!("workspace::{sanitized}")
        });
    crate::relation_frontend::FrontendOptions {
        enabled: true,
        module_id,
        ..relation_default_options()
    }
}

fn relation_module_directive(text: &str) -> Option<&str> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("// module:"))
        .map(str::trim)
        .filter(|module| !module.is_empty())
}

fn relation_default_options() -> crate::relation_frontend::FrontendOptions {
    crate::relation_frontend::FrontendOptions {
        enabled: true,
        module_id: "workspace::facts".to_string(),
        ..crate::relation_frontend::FrontendOptions::default()
    }
}

#[cfg(test)]
mod lsp_position_tests {
    use super::*;

    #[test]
    fn utf16_col_ascii() {
        assert_eq!(utf16_col_to_char_idx("abc", 0), 0);
        assert_eq!(utf16_col_to_char_idx("abc", 2), 2);
        assert_eq!(utf16_col_to_char_idx("abc", 3), 3);
    }

    #[test]
    fn utf16_col_before_supplementary() {
        let s = "a😀b";
        assert_eq!(utf16_col_to_char_idx(s, 0), 0);
        assert_eq!(utf16_col_to_char_idx(s, 1), 1);
        assert_eq!(utf16_col_to_char_idx(s, 2), 1);
        assert_eq!(utf16_col_to_char_idx(s, 3), 2);
        assert_eq!(utf16_col_to_char_idx(s, 4), 3);
    }

    #[test]
    fn document_end_position_uses_lsp_utf16_columns() {
        assert_eq!(
            document_end_position("one\r\ntwo\u{1F600}"),
            Position::new(1, 5)
        );
        assert_eq!(document_end_position("one\n"), Position::new(1, 0));
    }

    #[test]
    fn system_ref_path_picks_rightmost_on_line() {
        let line = "[system::A, system::B]";
        let idx_b = line.chars().position(|c| c == 'B').unwrap();
        assert_eq!(system_ref_path_at(line, idx_b), Some(vec!["B".to_string()]));
        let idx_a = line.chars().position(|c| c == 'A').unwrap();
        assert_eq!(system_ref_path_at(line, idx_a), Some(vec!["A".to_string()]));
    }

    #[test]
    fn relation_documents_and_module_directives_are_detected_without_runtime_state() {
        let source = "// module: game::facts\nrelation Owns(owner: entity, item: entity)\n";
        assert!(is_relation_document(source));
        let options = relation_options(Path::new("facts.rad"), source);
        assert_eq!(options.module_id, "game::facts");
        assert!(crate::relation_frontend::compile(source, &options).is_ok());
        assert!(is_relation_document("insert(Owns, (alice, sword))\n"));
        assert!(is_relation_document("remove(Owns, (alice, sword))\n"));
        assert!(is_relation_document(
            "replace_by(Owns, item, sword, (alice, sword))\n"
        ));
    }
}
