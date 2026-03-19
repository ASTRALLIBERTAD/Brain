// All top-level declaration parsing.  The redundant pattern noticed by Patoka (such a good boy)
// Unsafe/Fn/Let/Struct/Enum each being handled separately with near-identical
// consume + name + body logic — is collapsed here.  parse_export() does one
// match dispatch and delegates to the appropriate parser instead of
// duplicating the token sequence.

use super::Parser;
use crate::ast::{AstNode, EnumVariant, Field};
use crate::lexer::{Keyword, TokenKind};

impl<'a> Parser<'a> {
    // Import

    pub(crate) fn parse_import(&mut self) -> Result<AstNode, String> {
        self.expect_keyword(&Keyword::Import, "Expected 'import'")?;
        self.expect(&TokenKind::LBrace, "Expected '{' after 'import'")?;

        let mut names = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            names.push(self.expect_identifier("Expected symbol name in import list")?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }

        if names.is_empty() {
            return Err(self.error("Import list cannot be empty"));
        }

        self.expect(&TokenKind::RBrace, "Expected '}' after import list")?;
        self.expect_keyword(&Keyword::From, "Expected 'from' after import list")?;

        let path = match self.peek().kind.clone() {
            TokenKind::StringLit(s) => {
                self.advance();
                s
            }
            _ => return Err(self.error("Expected a file path string after 'from'")),
        };

        self.expect(&TokenKind::Semicolon, "Expected ';'")?;
        Ok(AstNode::Import { names, path })
    }

    // Export

    pub(crate) fn parse_export(&mut self) -> Result<AstNode, String> {
        self.expect_keyword(&Keyword::Export, "Expected 'export'")?;

        // Dispatch to the right parser — no duplicated token sequences.
        match self.peek().kind.clone() {
            TokenKind::Keyword(Keyword::Unsafe) => {
                self.advance();
                self.parse_function(true, true)
            }
            TokenKind::Keyword(Keyword::Fn) => self.parse_function(true, false),
            TokenKind::Keyword(Keyword::Let) => self.parse_let_binding(true),
            TokenKind::Keyword(Keyword::Struct) => {
                let mut node = self.parse_struct_def()?;
                if let AstNode::StructDef {
                    ref mut is_exported,
                    ..
                } = node
                {
                    *is_exported = true;
                }
                Ok(node)
            }
            TokenKind::Keyword(Keyword::Enum) => {
                let mut node = self.parse_enum_def()?;
                if let AstNode::EnumDef {
                    ref mut is_exported,
                    ..
                } = node
                {
                    *is_exported = true;
                }
                Ok(node)
            }
            _ => Err(self.error(
                "'export' can only be applied to 'fn', 'unsafe fn', 'let', 'struct', or 'enum'",
            )),
        }
    }

    // Functions

    pub(crate) fn parse_function(
        &mut self,
        is_exported: bool,
        is_unsafe: bool,
    ) -> Result<AstNode, String> {
        self.expect_keyword(&Keyword::Fn, "Expected 'fn'")?;
        let name = self.expect_identifier("Expected function name after 'fn'")?;

        self.expect(&TokenKind::LParen, "Expected '(' after function name")?;
        let params = self.parse_parameters()?;
        self.expect(&TokenKind::RParen, "Expected ')' to close parameter list")?;

        let return_type = if self.eat(&TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let body = Box::new(self.parse_block()?);
        Ok(AstNode::FunctionDef {
            name,
            params,
            return_type,
            body,
            is_exported,
            is_unsafe,
        })
    }

    // Struct

    pub(crate) fn parse_struct_def(&mut self) -> Result<AstNode, String> {
        self.expect_keyword(&Keyword::Struct, "Expected 'struct'")?;
        let name = self.expect_identifier("Expected struct name")?;
        self.expect(&TokenKind::LBrace, "Expected '{' after struct name")?;

        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let field_name = self.expect_identifier("Expected field name")?;
            self.expect(&TokenKind::Colon, "Expected ':' after field name")?;
            let field_type = self.parse_type()?;
            self.eat(&TokenKind::Comma); // trailing comma is optional
            fields.push(Field {
                name: field_name,
                field_type,
            });
        }

        self.expect(&TokenKind::RBrace, "Expected '}' to close struct body")?;
        Ok(AstNode::StructDef {
            name,
            fields,
            is_exported: false,
        })
    }

    // Enum

    pub(crate) fn parse_enum_def(&mut self) -> Result<AstNode, String> {
        self.expect_keyword(&Keyword::Enum, "Expected 'enum'")?;
        let name = self.expect_identifier("Expected enum name")?;
        self.expect(&TokenKind::LBrace, "Expected '{' after enum name")?;

        let mut variants = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let variant_name = self.expect_identifier("Expected variant name")?;
            let value_type = if self.eat(&TokenKind::LParen) {
                let ty = self.parse_type()?;
                self.expect(&TokenKind::RParen, "Expected ')' after variant type")?;
                Some(ty)
            } else {
                None
            };
            variants.push(EnumVariant {
                name: variant_name,
                value_type,
            });
            self.eat(&TokenKind::Comma);
        }

        self.expect(&TokenKind::RBrace, "Expected '}' to close enum body")?;
        Ok(AstNode::EnumDef {
            name,
            variants,
            is_exported: false,
        })
    }

    // let binding

    pub(crate) fn parse_let_binding(&mut self, is_exported: bool) -> Result<AstNode, String> {
        let location = self.current_location();
        self.expect_keyword(&Keyword::Let, "Expected 'let'")?;

        let mutable = self.eat_keyword(&Keyword::Mut);
        let name = self.expect_identifier("Expected variable name after 'let'")?;

        let type_annotation = if self.eat(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };

        self.expect(&TokenKind::Assign, "Expected '=' after variable name")?;
        let value = Box::new(self.parse_expression()?);
        self.expect(&TokenKind::Semicolon, "Expected ';' after let binding")?;

        Ok(AstNode::LetBinding {
            mutable,
            name,
            type_annotation,
            value,
            location,
            is_exported,
        })
    }
}
