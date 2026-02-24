use crate::lexer::{Token, TokenType};

#[derive(Debug, Clone, Copy)]
pub struct Location {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub enum AstNode {
    Program(Vec<AstNode>),

    Import {
        names: Vec<String>,
        path: String,
    },

    LetBinding {
        mutable: bool,
        name: String,
        type_annotation: Option<String>,
        value: Box<AstNode>,
        location: Location,
        is_exported: bool,
    },
    Assignment {
        name: String,
        value: Box<AstNode>,
        location: Location,
    },

    FunctionDef {
        name: String,
        params: Vec<Parameter>,
        return_type: Option<String>,
        body: Box<AstNode>,
        is_exported: bool,
        is_unsafe: bool,
    },

    StructDef {
        name: String,
        fields: Vec<Field>,
    },
    StructInit {
        name: String,
        fields: Vec<(String, AstNode)>,
    },

    EnumDef {
        name: String,
        variants: Vec<EnumVariant>,
    },
    EnumValue {
        enum_name: String,
        variant: String,
        value: Option<Box<AstNode>>,
    },

    ArrayLit(Vec<AstNode>),
    ArrayType {
        element_type: String,
        size: usize,
    },
    Index {
        array: Box<AstNode>,
        index: Box<AstNode>,
    },

    ArrayAssignment {
        array: String,
        index: Box<AstNode>,
        value: Box<AstNode>,
        location: Location,
    },

    MemberAssignment {
        object: String,
        field: String,
        value: Box<AstNode>,
        location: Location,
    },

    BinaryOp {
        op: BinOp,
        left: Box<AstNode>,
        right: Box<AstNode>,
    },
    UnaryOp {
        op: UnOp,
        operand: Box<AstNode>,
    },
    Number(i64),
    Boolean(bool),
    Character(char),
    StringLit(String),
    Identifier {
        name: String,
        location: Location,
    },
    Reference(Box<AstNode>),
    Call {
        name: String,
        args: Vec<AstNode>,
    },
    MethodCall {
        object: Box<AstNode>,
        method: String,
        args: Vec<AstNode>,
    },
    MemberAccess {
        object: Box<AstNode>,
        field: String,
    },

    If {
        condition: Box<AstNode>,
        then_block: Box<AstNode>,
        else_block: Option<Box<AstNode>>,
    },
    While {
        condition: Box<AstNode>,
        body: Box<AstNode>,
    },
    For {
        variable: String,
        iterator: Box<AstNode>,
        body: Box<AstNode>,
    },
    Match {
        value: Box<AstNode>,
        arms: Vec<MatchArm>,
    },
    Return(Option<Box<AstNode>>),
    Break,
    Continue,

    Block(Vec<AstNode>),
    ExpressionStatement(Box<AstNode>),
}

#[derive(Debug, Clone)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Equal,
    NotEqual,
    LessThan,
    LessEqual,
    GreaterThan,
    GreaterEqual,
    And,
    Or,
    DotDot,
}

#[derive(Debug, Clone)]
pub enum UnOp {
    Not,
    Negate,
}

#[derive(Debug, Clone)]
pub struct Parameter {
    pub is_reference: bool,
    pub is_mutable: bool,
    pub name: String,
    pub param_type: String,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub field_type: String,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub value_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: AstNode,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Identifier(String),
    EnumPattern {
        enum_name: String,
        variant: String,
        binding: Option<String>,
    },
    NumberPattern(i64),
    StringPattern(String),
    Wildcard,
}

pub struct Parser<'a> {
    tokens: Vec<Token>,
    current: usize,
    filename: &'a str,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: Vec<Token>, filename: &'a str) -> Self {
        Parser {
            tokens,
            current: 0,
            filename,
        }
    }

    pub fn parse(&mut self) -> Result<AstNode, String> {
        let mut nodes = Vec::new();

        while !self.is_at_end() {
            if self.check(&TokenType::Import) {
                nodes.push(self.parse_import()?);
            } else if self.check(&TokenType::Export) {
                nodes.push(self.parse_export()?);
            } else if self.check(&TokenType::Unsafe) {
                self.advance();
                nodes.push(self.parse_function(false, true)?);
            } else if self.check(&TokenType::Fn) {
                nodes.push(self.parse_function(false, false)?);
            } else if self.check(&TokenType::Struct) {
                nodes.push(self.parse_struct_def()?);
            } else if self.check(&TokenType::Enum) {
                nodes.push(self.parse_enum_def()?);
            } else {
                nodes.push(self.parse_statement()?);
            }
        }

        Ok(AstNode::Program(nodes))
    }

    fn parse_import(&mut self) -> Result<AstNode, String> {
        self.consume(&TokenType::Import, "Expected 'import'")?;
        self.consume(&TokenType::LBrace, "Expected '{' after 'import'")?;

        let mut names = Vec::new();

        if !self.check(&TokenType::RBrace) {
            loop {
                names.push(self.consume_identifier("Expected symbol name in import list")?);
                if !self.check(&TokenType::Comma) {
                    break;
                }
                self.advance();
                if self.check(&TokenType::RBrace) {
                    break;
                }
            }
        }

        if names.is_empty() {
            return Err(self.error("Import list cannot be empty"));
        }

        self.consume(&TokenType::RBrace, "Expected '}' after import list")?;
        self.consume(&TokenType::From, "Expected 'from' after import list")?;

        let path = match &self.peek().token_type {
            TokenType::StringLit(s) => {
                let s = s.clone();
                self.advance();
                s
            }
            _ => return Err(self.error("Expected file path string after 'from'")),
        };

        self.consume(&TokenType::Semicolon, "Expected ';'")?;

        Ok(AstNode::Import { names, path })
    }

    fn parse_export(&mut self) -> Result<AstNode, String> {
        self.consume(&TokenType::Export, "Expected 'export'")?;

        if self.check(&TokenType::Unsafe) {
            self.advance();
            self.parse_function(true, true)
        } else if self.check(&TokenType::Fn) {
            self.parse_function(true, false)
        } else if self.check(&TokenType::Let) {
            self.parse_let_binding_exported(true)
        } else {
            Err(self
                .error("'export' can only be applied to 'fn', 'unsafe fn', or 'let' declarations"))
        }
    }

    fn parse_function(&mut self, is_exported: bool, is_unsafe: bool) -> Result<AstNode, String> {
        self.consume(&TokenType::Fn, "Expected 'fn'")?;

        let name = self.consume_identifier("Expected function name")?;

        self.consume(&TokenType::LParen, "Expected '('")?;
        let params = self.parse_parameters()?;
        self.consume(&TokenType::RParen, "Expected ')'")?;

        let return_type = if self.check(&TokenType::Arrow) {
            self.advance();
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

    fn parse_parameters(&mut self) -> Result<Vec<Parameter>, String> {
        let mut params = Vec::new();

        if self.check(&TokenType::RParen) {
            return Ok(params);
        }

        loop {
            let is_reference = if self.check(&TokenType::Ampersand) {
                self.advance();
                true
            } else {
                false
            };

            let is_mutable = if self.check(&TokenType::Mut) {
                self.advance();
                true
            } else {
                false
            };

            let name = self.consume_identifier("Expected parameter name")?;
            self.consume(&TokenType::Colon, "Expected ':'")?;
            let param_type = self.parse_type()?;

            params.push(Parameter {
                is_reference,
                is_mutable,
                name,
                param_type,
            });

            if !self.check(&TokenType::Comma) {
                break;
            }
            self.advance();
        }

        Ok(params)
    }

    fn parse_struct_def(&mut self) -> Result<AstNode, String> {
        self.consume(&TokenType::Struct, "Expected 'struct'")?;
        let name = self.consume_identifier("Expected struct name")?;

        self.consume(&TokenType::LBrace, "Expected '{'")?;
        let mut fields = Vec::new();

        while !self.check(&TokenType::RBrace) && !self.is_at_end() {
            let field_name = self.consume_identifier("Expected field name")?;
            self.consume(&TokenType::Colon, "Expected ':'")?;
            let field_type = self.parse_type()?;
            self.consume(&TokenType::Semicolon, "Expected ';'")?;

            fields.push(Field {
                name: field_name,
                field_type,
            });
        }

        self.consume(&TokenType::RBrace, "Expected '}'")?;

        Ok(AstNode::StructDef { name, fields })
    }

    fn parse_enum_def(&mut self) -> Result<AstNode, String> {
        self.consume(&TokenType::Enum, "Expected 'enum'")?;
        let name = self.consume_identifier("Expected enum name")?;

        self.consume(&TokenType::LBrace, "Expected '{'")?;
        let mut variants = Vec::new();

        while !self.check(&TokenType::RBrace) && !self.is_at_end() {
            let variant_name = self.consume_identifier("Expected variant name")?;

            let value_type = if self.check(&TokenType::LParen) {
                self.advance();
                let ty = self.parse_type()?;
                self.consume(&TokenType::RParen, "Expected ')'")?;
                Some(ty)
            } else {
                None
            };

            variants.push(EnumVariant {
                name: variant_name,
                value_type,
            });

            if self.check(&TokenType::Comma) {
                self.advance();
            }
        }

        self.consume(&TokenType::RBrace, "Expected '}'")?;

        Ok(AstNode::EnumDef { name, variants })
    }

    fn parse_type(&mut self) -> Result<String, String> {
        match &self.peek().token_type {
            TokenType::IntType => {
                self.advance();
                Ok("int".to_string())
            }
            TokenType::BoolType => {
                self.advance();
                Ok("bool".to_string())
            }
            TokenType::StringType => {
                self.advance();
                Ok("string".to_string())
            }
            TokenType::CharType => {
                self.advance();
                Ok("char".to_string())
            }
            TokenType::Ampersand => {
                self.advance();
                // Handle both &T and &mut T
                if self.check(&TokenType::Mut) {
                    self.advance();
                    let inner = self.parse_type()?;
                    Ok(format!("&mut {}", inner))
                } else {
                    let inner = self.parse_type()?;
                    Ok(format!("&{}", inner))
                }
            }
            TokenType::LBracket => {
                self.advance();
                let elem_type = self.parse_type()?;
                self.consume(&TokenType::Semicolon, "Expected ';'")?;

                let size = if let TokenType::Number(n) = self.peek().token_type {
                    let num = n as usize;
                    self.advance();
                    num
                } else {
                    return Err(self.error("Expected array size"));
                };

                self.consume(&TokenType::RBracket, "Expected ']'")?;
                Ok(format!("[{}; {}]", elem_type, size))
            }
            TokenType::Identifier(name) => {
                let name = name.clone();
                self.advance();
                if name == "Vec" {
                    Ok("Vec".to_string())
                } else if name == "Mutex" {
                    self.consume(&TokenType::LessThan, "Expected '<' after 'Mutex'")?;
                    let inner = self.parse_type()?;
                    self.consume(
                        &TokenType::GreaterThan,
                        "Expected '>' after Mutex inner type",
                    )?;
                    Ok(format!("Mutex<{}>", inner))
                } else {
                    Ok(name)
                }
            }
            _ => Err(self.error("Expected type")),
        }
    }

    fn parse_array_assignment_or_expression(&mut self) -> Result<AstNode, String> {
        let location = Location {
            line: self.peek().line,
            column: self.peek().column,
        };

        let name = self.consume_identifier("Expected identifier")?;

        if self.check(&TokenType::LBracket) {
            self.advance();
            let index = self.parse_expression()?;
            self.consume(&TokenType::RBracket, "Expected ']'")?;

            if self.check(&TokenType::Assign) {
                self.advance();
                let value = Box::new(self.parse_expression()?);
                self.consume(&TokenType::Semicolon, "Expected ';'")?;

                return Ok(AstNode::ArrayAssignment {
                    array: name,
                    index: Box::new(index),
                    value,
                    location,
                });
            } else {
                self.consume(&TokenType::Semicolon, "Expected ';'")?;
                return Ok(AstNode::ExpressionStatement(Box::new(AstNode::Index {
                    array: Box::new(AstNode::Identifier { name, location }),
                    index: Box::new(index),
                })));
            }
        }

        Err(self.error("Expected array index"))
    }

    fn parse_statement(&mut self) -> Result<AstNode, String> {
        if self.check(&TokenType::Let) {
            self.parse_let_binding_exported(false)
        } else if self.check(&TokenType::If) {
            self.parse_if()
        } else if self.check(&TokenType::While) {
            self.parse_while()
        } else if self.check(&TokenType::For) {
            self.parse_for()
        } else if self.check(&TokenType::Match) {
            self.parse_match()
        } else if self.check(&TokenType::Return) {
            self.parse_return()
        } else if self.check(&TokenType::Break) {
            self.advance();
            self.consume(&TokenType::Semicolon, "Expected ';'")?;
            Ok(AstNode::Break)
        } else if self.check(&TokenType::Continue) {
            self.advance();
            self.consume(&TokenType::Semicolon, "Expected ';'")?;
            Ok(AstNode::Continue)
        } else if self.check(&TokenType::LBrace) {
            self.parse_block()
        } else if self.check_identifier() {
            let next_token = &self.peek_ahead(1).token_type;
            if *next_token == TokenType::Assign {
                self.parse_assignment()
            } else if *next_token == TokenType::LBracket {
                self.parse_array_assignment_or_expression()
            } else if *next_token == TokenType::Dot {
                // Check for member assignment: obj.field = val;
                let ahead2 = &self.peek_ahead(2).token_type;
                let ahead3 = &self.peek_ahead(3).token_type;
                if matches!(ahead2, TokenType::Identifier(_)) && *ahead3 == TokenType::Assign {
                    let location = Location {
                        line: self.peek().line,
                        column: self.peek().column,
                    };
                    let obj_name = self.consume_identifier("Expected object name")?;
                    self.advance(); // consume '.'
                    let field_name = self.consume_identifier("Expected field name")?;
                    self.advance(); // consume '='
                    let value = Box::new(self.parse_expression()?);
                    self.consume(&TokenType::Semicolon, "Expected ';'")?;
                    Ok(AstNode::MemberAssignment {
                        object: obj_name,
                        field: field_name,
                        value,
                        location,
                    })
                } else {
                    let expr = self.parse_expression()?;
                    self.consume(&TokenType::Semicolon, "Expected ';'")?;
                    Ok(AstNode::ExpressionStatement(Box::new(expr)))
                }
            } else {
                let expr = self.parse_expression()?;
                self.consume(&TokenType::Semicolon, "Expected ';'")?;
                Ok(AstNode::ExpressionStatement(Box::new(expr)))
            }
        } else {
            let expr = self.parse_expression()?;
            self.consume(&TokenType::Semicolon, "Expected ';'")?;
            Ok(AstNode::ExpressionStatement(Box::new(expr)))
        }
    }

    fn parse_let_binding_exported(&mut self, is_exported: bool) -> Result<AstNode, String> {
        let location = Location {
            line: self.peek().line,
            column: self.peek().column,
        };

        self.consume(&TokenType::Let, "Expected 'let'")?;

        let mutable = if self.check(&TokenType::Mut) {
            self.advance();
            true
        } else {
            false
        };

        let name = self.consume_identifier("Expected variable name")?;

        let type_annotation = if self.check(&TokenType::Colon) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };

        self.consume(&TokenType::Assign, "Expected '='")?;
        let value = Box::new(self.parse_expression()?);
        self.consume(&TokenType::Semicolon, "Expected ';'")?;

        Ok(AstNode::LetBinding {
            mutable,
            name,
            type_annotation,
            value,
            location,
            is_exported,
        })
    }

    fn parse_assignment(&mut self) -> Result<AstNode, String> {
        let location = Location {
            line: self.peek().line,
            column: self.peek().column,
        };

        let name = self.consume_identifier("Expected variable name")?;
        self.consume(&TokenType::Assign, "Expected '='")?;
        let value = Box::new(self.parse_expression()?);
        self.consume(&TokenType::Semicolon, "Expected ';'")?;

        Ok(AstNode::Assignment {
            name,
            value,
            location,
        })
    }

    fn parse_block(&mut self) -> Result<AstNode, String> {
        self.consume(&TokenType::LBrace, "Expected '{'")?;
        let mut statements = Vec::new();

        while !self.check(&TokenType::RBrace) && !self.is_at_end() {
            statements.push(self.parse_statement()?);
        }

        self.consume(&TokenType::RBrace, "Expected '}'")?;
        Ok(AstNode::Block(statements))
    }

    fn parse_if(&mut self) -> Result<AstNode, String> {
        self.consume(&TokenType::If, "Expected 'if'")?;
        let condition = Box::new(self.parse_expression()?);
        let then_block = Box::new(self.parse_block()?);

        let else_block = if self.check(&TokenType::Else) {
            self.advance();
            Some(Box::new(if self.check(&TokenType::If) {
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

    fn parse_while(&mut self) -> Result<AstNode, String> {
        self.consume(&TokenType::While, "Expected 'while'")?;
        let condition = Box::new(self.parse_expression()?);
        let body = Box::new(self.parse_block()?);

        Ok(AstNode::While { condition, body })
    }

    fn parse_for(&mut self) -> Result<AstNode, String> {
        self.consume(&TokenType::For, "Expected 'for'")?;
        let variable = self.consume_identifier("Expected loop variable")?;
        self.consume(&TokenType::In, "Expected 'in'")?;

        let start = self.parse_expression()?;

        let iterator = if self.check(&TokenType::DotDot) {
            self.advance();
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

    fn parse_match(&mut self) -> Result<AstNode, String> {
        self.consume(&TokenType::Match, "Expected 'match'")?;
        let value = Box::new(self.parse_expression()?);

        self.consume(&TokenType::LBrace, "Expected '{'")?;
        let mut arms = Vec::new();

        while !self.check(&TokenType::RBrace) && !self.is_at_end() {
            let pattern = self.parse_pattern()?;
            self.consume(&TokenType::FatArrow, "Expected '=>'")?;
            let body = self.parse_expression()?;

            arms.push(MatchArm { pattern, body });

            if self.check(&TokenType::Comma) {
                self.advance();
            }
        }

        self.consume(&TokenType::RBrace, "Expected '}'")?;

        Ok(AstNode::Match { value, arms })
    }

    fn parse_pattern(&mut self) -> Result<Pattern, String> {
        match self.peek().token_type.clone() {
            TokenType::Number(n) => {
                self.advance();
                Ok(Pattern::NumberPattern(n))
            }
            TokenType::Minus => {
                self.advance();
                if let TokenType::Number(n) = self.peek().token_type {
                    self.advance();
                    Ok(Pattern::NumberPattern(-n))
                } else {
                    Err(self.error("Expected number after '-' in pattern"))
                }
            }
            TokenType::StringLit(s) => {
                self.advance();
                Ok(Pattern::StringPattern(s))
            }
            TokenType::Identifier(_) => {
                let first = self.consume_identifier("Expected identifier")?;

                if first == "_" {
                    return Ok(Pattern::Wildcard);
                }

                if self.check(&TokenType::Colon)
                    && self.peek_ahead(1).token_type == TokenType::Colon
                {
                    self.advance();
                    self.advance();
                    let variant = self.consume_identifier("Expected variant name")?;

                    let binding = if self.check(&TokenType::LParen) {
                        self.advance();
                        let b = self.consume_identifier("Expected binding")?;
                        self.consume(&TokenType::RParen, "Expected ')'")?;
                        Some(b)
                    } else {
                        None
                    };

                    Ok(Pattern::EnumPattern {
                        enum_name: first,
                        variant,
                        binding,
                    })
                } else {
                    Ok(Pattern::Identifier(first))
                }
            }
            _ => Err(self.error("Expected pattern")),
        }
    }

    fn parse_return(&mut self) -> Result<AstNode, String> {
        self.consume(&TokenType::Return, "Expected 'return'")?;

        let value = if self.check(&TokenType::Semicolon) {
            None
        } else {
            Some(Box::new(self.parse_expression()?))
        };

        self.consume(&TokenType::Semicolon, "Expected ';'")?;
        Ok(AstNode::Return(value))
    }

    fn parse_expression(&mut self) -> Result<AstNode, String> {
        if self.check(&TokenType::Ampersand) {
            self.advance();
            let expr = self.parse_or()?;
            return Ok(AstNode::Reference(Box::new(expr)));
        }

        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_and()?;

        while self.check(&TokenType::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = AstNode::BinaryOp {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_and(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_comparison()?;

        while self.check(&TokenType::And) {
            self.advance();
            let right = self.parse_comparison()?;
            left = AstNode::BinaryOp {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_additive()?;

        while matches!(
            self.peek().token_type,
            TokenType::EqualEqual
                | TokenType::NotEqual
                | TokenType::LessThan
                | TokenType::LessEqual
                | TokenType::GreaterThan
                | TokenType::GreaterEqual
        ) {
            let op = match &self.peek().token_type {
                TokenType::EqualEqual => {
                    self.advance();
                    BinOp::Equal
                }
                TokenType::NotEqual => {
                    self.advance();
                    BinOp::NotEqual
                }
                TokenType::LessThan => {
                    self.advance();
                    BinOp::LessThan
                }
                TokenType::LessEqual => {
                    self.advance();
                    BinOp::LessEqual
                }
                TokenType::GreaterThan => {
                    self.advance();
                    BinOp::GreaterThan
                }
                TokenType::GreaterEqual => {
                    self.advance();
                    BinOp::GreaterEqual
                }
                _ => unreachable!(),
            };

            let right = self.parse_additive()?;
            left = AstNode::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_term()?;

        while self.check(&TokenType::Plus) || self.check(&TokenType::Minus) {
            let op = if self.check(&TokenType::Plus) {
                self.advance();
                BinOp::Add
            } else {
                self.advance();
                BinOp::Sub
            };

            let right = self.parse_term()?;
            left = AstNode::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_term(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_unary()?;

        while self.check(&TokenType::Star)
            || self.check(&TokenType::Slash)
            || self.check(&TokenType::Percent)
        {
            let op = if self.check(&TokenType::Star) {
                self.advance();
                BinOp::Mul
            } else if self.check(&TokenType::Slash) {
                self.advance();
                BinOp::Div
            } else {
                self.advance();
                BinOp::Mod
            };

            let right = self.parse_unary()?;
            left = AstNode::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<AstNode, String> {
        if self.check(&TokenType::Minus) {
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(AstNode::UnaryOp {
                op: UnOp::Negate,
                operand: Box::new(operand),
            });
        }

        if self.check(&TokenType::Not) {
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(AstNode::UnaryOp {
                op: UnOp::Not,
                operand: Box::new(operand),
            });
        }

        if self.check(&TokenType::Ampersand) {
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(AstNode::Reference(Box::new(operand)));
        }

        self.parse_factor()
    }

    fn parse_factor(&mut self) -> Result<AstNode, String> {
        match &self.peek().token_type {
            TokenType::Number(n) => {
                let n = *n;
                self.advance();
                Ok(AstNode::Number(n))
            }
            TokenType::True => {
                self.advance();
                Ok(AstNode::Boolean(true))
            }
            TokenType::False => {
                self.advance();
                Ok(AstNode::Boolean(false))
            }
            TokenType::CharLit(c) => {
                let c = *c;
                self.advance();
                Ok(AstNode::Character(c))
            }
            TokenType::StringLit(s) => {
                let s = s.clone();
                self.advance();
                Ok(AstNode::StringLit(s))
            }
            TokenType::LBracket => {
                self.advance();
                let mut elements = Vec::new();

                while !self.check(&TokenType::RBracket) && !self.is_at_end() {
                    elements.push(self.parse_expression()?);

                    if !self.check(&TokenType::Comma) {
                        break;
                    }
                    self.advance();
                }

                self.consume(&TokenType::RBracket, "Expected ']'")?;
                Ok(AstNode::ArrayLit(elements))
            }
            TokenType::Identifier(name) => {
                let name = name.clone();
                let location = Location {
                    line: self.peek().line,
                    column: self.peek().column,
                };
                self.advance();

                self.parse_postfix(AstNode::Identifier { name, location })
            }
            TokenType::LParen => {
                self.advance();
                let expr = self.parse_expression()?;
                self.consume(&TokenType::RParen, "Expected ')'")?;
                Ok(expr)
            }
            _ => Err(self.error("Expected expression")),
        }
    }

    fn parse_postfix(&mut self, mut left: AstNode) -> Result<AstNode, String> {
        loop {
            if self.check(&TokenType::LParen) {
                self.advance();
                let args = self.parse_arguments()?;
                self.consume(&TokenType::RParen, "Expected ')'")?;

                if let AstNode::Identifier { name, .. } = left {
                    left = AstNode::Call { name, args };
                } else {
                    return Err(self.error("Invalid function call"));
                }
            } else if self.check(&TokenType::Dot) {
                self.advance();
                let field = self.consume_identifier("Expected field or method name")?;

                if self.check(&TokenType::LParen) {
                    self.advance();
                    let args = self.parse_arguments()?;
                    self.consume(&TokenType::RParen, "Expected ')'")?;
                    left = AstNode::MethodCall {
                        object: Box::new(left),
                        method: field,
                        args,
                    };
                } else {
                    left = AstNode::MemberAccess {
                        object: Box::new(left),
                        field,
                    };
                }
            } else if self.check(&TokenType::LBracket) {
                self.advance();
                let index = self.parse_expression()?;
                self.consume(&TokenType::RBracket, "Expected ']'")?;
                left = AstNode::Index {
                    array: Box::new(left),
                    index: Box::new(index),
                };
            } else if self.check(&TokenType::LBrace) {
                if let AstNode::Identifier { name, .. } = left {
                    self.advance();
                    let fields = self.parse_field_inits()?;
                    self.consume(&TokenType::RBrace, "Expected '}'")?;
                    left = AstNode::StructInit { name, fields };
                } else {
                    break;
                }
            } else if self.check(&TokenType::Colon)
                && self.peek_ahead(1).token_type == TokenType::Colon
            {
                if let AstNode::Identifier {
                    name: enum_name, ..
                } = left
                {
                    self.advance();
                    self.advance();
                    let variant = self.consume_identifier("Expected variant name")?;

                    let value = if self.check(&TokenType::LParen) {
                        self.advance();
                        if self.check(&TokenType::RParen) {
                            self.advance();
                            None
                        } else {
                            let v = self.parse_expression()?;
                            self.consume(&TokenType::RParen, "Expected ')'")?;
                            Some(Box::new(v))
                        }
                    } else {
                        None
                    };

                    left = AstNode::EnumValue {
                        enum_name,
                        variant,
                        value,
                    };
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_arguments(&mut self) -> Result<Vec<AstNode>, String> {
        let mut args = Vec::new();

        if self.check(&TokenType::RParen) {
            return Ok(args);
        }

        loop {
            if self.check(&TokenType::Ampersand) {
                self.advance();
                let expr = self.parse_expression()?;
                args.push(AstNode::Reference(Box::new(expr)));
            } else {
                args.push(self.parse_expression()?);
            }

            if !self.check(&TokenType::Comma) {
                break;
            }
            self.advance();
        }

        Ok(args)
    }

    fn parse_field_inits(&mut self) -> Result<Vec<(String, AstNode)>, String> {
        let mut fields = Vec::new();

        if self.check(&TokenType::RBrace) {
            return Ok(fields);
        }

        loop {
            let name = self.consume_identifier("Expected field name")?;
            self.consume(&TokenType::Colon, "Expected ':'")?;
            let value = self.parse_expression()?;

            fields.push((name, value));

            if self.check(&TokenType::Comma) {
                self.advance();
            }

            if self.check(&TokenType::RBrace) {
                break;
            }
        }

        Ok(fields)
    }

    fn check(&self, token_type: &TokenType) -> bool {
        if self.is_at_end() {
            return false;
        }
        std::mem::discriminant(&self.peek().token_type) == std::mem::discriminant(token_type)
    }

    fn check_identifier(&self) -> bool {
        if self.is_at_end() {
            return false;
        }
        matches!(self.peek().token_type, TokenType::Identifier(_))
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    fn peek_ahead(&self, offset: usize) -> &Token {
        let pos = self.current + offset;
        if pos >= self.tokens.len() {
            &self.tokens[self.tokens.len() - 1]
        } else {
            &self.tokens[pos]
        }
    }

    fn advance(&mut self) -> &Token {
        if !self.is_at_end() {
            self.current += 1;
        }
        &self.tokens[self.current - 1]
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek().token_type, TokenType::Eof)
    }

    fn consume(&mut self, token_type: &TokenType, message: &str) -> Result<(), String> {
        if self.check(token_type) {
            self.advance();
            Ok(())
        } else {
            Err(self.error(message))
        }
    }

    fn consume_identifier(&mut self, message: &str) -> Result<String, String> {
        match &self.peek().token_type {
            TokenType::Identifier(name) => {
                let name = name.clone();
                self.advance();
                Ok(name)
            }
            _ => Err(self.error(message)),
        }
    }

    fn error(&self, message: &str) -> String {
        let token = self.peek();
        format!(
            "{}:{}:{}: {}",
            self.filename, token.line, token.column, message
        )
    }
}
