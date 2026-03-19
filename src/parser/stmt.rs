use super::Parser;
use crate::ast::{AstNode, BinOp, Location, MatchArm};
use crate::lexer::{Keyword, TokenKind};

impl<'a> Parser<'a> {
    // Statement dispatcher

    pub(crate) fn parse_statement(&mut self) -> Result<AstNode, String> {
        match self.peek().kind.clone() {
            TokenKind::Keyword(Keyword::Let) => self.parse_let_binding(false),
            TokenKind::Keyword(Keyword::If) => self.parse_if(),
            TokenKind::Keyword(Keyword::While) => self.parse_while(),
            TokenKind::Keyword(Keyword::For) => self.parse_for(),
            TokenKind::Keyword(Keyword::Match) => self.parse_match(),
            TokenKind::Keyword(Keyword::Return) => self.parse_return(),
            TokenKind::Keyword(Keyword::Break) => {
                self.advance();
                self.expect(&TokenKind::Semicolon, "Expected ';' after 'break'")?;
                Ok(AstNode::Break)
            }
            TokenKind::Keyword(Keyword::Continue) => {
                self.advance();
                self.expect(&TokenKind::Semicolon, "Expected ';' after 'continue'")?;
                Ok(AstNode::Continue)
            }
            TokenKind::LBrace => self.parse_block(),

            // Identifiers need lookahead to distinguish assignment from expression.
            TokenKind::Identifier(_) => self.parse_identifier_led_stmt(),

            _ => {
                let expr = self.parse_expression()?;
                self.expect(&TokenKind::Semicolon, "Expected ';' after expression")?;
                Ok(AstNode::ExpressionStatement(Box::new(expr)))
            }
        }
    }

    /// Identifier-led statements: decide among simple assignment, array
    /// assignment, member assignment, or a plain expression statement.
    fn parse_identifier_led_stmt(&mut self) -> Result<AstNode, String> {
        let location = self.current_location();

        match self.peek_ahead(1).kind.clone() {
            // name = expr;
            TokenKind::Assign => self.parse_simple_assignment(location),

            // name[idx] = expr;   or   name[idx];
            TokenKind::LBracket => self.parse_array_assignment_or_index(location),

            // name.field = expr;  — only if ahead[2] is ident and ahead[3] is =
            TokenKind::Dot if self.is_member_assign_ahead() => {
                self.parse_member_assignment(location)
            }

            // Anything else is an expression statement (method call, fn call, etc.)
            _ => {
                let expr = self.parse_expression()?;
                self.expect(&TokenKind::Semicolon, "Expected ';'")?;
                Ok(AstNode::ExpressionStatement(Box::new(expr)))
            }
        }
    }

    fn is_member_assign_ahead(&self) -> bool {
        matches!(self.peek_ahead(2).kind, TokenKind::Identifier(_))
            && matches!(self.peek_ahead(3).kind, TokenKind::Assign)
    }

    fn parse_simple_assignment(&mut self, location: Location) -> Result<AstNode, String> {
        let name = self.expect_identifier("Expected variable name")?;
        self.expect(&TokenKind::Assign, "Expected '='")?;
        let value = Box::new(self.parse_expression()?);
        self.expect(&TokenKind::Semicolon, "Expected ';' after assignment")?;
        Ok(AstNode::Assignment {
            name,
            value,
            location,
        })
    }

    fn parse_array_assignment_or_index(&mut self, location: Location) -> Result<AstNode, String> {
        let name = self.expect_identifier("Expected identifier")?;
        self.expect(&TokenKind::LBracket, "Expected '['")?;
        let index = self.parse_expression()?;
        self.expect(&TokenKind::RBracket, "Expected ']'")?;

        if self.eat(&TokenKind::Assign) {
            let value = Box::new(self.parse_expression()?);
            self.expect(&TokenKind::Semicolon, "Expected ';'")?;
            Ok(AstNode::ArrayAssignment {
                array: name,
                index: Box::new(index),
                value,
                location,
            })
        } else {
            // It was just an index expression used as a statement.
            self.expect(&TokenKind::Semicolon, "Expected ';'")?;
            Ok(AstNode::ExpressionStatement(Box::new(AstNode::Index {
                array: Box::new(AstNode::Identifier { name, location }),
                index: Box::new(index),
            })))
        }
    }

    fn parse_member_assignment(&mut self, location: Location) -> Result<AstNode, String> {
        let object = self.expect_identifier("Expected object name")?;
        self.expect(&TokenKind::Dot, "Expected '.'")?;
        let field = self.expect_identifier("Expected field name")?;
        self.expect(&TokenKind::Assign, "Expected '='")?;
        let value = Box::new(self.parse_expression()?);
        self.expect(&TokenKind::Semicolon, "Expected ';'")?;
        Ok(AstNode::MemberAssignment {
            object,
            field,
            value,
            location,
        })
    }

    // Block

    /// Blocks collect inner errors locally and synchronize, so a bad
    /// statement inside a function doesn't stop the rest of the block
    /// (or subsequent functions) from being parsed.
    pub(crate) fn parse_block(&mut self) -> Result<AstNode, String> {
        self.expect(&TokenKind::LBrace, "Expected '{'")?;
        let mut stmts = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            match self.parse_statement() {
                Ok(s) => stmts.push(s),
                Err(e) => {
                    self.errors.push(e);
                    self.synchronize(); // recover within the block
                }
            }
        }

        self.expect(&TokenKind::RBrace, "Expected '}' to close block")?;
        Ok(AstNode::Block(stmts))
    }

    // Control flow

    pub(crate) fn parse_if(&mut self) -> Result<AstNode, String> {
        self.expect_keyword(&Keyword::If, "Expected 'if'")?;

        // Disable struct-init syntax in the condition so `if Foo { }` is not
        // parsed as `if (Foo { })`.
        self.no_struct_init = true;
        let condition = Box::new(self.parse_expression()?);
        self.no_struct_init = false;

        let then_block = Box::new(self.parse_block()?);
        let else_block = if self.eat_keyword(&Keyword::Else) {
            Some(Box::new(if self.check_keyword(&Keyword::If) {
                self.parse_if()?
            } else {
                self.parse_block()?
            }))
        } else {
            None
        };

        Ok(AstNode::If {
            condition,
            then_block,
            else_block,
        })
    }

    pub(crate) fn parse_while(&mut self) -> Result<AstNode, String> {
        self.expect_keyword(&Keyword::While, "Expected 'while'")?;

        self.no_struct_init = true;
        let condition = Box::new(self.parse_expression()?);
        self.no_struct_init = false;

        let body = Box::new(self.parse_block()?);
        Ok(AstNode::While { condition, body })
    }

    pub(crate) fn parse_for(&mut self) -> Result<AstNode, String> {
        self.expect_keyword(&Keyword::For, "Expected 'for'")?;
        let variable = self.expect_identifier("Expected loop variable name")?;
        self.expect_keyword(&Keyword::In, "Expected 'in'")?;

        self.no_struct_init = true;
        let start = self.parse_expression()?;
        self.no_struct_init = false;

        // `start..end` range or a plain iterator expression
        let iterator = if self.eat(&TokenKind::DotDot) {
            let end = self.parse_expression()?;
            AstNode::BinaryOp {
                op: BinOp::DotDot,
                left: Box::new(start),
                right: Box::new(end),
            }
        } else {
            start
        };

        let body = Box::new(self.parse_block()?);
        Ok(AstNode::For {
            variable,
            iterator: Box::new(iterator),
            body,
        })
    }

    pub(crate) fn parse_match(&mut self) -> Result<AstNode, String> {
        self.expect_keyword(&Keyword::Match, "Expected 'match'")?;

        self.no_struct_init = true;
        let value = Box::new(self.parse_expression()?);
        self.no_struct_init = false;

        self.expect(&TokenKind::LBrace, "Expected '{' after match value")?;
        let mut arms = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let pattern = self.parse_pattern()?;
            self.expect(&TokenKind::FatArrow, "Expected '=>' after pattern")?;

            let body = if self.check(&TokenKind::LBrace) {
                self.parse_block()?
            } else if self.check_keyword(&Keyword::Return) {
                self.parse_return()?
            } else {
                let expr = self.parse_expression()?;
                AstNode::ExpressionStatement(Box::new(expr))
            };

            arms.push(MatchArm { pattern, body });
            self.eat(&TokenKind::Comma); // trailing comma optional
        }

        self.expect(&TokenKind::RBrace, "Expected '}' to close match")?;
        Ok(AstNode::Match { value, arms })
    }

    pub(crate) fn parse_return(&mut self) -> Result<AstNode, String> {
        self.expect_keyword(&Keyword::Return, "Expected 'return'")?;

        let value = if self.check(&TokenKind::Semicolon) {
            None
        } else {
            Some(Box::new(self.parse_expression()?))
        };

        self.expect(&TokenKind::Semicolon, "Expected ';' after return")?;
        Ok(AstNode::Return(value))
    }
}
