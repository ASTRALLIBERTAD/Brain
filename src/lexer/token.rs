// Not all keywords are the same — grouping them makes the parser's match
// arms self-documenting and prevents accidentally handling a control-flow
// keyword where a declaration keyword is expected (and vice-versa).

// Keyword categories

#[derive(Debug, Clone, PartialEq)]
pub enum Keyword {
    // Declaration — introduce new bindings
    Let,
    Mut,
    Fn,
    Struct,
    Enum,

    // Control flow — alter execution path
    If,
    Else,
    While,
    For,
    In,
    Return,
    Break,
    Continue,
    Match,

    // Module system
    Export,
    Import,
    From,

    // Safety annotation
    Unsafe,

    // Boolean literals look like keywords to the lexer
    True,
    False,
}
// I'm  gonna refactor it nextime, I gonna keep this as is for now
impl Keyword {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "let" => Some(Keyword::Let),
            "mut" => Some(Keyword::Mut),
            "fn" => Some(Keyword::Fn),
            "struct" => Some(Keyword::Struct),
            "enum" => Some(Keyword::Enum),
            "if" => Some(Keyword::If),
            "else" => Some(Keyword::Else),
            "while" => Some(Keyword::While),
            "for" => Some(Keyword::For),
            "in" => Some(Keyword::In),
            "return" => Some(Keyword::Return),
            "break" => Some(Keyword::Break),
            "continue" => Some(Keyword::Continue),
            "match" => Some(Keyword::Match),
            "export" => Some(Keyword::Export),
            "import" => Some(Keyword::Import),
            "from" => Some(Keyword::From),
            "unsafe" => Some(Keyword::Unsafe),
            "true" => Some(Keyword::True),
            "false" => Some(Keyword::False),
            _ => None,
        }
    }
}

// Primitive types
// Separated from keywords because they only appear in type positions, not
// expression positions.  Mixing them into Keyword means every expression
// parser arm would need to explicitly reject them.

#[derive(Debug, Clone, PartialEq)]
pub enum PrimitiveType {
    Int,
    Bool,
    String,
    Char,
}

impl PrimitiveType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "int" => Some(PrimitiveType::Int),
            "bool" => Some(PrimitiveType::Bool),
            "string" => Some(PrimitiveType::String),
            "char" => Some(PrimitiveType::Char),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            PrimitiveType::Int => "int",
            PrimitiveType::Bool => "bool",
            PrimitiveType::String => "string",
            PrimitiveType::Char => "char",
        }
    }
}

// Token kind

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Named groups
    Keyword(Keyword),
    PrimitiveType(PrimitiveType),

    // Literals
    Integer(i64),
    StringLit(String),
    CharLit(char),

    // Names
    Identifier(String),

    // Arithmetic
    Plus,
    Minus,
    Star,
    Slash,
    Percent,

    // Comparison
    EqualEqual,
    NotEqual,
    LessThan,
    LessEqual,
    GreaterThan,
    GreaterEqual,

    // Logical
    And, // &&
    Or,  // ||
    Not, // !

    // Assignment / reference
    Assign,    // =
    Ampersand, // &  (single)

    // Paired delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,

    // Punctuation
    Semicolon,
    Colon,
    DoubleColon, // ::  (emitted directly — was peek-ahead hacked before)
    Comma,
    Dot,
    DotDot, // ..

    // Arrows
    Arrow,    // ->
    FatArrow, // =>

    Eof,
}

impl TokenKind {
    /// Human-readable description for error messages.
    pub fn description(&self) -> std::borrow::Cow<'static, str> {
        match self {
            TokenKind::Keyword(k) => {
                let s = match k {
                    Keyword::Let => "'let'",
                    Keyword::Mut => "'mut'",
                    Keyword::Fn => "'fn'",
                    Keyword::Struct => "'struct'",
                    Keyword::Enum => "'enum'",
                    Keyword::If => "'if'",
                    Keyword::Else => "'else'",
                    Keyword::While => "'while'",
                    Keyword::For => "'for'",
                    Keyword::In => "'in'",
                    Keyword::Return => "'return'",
                    Keyword::Break => "'break'",
                    Keyword::Continue => "'continue'",
                    Keyword::Match => "'match'",
                    Keyword::Export => "'export'",
                    Keyword::Import => "'import'",
                    Keyword::From => "'from'",
                    Keyword::Unsafe => "'unsafe'",
                    Keyword::True => "'true'",
                    Keyword::False => "'false'",
                };
                s.into()
            }
            TokenKind::PrimitiveType(p) => match p {
                PrimitiveType::Int => "'int'".into(),
                PrimitiveType::Bool => "'bool'".into(),
                PrimitiveType::String => "'string'".into(),
                PrimitiveType::Char => "'char'".into(),
            },
            TokenKind::Integer(n) => format!("integer {}", n).into(),
            TokenKind::StringLit(_) => "string literal".into(),
            TokenKind::CharLit(_) => "char literal".into(),
            TokenKind::Identifier(n) => format!("identifier `{}`", n).into(),
            TokenKind::Plus => "'+'".into(),
            TokenKind::Minus => "'-'".into(),
            TokenKind::Star => "'*'".into(),
            TokenKind::Slash => "'/'".into(),
            TokenKind::Percent => "'%'".into(),
            TokenKind::EqualEqual => "'=='".into(),
            TokenKind::NotEqual => "'!='".into(),
            TokenKind::LessThan => "'<'".into(),
            TokenKind::LessEqual => "'<='".into(),
            TokenKind::GreaterThan => "'>'".into(),
            TokenKind::GreaterEqual => "'>='".into(),
            TokenKind::And => "'&&'".into(),
            TokenKind::Or => "'||'".into(),
            TokenKind::Not => "'!'".into(),
            TokenKind::Assign => "'='".into(),
            TokenKind::Ampersand => "'&'".into(),
            TokenKind::LParen => "'('".into(),
            TokenKind::RParen => "')'".into(),
            TokenKind::LBrace => "'{'".into(),
            TokenKind::RBrace => "'}'".into(),
            TokenKind::LBracket => "'['".into(),
            TokenKind::RBracket => "']'".into(),
            TokenKind::Semicolon => "';'".into(),
            TokenKind::Colon => "':'".into(),
            TokenKind::DoubleColon => "'::'".into(),
            TokenKind::Comma => "','".into(),
            TokenKind::Dot => "'.'".into(),
            TokenKind::DotDot => "'..'".into(),
            TokenKind::Arrow => "'->'".into(),
            TokenKind::FatArrow => "'=>'".into(),
            TokenKind::Eof => "end of file".into(),
        }
    }

    /// True for token kinds that typically begin a new top-level item or
    /// statement.  The parser's `synchronize()` uses this to find safe
    /// re-entry points after an error instead of cascading to EOF.
    pub fn is_statement_boundary(&self) -> bool {
        matches!(
            self,
            TokenKind::Keyword(
                Keyword::Fn
                    | Keyword::Let
                    | Keyword::If
                    | Keyword::While
                    | Keyword::For
                    | Keyword::Match
                    | Keyword::Return
                    | Keyword::Break
                    | Keyword::Continue
                    | Keyword::Struct
                    | Keyword::Enum
                    | Keyword::Import
                    | Keyword::Export
                    | Keyword::Unsafe
            ) | TokenKind::RBrace
        )
    }
}

// Token

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
}

impl Token {
    pub fn new(kind: TokenKind, line: usize, column: usize) -> Self {
        Token { kind, line, column }
    }
}
