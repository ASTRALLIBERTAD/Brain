pub mod token;
pub use token::{Keyword, PrimitiveType, Token, TokenKind};

pub struct Lexer<'a> {
    source: &'a str,
    filename: &'a str,
    chars: Vec<char>,
    current: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str, filename: &'a str) -> Self {
        Lexer {
            source,
            filename,
            chars: source.chars().collect(),
            current: 0,
            line: 1,
            column: 1,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();
        while !self.is_at_end() {
            self.skip_whitespace_and_comments();
            if self.is_at_end() {
                break;
            }
            tokens.push(self.next_token()?);
        }
        tokens.push(Token::new(TokenKind::Eof, self.line, self.column));
        Ok(tokens)
    }

    fn next_token(&mut self) -> Result<Token, String> {
        let line = self.line;
        let col = self.column;
        let ch = self.peek();

        let kind = match ch {
            // Single-char unambiguous tokens
            '+' => { self.advance(); TokenKind::Plus }
            '*' => { self.advance(); TokenKind::Star }
            '/' => { self.advance(); TokenKind::Slash }
            '%' => { self.advance(); TokenKind::Percent }
            '(' => { self.advance(); TokenKind::LParen }
            ')' => { self.advance(); TokenKind::RParen }
            '{' => { self.advance(); TokenKind::LBrace }
            '}' => { self.advance(); TokenKind::RBrace }
            '[' => { self.advance(); TokenKind::LBracket }
            ']' => { self.advance(); TokenKind::RBracket }
            ';' => { self.advance(); TokenKind::Semicolon }
            ',' => { self.advance(); TokenKind::Comma }

            // Two-char possibilities
            '-' => { self.advance(); if self.peek() == '>' { self.advance(); TokenKind::Arrow } else { TokenKind::Minus } }
            '=' => { self.advance(); match self.peek() { '=' => { self.advance(); TokenKind::EqualEqual } '>' => { self.advance(); TokenKind::FatArrow } _ => TokenKind::Assign } }
            '<' => { self.advance(); if self.peek() == '=' { self.advance(); TokenKind::LessEqual } else { TokenKind::LessThan } }
            '>' => { self.advance(); if self.peek() == '=' { self.advance(); TokenKind::GreaterEqual } else { TokenKind::GreaterThan } }
            '!' => { self.advance(); if self.peek() == '=' { self.advance(); TokenKind::NotEqual } else { TokenKind::Not } }
            '&' => { self.advance(); if self.peek() == '&' { self.advance(); TokenKind::And } else { TokenKind::Ampersand } }
            ':' => { self.advance(); if self.peek() == ':' { self.advance(); TokenKind::DoubleColon } else { TokenKind::Colon } }
            '.' => { self.advance(); if self.peek() == '.' { self.advance(); TokenKind::DotDot } else { TokenKind::Dot } }

            '|' => {
                self.advance();
                if self.peek() == '|' { self.advance(); TokenKind::Or }
                else { return Err(self.error("Unexpected character '|' — did you mean '||'?")); }
            }

            '"'  => self.lex_string()?,
            '\'' => self.lex_char()?,

            c if c.is_ascii_digit()            => self.lex_integer(),
            c if c.is_alphabetic() || c == '_' => self.lex_word(),

            c => return Err(self.error(&format!("Unexpected character '{}'", c))),
        };

        Ok(Token::new(kind, line, col))
    }

    // Literal lexers

    fn lex_string(&mut self) -> Result<TokenKind, String> {
        self.advance(); // opening "
        let mut buf = String::new();
        while !self.is_at_end() && self.peek() != '"' {
            if self.peek() == '\n' {
                return Err(self.error("Unterminated string literal — strings cannot span lines"));
            }
            if self.peek() == '\\' {
                self.advance();
                buf.push(self.lex_escape()?);
            } else {
                buf.push(self.advance());
            }
        }
        if self.is_at_end() {
            return Err(self.error("Unterminated string literal (missing closing '\"')"));
        }
        self.advance(); // closing "
        Ok(TokenKind::StringLit(buf))
    }

    fn lex_char(&mut self) -> Result<TokenKind, String> {
        self.advance(); // opening '
        if self.is_at_end() {
            return Err(self.error("Unterminated character literal"));
        }
        let ch = if self.peek() == '\\' {
            self.advance();
            self.lex_escape()?
        } else {
            self.advance()
        };
        if self.peek() != '\'' {
            return Err(self.error("Unterminated character literal (expected closing \"'\")"));
        }
        self.advance(); // closing '
        Ok(TokenKind::CharLit(ch))
    }

    fn lex_escape(&mut self) -> Result<char, String> {
        let c = match self.peek() {
            'n'  => '\n',
            't'  => '\t',
            'r'  => '\r',
            '\\' => '\\',
            '"'  => '"',
            '\'' => '\'',
            c    => return Err(self.error(&format!("Unknown escape sequence '\\{}'", c))),
        };
        self.advance();
        Ok(c)
    }

    fn lex_integer(&mut self) -> TokenKind {
        let mut buf = String::new();
        while !self.is_at_end() && self.peek().is_ascii_digit() {
            buf.push(self.advance());
        }
        TokenKind::Integer(buf.parse().unwrap_or(0))
    }

    /// Identifiers resolve to Keyword → PrimitiveType → Identifier in that order.
    fn lex_word(&mut self) -> TokenKind {
        let mut buf = String::new();
        while !self.is_at_end() {
            let c = self.peek();
            if c.is_alphanumeric() || c == '_' { buf.push(self.advance()); } else { break; }
        }
        if let Some(kw) = Keyword::from_str(&buf) {
            return TokenKind::Keyword(kw);
        }
        if let Some(pt) = PrimitiveType::from_str(&buf) {
            return TokenKind::PrimitiveType(pt);
        }
        TokenKind::Identifier(buf)
    }

    // Whitespace / comments

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                ' ' | '\t' | '\r' => { self.advance(); }
                '\n' => { self.advance(); self.line += 1; self.column = 1; }
                '/' if self.peek_ahead(1) == '/' => {
                    while !self.is_at_end() && self.peek() != '\n' { self.advance(); }
                }
                _ => break,
            }
        }
    }

    // Diagnostics

    fn error(&self, message: &str) -> String {
        let lines: Vec<&str> = self.source.lines().collect();
        let src_line = lines.get(self.line - 1).unwrap_or(&"");
        let w = self.line.to_string().len().max(3);
        format!(
            "\x1b[1;31merror\x1b[0m: {}\n  \x1b[1;34m-->\x1b[0m {}:{}:{}\n{0:w$} \x1b[1;34m|\x1b[0m\n\x1b[1;34m{1:w$} |\x1b[0m {2}\n{0:w$} \x1b[1;34m|\x1b[0m {3}\x1b[1;31m^\x1b[0m\n",
            "",
            self.line,
            src_line,
            " ".repeat(self.column.saturating_sub(1)),
            w = w,
        ) + message + "\n"
            + &format!("  \x1b[1;34m-->\x1b[0m {}:{}:{}", self.filename, self.line, self.column)
    }

    // Char helpers

    fn peek(&self) -> char {
        if self.is_at_end() { '\0' } else { self.chars[self.current] }
    }

    fn peek_ahead(&self, offset: usize) -> char {
        self.chars.get(self.current + offset).copied().unwrap_or('\0')
    }

    fn advance(&mut self) -> char {
        let ch = self.chars[self.current];
        self.current += 1;
        self.column += 1;
        ch
    }

    fn is_at_end(&self) -> bool {
        self.current >= self.chars.len()
    }
}
