// struct's impl across multiple files inside the same module if done right, I guess.

mod decl;
mod expr;
mod pattern;
mod stmt;
mod types;
pub mod error;

pub use error::ParseErrors;

use crate::ast::{AstNode, Location};
use crate::lexer::{Keyword, Token, TokenKind};

pub struct Parser<'a> {
    tokens: Vec<Token>,
    current: usize,
    filename: &'a str,
    /// When true, a bare `{` after an expression cannot start a struct
    /// literal — prevents `if condition { ... }` from being parsed as
    /// `if (condition { ... })`.
    pub(crate) no_struct_init: bool,
    /// Non-fatal errors accumulated during recovery so we can report
    /// everything in one pass rather than stopping at the first problem.
    pub(crate) errors: Vec<String>,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: Vec<Token>, filename: &'a str) -> Self {
        Parser {
            tokens,
            current: 0,
            filename,
            no_struct_init: false,
            errors: Vec::new(),
        }
    }

    /// Main entry point.  Returns a `Program` node, or all errors found.
    pub fn parse(&mut self) -> Result<AstNode, ParseErrors> {
        let mut nodes = Vec::new();

        while !self.is_at_end() {
            match self.parse_top_level_item() {
                Ok(node) => nodes.push(node),
                Err(e) => {
                    self.errors.push(e);
                    self.synchronize(); // skip to next safe point, keep parsing
                }
            }
        }

        if self.errors.is_empty() {
            Ok(AstNode::Program(nodes))
        } else {
            Err(ParseErrors(std::mem::take(&mut self.errors)))
        }
    }

    fn parse_top_level_item(&mut self) -> Result<AstNode, String> {
        match &self.peek().kind {
            TokenKind::Keyword(Keyword::Import) => self.parse_import(),
            TokenKind::Keyword(Keyword::Export) => self.parse_export(),
            TokenKind::Keyword(Keyword::Unsafe) => {
                self.advance();
                self.parse_function(false, true)
            }
            TokenKind::Keyword(Keyword::Fn)     => self.parse_function(false, false),
            TokenKind::Keyword(Keyword::Struct)  => self.parse_struct_def(),
            TokenKind::Keyword(Keyword::Enum)    => self.parse_enum_def(),
            _ => self.parse_statement(),
        }
    }

    // Error recovery

    /// Skip tokens until we reach a probable statement/declaration boundary.
    /// Hmmmm... Maybe this keeps a single bad token from poisoning the rest of the file.
    ///
    /// Example: missing semicolon on line 5 will cause an error, but the
    /// parser recovers at the next `fn`/`let`/`if`/`}` and continues
    /// parsing normally — you get all errors in one run.
    pub(crate) fn synchronize(&mut self) {
        while !self.is_at_end() {
            // Just consumed a semicolon — resume after the broken statement.
            if matches!(self.previous_kind(), TokenKind::Semicolon) {
                return;
            }
            // These tokens start fresh constructs.
            if self.peek().kind.is_statement_boundary() {
                return;
            }
            self.advance();
        }
    }

    // Core navigation

    pub(crate) fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    pub(crate) fn peek_kind(&self) -> &TokenKind {
        &self.tokens[self.current].kind
    }

    /// Look ahead by `offset` without consuming.
    pub(crate) fn peek_ahead(&self, offset: usize) -> &Token {
        let pos = (self.current + offset).min(self.tokens.len() - 1);
        &self.tokens[pos]
    }

    pub(crate) fn advance(&mut self) -> &Token {
        if !self.is_at_end() {
            self.current += 1;
        }
        &self.tokens[self.current - 1]
    }

    pub(crate) fn previous_kind(&self) -> &TokenKind {
        if self.current == 0 { &TokenKind::Eof } else { &self.tokens[self.current - 1].kind }
    }

    pub(crate) fn is_at_end(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    // Predicate helpers

    /// True if the current token has the same *variant* as `kind`
    /// (ignores payload — mirrors the old `std::mem::discriminant` approach).
    pub(crate) fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.peek_kind()) == std::mem::discriminant(kind)
    }

    pub(crate) fn check_keyword(&self, kw: &Keyword) -> bool {
        matches!(&self.peek().kind, TokenKind::Keyword(k) if k == kw)
    }

    pub(crate) fn check_identifier(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Identifier(_))
    }

    // Consuming helpers

    /// Advance if the current token matches `kind`, otherwise do nothing.
    pub(crate) fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) { self.advance(); true } else { false }
    }

    pub(crate) fn eat_keyword(&mut self, kw: &Keyword) -> bool {
        if self.check_keyword(kw) { self.advance(); true } else { false }
    }

    /// Advance and return `Ok(())`, or return a contextual error.
    pub(crate) fn expect(&mut self, kind: &TokenKind, msg: &str) -> Result<(), String> {
        if self.check(kind) { self.advance(); Ok(()) } else { Err(self.error(msg)) }
    }

    pub(crate) fn expect_keyword(&mut self, kw: &Keyword, msg: &str) -> Result<(), String> {
        if self.check_keyword(kw) { self.advance(); Ok(()) } else { Err(self.error(msg)) }
    }

    /// Consume an `Identifier` token and return its name.
    pub(crate) fn expect_identifier(&mut self, msg: &str) -> Result<String, String> {
        match self.peek().kind.clone() {
            TokenKind::Identifier(name) => { self.advance(); Ok(name) }
            _ => Err(self.error(msg)),
        }
    }

    pub(crate) fn current_location(&self) -> Location {
        Location::new(self.peek().line, self.peek().column)
    }

    // Error formatting

    pub(crate) fn error(&self, message: &str) -> String {
        let t = self.peek();
        // Include what was *found* — makes "Expected ';'" → "Expected ';', found 'fn'"
        // which is far more actionable than just "Expected ';'".
        format!(
            "{}:{}:{}: {} (found {})",
            self.filename, t.line, t.column, message,
            t.kind.description(),
        )
    }
}
