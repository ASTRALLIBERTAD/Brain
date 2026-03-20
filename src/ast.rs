// All AST node types live here so codegen, semantic, and module can
// import them without pulling in parser machinery.

use crate::generics::{TypeArgs, TypeParam};

// Location

#[derive(Debug, Clone, Copy, Default)]
pub struct Location {
    pub line: usize,
    pub column: usize,
}

impl Location {
    pub fn new(line: usize, column: usize) -> Self {
        Location { line, column }
    }
}

// Operators

#[derive(Debug, Clone, Copy, PartialEq)]
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Not,
    Negate,
}

// Supporting structures

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
    #[allow(dead_code)]
    pub value_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: AstNode,
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone)]
pub enum Pattern {
    Wildcard,
    Identifier(String),
    NumberPattern(i64),
    StringPattern(String),
    EnumPattern {
        enum_name: String,
        variant: String,
        binding: Option<String>,
    },
}

// AST Nodes

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
        #[allow(dead_code)]
        type_params: Vec<TypeParam>, // TODO: populate when generics are implemented
        params: Vec<Parameter>,
        return_type: Option<String>,
        body: Box<AstNode>,
        is_exported: bool,
        is_unsafe: bool,
    },

    StructDef {
        name: String,
        #[allow(dead_code)]
        type_params: Vec<TypeParam>, // TODO: populate when generics are implemented
        fields: Vec<Field>,
        is_exported: bool,
    },

    StructInit {
        name: String,
        #[allow(dead_code)]
        type_args: TypeArgs, // TODO: populate when generics are implemented
        fields: Vec<(String, AstNode)>,
    },

    EnumDef {
        name: String,
        variants: Vec<EnumVariant>,
        is_exported: bool,
    },

    EnumValue {
        enum_name: String,
        variant: String,
        value: Option<Box<AstNode>>,
    },

    ArrayLit(Vec<AstNode>),

    #[allow(dead_code)]
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
        #[allow(dead_code)]
        type_args: TypeArgs, // TODO: populate when generics are implemented
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
