use crate::ast::{AstNode, BinOp, UnOp};
use crate::lexer::{Keyword, TokenKind};
use super::Parser;

impl<'a> Parser<'a> {
    // Entry point

    pub(crate) fn parse_expression(&mut self) -> Result<AstNode, String> {
        // Top-level `&expr` shorthand — also handled inside parse_unary.
        if self.eat(&TokenKind::Ampersand) {
            let expr = self.parse_or()?;
            return Ok(AstNode::Reference(Box::new(expr)));
        }
        self.parse_or()
    }

    // Precedence ladder (lowest → highest)

    fn parse_or(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_and()?;
        while self.eat(&TokenKind::Or) {
            let right = self.parse_and()?;
            left = AstNode::BinaryOp { op: BinOp::Or, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_comparison()?;
        while self.eat(&TokenKind::And) {
            let right = self.parse_comparison()?;
            left = AstNode::BinaryOp { op: BinOp::And, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::EqualEqual   => { self.advance(); BinOp::Equal }
                TokenKind::NotEqual     => { self.advance(); BinOp::NotEqual }
                TokenKind::LessThan     => { self.advance(); BinOp::LessThan }
                TokenKind::LessEqual    => { self.advance(); BinOp::LessEqual }
                TokenKind::GreaterThan  => { self.advance(); BinOp::GreaterThan }
                TokenKind::GreaterEqual => { self.advance(); BinOp::GreaterEqual }
                _ => break,
            };
            let right = self.parse_additive()?;
            left = AstNode::BinaryOp { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Plus  => { self.advance(); BinOp::Add }
                TokenKind::Minus => { self.advance(); BinOp::Sub }
                _ => break,
            };
            let right = self.parse_multiplicative()?;
            left = AstNode::BinaryOp { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Star    => { self.advance(); BinOp::Mul }
                TokenKind::Slash   => { self.advance(); BinOp::Div }
                TokenKind::Percent => { self.advance(); BinOp::Mod }
                _ => break,
            };
            let right = self.parse_unary()?;
            left = AstNode::BinaryOp { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<AstNode, String> {
        match self.peek_kind().clone() {
            TokenKind::Minus => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(AstNode::UnaryOp { op: UnOp::Negate, operand: Box::new(operand) })
            }
            TokenKind::Not => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(AstNode::UnaryOp { op: UnOp::Not, operand: Box::new(operand) })
            }
            TokenKind::Ampersand => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(AstNode::Reference(Box::new(operand)))
            }
            _ => self.parse_primary(),
        }
    }

    // Primary / atoms

    fn parse_primary(&mut self) -> Result<AstNode, String> {
        let location = self.current_location();

        match self.peek().kind.clone() {
            TokenKind::Integer(n) => {
                self.advance();
                Ok(AstNode::Number(n))
            }
            TokenKind::Keyword(Keyword::True) => {
                self.advance();
                Ok(AstNode::Boolean(true))
            }
            TokenKind::Keyword(Keyword::False) => {
                self.advance();
                Ok(AstNode::Boolean(false))
            }
            TokenKind::CharLit(c) => {
                self.advance();
                Ok(AstNode::Character(c))
            }
            TokenKind::StringLit(s) => {
                self.advance();
                Ok(AstNode::StringLit(s))
            }
            TokenKind::LBracket => {
                self.advance();
                let mut elems = Vec::new();
                while !self.check(&TokenKind::RBracket) && !self.is_at_end() {
                    elems.push(self.parse_expression()?);
                    if !self.eat(&TokenKind::Comma) { break; }
                }
                self.expect(&TokenKind::RBracket, "Expected ']' to close array literal")?;
                Ok(AstNode::ArrayLit(elems))
            }
            TokenKind::Identifier(name) => {
                self.advance();
                self.parse_postfix(AstNode::Identifier { name, location })
            }
            TokenKind::LParen => {
                self.advance();
                let expr = self.parse_expression()?;
                self.expect(&TokenKind::RParen, "Expected ')'")?;
                Ok(expr)
            }
            _ => Err(self.error("Expected an expression")),
        }
    }

    // Postfix chain

    /// Handles call, method-call, index, struct-init, enum-variant chains.
    fn parse_postfix(&mut self, mut left: AstNode) -> Result<AstNode, String> {
        loop {
            match self.peek_kind().clone() {
                // fn_name(args...)
                TokenKind::LParen => {
                    self.advance();
                    let args = self.parse_call_args()?;
                    self.expect(&TokenKind::RParen, "Expected ')' to close argument list")?;
                    match left {
                        AstNode::Identifier { name, .. } => left = AstNode::Call { name, args },
                        _ => return Err(self.error("Invalid call target")),
                    }
                }

                // .field  or  .method(args...)  — but NOT ..
                TokenKind::Dot => {
                    self.advance();
                    let field = self.expect_identifier("Expected field or method name after '.'")?;
                    if self.eat(&TokenKind::LParen) {
                        let args = self.parse_call_args()?;
                        self.expect(&TokenKind::RParen, "Expected ')'")?;
                        left = AstNode::MethodCall { object: Box::new(left), method: field, args };
                    } else {
                        left = AstNode::MemberAccess { object: Box::new(left), field };
                    }
                }

                // expr[index]
                TokenKind::LBracket => {
                    self.advance();
                    let index = self.parse_expression()?;
                    self.expect(&TokenKind::RBracket, "Expected ']'")?;
                    left = AstNode::Index { array: Box::new(left), index: Box::new(index) };
                }

                // StructName { field: val, ... }
                // Only when no_struct_init is false (i.e., not after if/while/for/match).
                TokenKind::LBrace if !self.no_struct_init => {
                    if let AstNode::Identifier { name, .. } = left {
                        self.advance();
                        let fields = self.parse_field_inits()?;
                        self.expect(&TokenKind::RBrace, "Expected '}' to close struct literal")?;
                        left = AstNode::StructInit { name, fields };
                    } else {
                        break; // not a struct name, stop chaining
                    }
                }

                // EnumName::Variant  or  EnumName::Variant(val)
                // DoubleColon is a first-class token now — no peek_ahead hack.
                TokenKind::DoubleColon => {
                    if let AstNode::Identifier { name: enum_name, .. } = left {
                        self.advance();
                        let variant = self.expect_identifier("Expected variant name after '::'")?;
                        let value = if self.eat(&TokenKind::LParen) {
                            if self.check(&TokenKind::RParen) {
                                self.advance();
                                None
                            } else {
                                let v = self.parse_expression()?;
                                self.expect(&TokenKind::RParen, "Expected ')'")?;
                                Some(Box::new(v))
                            }
                        } else {
                            None
                        };
                        left = AstNode::EnumValue { enum_name, variant, value };
                    } else {
                        break;
                    }
                }

                // `..` is a range separator — stop postfix parsing here so the
                // for-loop range `x..y` is handled at the statement level.
                TokenKind::DotDot => break,

                _ => break,
            }
        }
        Ok(left)
    }

    // Argument / field helpers

    fn parse_call_args(&mut self) -> Result<Vec<AstNode>, String> {
        let mut args = Vec::new();
        if self.check(&TokenKind::RParen) {
            return Ok(args); // empty arg list
        }
        loop {
            if self.eat(&TokenKind::Ampersand) {
                self.eat_keyword(&Keyword::Mut); // &mut — ignored semantically here, tracked in semantic pass
                let expr = self.parse_expression()?;
                args.push(AstNode::Reference(Box::new(expr)));
            } else {
                args.push(self.parse_expression()?);
            }
            if !self.eat(&TokenKind::Comma) { break; }
        }
        Ok(args)
    }

    fn parse_field_inits(&mut self) -> Result<Vec<(String, AstNode)>, String> {
        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let name = self.expect_identifier("Expected field name")?;
            self.expect(&TokenKind::Colon, "Expected ':' after field name")?;
            let value = self.parse_expression()?;
            fields.push((name, value));
            self.eat(&TokenKind::Comma);
        }
        Ok(fields)
    }
}
