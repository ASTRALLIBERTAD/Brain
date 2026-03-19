use crate::ast::Pattern;
use crate::lexer::TokenKind;
use super::Parser;

impl<'a> Parser<'a> {
    pub(crate) fn parse_pattern(&mut self) -> Result<Pattern, String> {
        match self.peek().kind.clone() {
            TokenKind::Integer(n) => {
                self.advance();
                Ok(Pattern::NumberPattern(n))
            }

            // Negative number pattern: -N
            TokenKind::Minus => {
                self.advance();
                match self.peek().kind.clone() {
                    TokenKind::Integer(n) => { self.advance(); Ok(Pattern::NumberPattern(-n)) }
                    _ => Err(self.error("Expected a number after '-' in match pattern")),
                }
            }

            TokenKind::StringLit(s) => {
                self.advance();
                Ok(Pattern::StringPattern(s))
            }

            TokenKind::Identifier(name) => {
                self.advance();

                if name == "_" {
                    return Ok(Pattern::Wildcard);
                }

                // EnumName::Variant  or  EnumName::Variant(binding)
                // DoubleColon is now a first-class token — no peek_ahead needed.
                if self.eat(&TokenKind::DoubleColon) {
                    let variant = self.expect_identifier("Expected variant name after '::'")?;
                    let binding = if self.eat(&TokenKind::LParen) {
                        let b = self.expect_identifier("Expected binding name")?;
                        self.expect(&TokenKind::RParen, "Expected ')'")?;
                        Some(b)
                    } else {
                        None
                    };
                    Ok(Pattern::EnumPattern { enum_name: name, variant, binding })
                } else {
                    Ok(Pattern::Identifier(name))
                }
            }

            _ => Err(self.error("Expected a match pattern")),
        }
    }
}
