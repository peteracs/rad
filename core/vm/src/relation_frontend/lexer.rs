use super::limits::RawInputMeter;
use super::{DiagnosticCode, FrontendDiagnostic};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TokenKind {
    Ident,
    Integer,
    String,
    LParen,
    RParen,
    Comma,
    Colon,
    Greater,
    DColon,
    Newline,
    Eof,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub column: u32,
}

pub(crate) struct Lexed {
    pub tokens: Vec<Token>,
    pub maximum_identifier_length: usize,
}

impl Token {
    pub(crate) fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }
}

pub(crate) fn lex(source: &str, meter: &mut RawInputMeter) -> Result<Lexed, FrontendDiagnostic> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    let mut line = 1u32;
    let mut column = 1u32;
    let mut maximum_identifier_length = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b' ' | b'\t' | b'\r' => {
                index += 1;
                column += 1;
            }
            b'\n' => {
                push(
                    &mut tokens,
                    meter,
                    TokenKind::Newline,
                    index,
                    index + 1,
                    line,
                    column,
                )?;
                index += 1;
                line += 1;
                column = 1;
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                    column += 1;
                }
            }
            b'(' => single(
                &mut tokens,
                meter,
                TokenKind::LParen,
                &mut index,
                line,
                &mut column,
            )?,
            b')' => single(
                &mut tokens,
                meter,
                TokenKind::RParen,
                &mut index,
                line,
                &mut column,
            )?,
            b',' => single(
                &mut tokens,
                meter,
                TokenKind::Comma,
                &mut index,
                line,
                &mut column,
            )?,
            b'>' => single(
                &mut tokens,
                meter,
                TokenKind::Greater,
                &mut index,
                line,
                &mut column,
            )?,
            b':' if bytes.get(index + 1) == Some(&b':') => {
                push(
                    &mut tokens,
                    meter,
                    TokenKind::DColon,
                    index,
                    index + 2,
                    line,
                    column,
                )?;
                index += 2;
                column += 2;
            }
            b':' => single(
                &mut tokens,
                meter,
                TokenKind::Colon,
                &mut index,
                line,
                &mut column,
            )?,
            b'"' => {
                let start = index;
                let start_column = column;
                index += 1;
                column += 1;
                while index < bytes.len() && bytes[index] != b'"' && bytes[index] != b'\n' {
                    if bytes[index] == b'\\' && index + 1 < bytes.len() {
                        index += 1;
                        column += 1;
                    }
                    index += 1;
                    column += 1;
                }
                if bytes.get(index) != Some(&b'"') {
                    return Err(FrontendDiagnostic::new(
                        DiagnosticCode::Syntax,
                        "unterminated relation string literal",
                        line,
                        column,
                        &start.to_be_bytes(),
                    ));
                }
                index += 1;
                column += 1;
                push(
                    &mut tokens,
                    meter,
                    TokenKind::String,
                    start,
                    index,
                    line,
                    start_column,
                )?;
            }
            b'-' if bytes.get(index + 1).is_some_and(u8::is_ascii_digit) => {
                let start = index;
                let start_column = column;
                index += 1;
                column += 1;
                while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                    index += 1;
                    column += 1;
                }
                push(
                    &mut tokens,
                    meter,
                    TokenKind::Integer,
                    start,
                    index,
                    line,
                    start_column,
                )?;
            }
            byte if byte.is_ascii_digit() => {
                let start = index;
                let start_column = column;
                while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                    index += 1;
                    column += 1;
                }
                push(
                    &mut tokens,
                    meter,
                    TokenKind::Integer,
                    start,
                    index,
                    line,
                    start_column,
                )?;
            }
            byte if is_ident_start(byte) => {
                let start = index;
                let start_column = column;
                while bytes
                    .get(index)
                    .is_some_and(|byte| is_ident_continue(*byte))
                {
                    index += 1;
                    column += 1;
                }
                maximum_identifier_length = maximum_identifier_length.max(index - start);
                push(
                    &mut tokens,
                    meter,
                    TokenKind::Ident,
                    start,
                    index,
                    line,
                    start_column,
                )?;
            }
            other => {
                return Err(FrontendDiagnostic::new(
                    DiagnosticCode::Syntax,
                    format!("unexpected byte '{}' in relation source", other as char),
                    line,
                    column,
                    &[other],
                ));
            }
        }
    }
    meter.token()?;
    tokens.push(Token {
        kind: TokenKind::Eof,
        start: source.len(),
        end: source.len(),
        line,
        column,
    });
    Ok(Lexed {
        tokens,
        maximum_identifier_length,
    })
}

fn single(
    tokens: &mut Vec<Token>,
    meter: &mut RawInputMeter,
    kind: TokenKind,
    index: &mut usize,
    line: u32,
    column: &mut u32,
) -> Result<(), FrontendDiagnostic> {
    push(tokens, meter, kind, *index, *index + 1, line, *column)?;
    *index += 1;
    *column += 1;
    Ok(())
}

fn push(
    tokens: &mut Vec<Token>,
    meter: &mut RawInputMeter,
    kind: TokenKind,
    start: usize,
    end: usize,
    line: u32,
    column: u32,
) -> Result<(), FrontendDiagnostic> {
    meter.token()?;
    tokens.push(Token {
        kind,
        start,
        end,
        line,
        column,
    });
    Ok(())
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}
