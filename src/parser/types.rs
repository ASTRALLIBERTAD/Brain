use crate::ast::Parameter;
use crate::lexer::{Keyword, TokenKind};
use super::Parser;

impl<'a> Parser<'a> {
    // Types

    pub(crate) fn parse_type(&mut self) -> Result<String, String> {
        match self.peek().kind.clone() {
            // int / bool / string / char — the separated PrimitiveType variant
            TokenKind::PrimitiveType(pt) => {
                self.advance();
                Ok(pt.as_str().to_string())
            }

            // &T or &mut T
            TokenKind::Ampersand => {
                self.advance();
                let mutable = self.eat_keyword(&Keyword::Mut);
                let inner = self.parse_type()?;
                Ok(if mutable { format!("&mut {}", inner) } else { format!("&{}", inner) })
            }

            // [T; N]
            TokenKind::LBracket => {
                self.advance();
                let elem = self.parse_type()?;
                self.expect(&TokenKind::Semicolon, "Expected ';' in array type [T; N]")?;
                let size = match self.peek().kind.clone() {
                    TokenKind::Integer(n) => { self.advance(); n as usize }
                    _ => return Err(self.error("Expected array size (integer) in [T; N]")),
                };
                self.expect(&TokenKind::RBracket, "Expected ']' to close array type")?;
                Ok(format!("[{}; {}]", elem, size))
            }

            // Named types: Vec, Mutex<T>, or user-defined struct/enum names
            // Imma change this later, for now lets keep this. not a good code
            TokenKind::Identifier(name) => {
                self.advance();
                match name.as_str() {
                    "Vec" => Ok("Vec".to_string()),
                    "Mutex" => {
                        self.expect(&TokenKind::LessThan, "Expected '<' after 'Mutex'")?;
                        let inner = self.parse_type()?;
                        self.expect(&TokenKind::GreaterThan, "Expected '>' to close 'Mutex<...>'")?;
                        Ok(format!("Mutex<{}>", inner))
                    }
                    _ => Ok(name),
                }
            }

            _ => Err(self.error("Expected a type")),
        }
    }

    // Parameters

    pub(crate) fn parse_parameters(&mut self) -> Result<Vec<Parameter>, String> {
        let mut params = Vec::new();
        if self.check(&TokenKind::RParen) {
            return Ok(params); // empty list
        }
        loop {
            // `&` and `mut` before parameter name
            let is_reference = self.eat(&TokenKind::Ampersand);
            let is_mutable   = self.eat_keyword(&Keyword::Mut);
            let name         = self.expect_identifier("Expected parameter name")?;
            self.expect(&TokenKind::Colon, "Expected ':' after parameter name")?;
            let param_type   = self.parse_type()?;

            params.push(Parameter { is_reference, is_mutable, name, param_type });

            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        Ok(params)
    }
}
