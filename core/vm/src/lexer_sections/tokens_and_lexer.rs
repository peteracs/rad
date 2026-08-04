

/// The TokenType a word lexes to when it is a reserved keyword (e.g. "on").
/// Used by the parser to accept keywords in unambiguous field-name positions.
pub(crate) fn keyword_type_of(word: &str) -> Option<TokenType> {
    decl::keyword_token_type(word)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    Int,
    Float,
    String,
    Ident,

    Component,
    Struct,
    Entity,
    Resource,
    State,
    System,
    Event,
    On,
    Emit,
    Fn,
    Let,
    Mut,
    If,
    Else,
    While,
    For,
    In,
    Return,
    True,
    False,
    Nil,
    Schedule,
    And,
    Or,
    Not,
    Match,
    When,
    Use,
    Break,
    Continue,
    Type,
    Pure,
    Once,
    Async,
    Await,
    Rec,
    Pub,
    As,
    Indexed,
    Unique,

    FStringStart,
    FStringFragment,
    InterpolationStart,
    InterpolationEnd,
    FStringEnd,

    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Dot,
    DotDot,
    Colon,
    Arrow,
    FatArrow,
    PipeOp,
    Pipe,
    Amp,
    Caret,
    Question,
    Assign,
    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,
    LessLess,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    Tilde,
    DColon,

    Comment,
    Whitespace,
    Error,

    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub ty: TokenType,
    pub value: TokenValue,
    pub line: u32,
    pub col: u32,
    pub span: (usize, usize),
}

#[derive(Debug, Clone)]
pub enum TokenValue {
    None,
    IntVal(i64),
    FloatVal(f64),
    Str(String),
    Bool(bool),
}

impl fmt::Display for TokenValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenValue::None => write!(f, ""),
            TokenValue::IntVal(n) => write!(f, "{}", n),
            TokenValue::FloatVal(x) => write!(f, "{}", x),
            TokenValue::Str(s) => write!(f, "{}", s),
            TokenValue::Bool(b) => write!(f, "{}", b),
        }
    }
}

impl TokenValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            TokenValue::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            TokenValue::IntVal(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            TokenValue::FloatVal(x) => Some(*x),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            TokenValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct LexerError {
    pub message: String,
    pub line: u32,
    pub col: u32,
}

impl fmt::Display for LexerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[line {}, col {}] {}", self.line, self.col, self.message)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LexerMode {
    Normal,
    FString { triple: bool },
    Interpolation { brace_depth: u32 },
}

pub struct Lexer<'a> {
    source: &'a str,
    pos: usize,
    line: u32,
    col: u32,
    line_directives: Vec<(usize, u32, u32)>,
    next_line_directive: usize,
    pub preserve_comments: bool,
    pub(crate) mode_stack: Vec<LexerMode>,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self::new_with_offset(source, 1, 1)
    }

    pub fn new_with_offset(source: &'a str, line: u32, col: u32) -> Self {
        // Windows editors love to prepend a UTF-8 BOM; it is not part of the
        // program. Strip it here so every entry point (CLI, module loader,
        // sandbox guests, LSP) tolerates it uniformly.
        let source = source.strip_prefix('\u{feff}').unwrap_or(source);
        Self {
            source,
            pos: 0,
            line,
            col,
            line_directives: Vec::new(),
            next_line_directive: 0,
            preserve_comments: false,
            mode_stack: vec![LexerMode::Normal],
        }
    }

    /// Lex concatenated source using explicit, authenticated unit boundaries.
    /// Source comments remain ordinary comments and never affect locations.
    pub fn new_with_source_layout(source: &'a str, layout: &SourceLayout) -> Result<Self, String> {
        layout.validate(source)?;
        let mut lexer = Self::new(source);
        lexer.line_directives = layout
            .sections
            .iter()
            .map(|section| (section.byte_offset, section.line, section.column))
            .collect();
        lexer.apply_line_directives();
        Ok(lexer)
    }

    pub(crate) fn apply_line_directives(&mut self) {
        while let Some((offset, line, column)) =
            self.line_directives.get(self.next_line_directive).copied()
        {
            if offset != self.pos {
                break;
            }
            self.line = line;
            self.col = column;
            self.next_line_directive += 1;
        }
    }

    pub(crate) fn current_mode(&self) -> LexerMode {
        *self.mode_stack.last().unwrap_or(&LexerMode::Normal)
    }
}