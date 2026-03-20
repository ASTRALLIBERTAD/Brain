// All AST node types live here so codegen, semantic, and module can
// import them without pulling in parser machinery.  BinOp / UnOp used
// to sit in parser.rs even though they are grammar concepts, not
// parsing infrastructure — moving them here breaks the circular dep.

// ── Location ──────────────────────────────────────────────────────────────────

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

// ── Operators ─────────────────────────────────────────────────────────────────

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
    DotDot, // range
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Not,
    Negate,
}

// Generics infrastructure
//
// These types are NOT yet wired into AstNode — they exist so the design is
// settled and the rest of the compiler can be migrated incrementally.
// Nothing here breaks existing code.

/// Opaque identity for a type parameter — two `T`s in different functions
/// get different IDs so monomorphization never confuses them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TypeParamId(pub u32);

/// Every operator or builtin that has type-specific codegen becomes a trait.
/// Derived directly from the special-case branches already in the compiler:
///   Copy  -> is_copy_type() in semantic.rs
///   Add   -> BinOp::Add branch in gen_binary_op()
///   Eq    -> BinOp::Equal/NotEqual branch in gen_binary_op()
///   Ord   -> BinOp::LessThan/GreaterThan/.. in gen_binary_op()
///   Print -> "print" match arm in gen_call()
#[derive(Clone, Debug, PartialEq)]
pub enum BuiltinTrait {
    Copy,
    Add,
    Eq,
    Ord,
    Print,
}

/// A single constraint on a type parameter: `T: Add`, `T: Eq`, etc.
#[derive(Clone, Debug, PartialEq)]
pub struct TraitBound {
    pub trait_ref: BuiltinTrait,
}

impl TraitBound {
    pub fn copy() -> Self {
        TraitBound {
            trait_ref: BuiltinTrait::Copy,
        }
    }
    pub fn add() -> Self {
        TraitBound {
            trait_ref: BuiltinTrait::Add,
        }
    }
    pub fn eq() -> Self {
        TraitBound {
            trait_ref: BuiltinTrait::Eq,
        }
    }
    pub fn ord() -> Self {
        TraitBound {
            trait_ref: BuiltinTrait::Ord,
        }
    }
    pub fn print() -> Self {
        TraitBound {
            trait_ref: BuiltinTrait::Print,
        }
    }
}

/// A type parameter as it appears on a definition: the `T` in `fn foo<T: Add>`.
#[derive(Clone, Debug)]
pub struct TypeParam {
    pub name: String,
    pub id: TypeParamId,
    pub constraints: Vec<TraitBound>,
}

impl TypeParam {
    pub fn new(name: impl Into<String>, id: TypeParamId) -> Self {
        TypeParam {
            name: name.into(),
            id,
            constraints: vec![],
        }
    }

    pub fn with_bounds(mut self, bounds: Vec<TraitBound>) -> Self {
        self.constraints = bounds;
        self
    }

    pub fn satisfies(&self, bound: &BuiltinTrait) -> bool {
        self.constraints.iter().any(|c| &c.trait_ref == bound)
    }
}

/// Opaque identity for a generic definition (fn or struct).
/// Ties call-site TypeArgs back to the definition they came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DefId(pub u32);

/// The Brain type system — replaces bare `String` for type names.
/// Parameter/Field still use String today; migration is incremental.
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Int,
    Bool,
    Char,
    Str, // Brain's `string`
    Void,
    Param(TypeParamId),       // T, U — an unresolved type variable
    Named(String, Vec<Type>), // Foo, Vec<int>, Mutex<T>
    Ref(bool, Box<Type>),     // false=&T, true=&mut T
    Array(Box<Type>, usize),  // [int; 4]
}

impl Type {
    /// Produces a valid identifier fragment used in monomorphized names.
    ///   Type::Int                    -> "int"
    ///   Type::Named("Vec", [Int])    -> "Vec_int"
    ///   Type::Ref(false, Int)        -> "ref_int"
    ///   Type::Array(Int, 4)          -> "arr4_int"
    pub fn mangle(&self) -> String {
        match self {
            Type::Int => "int".into(),
            Type::Bool => "bool".into(),
            Type::Char => "char".into(),
            Type::Str => "str".into(),
            Type::Void => "void".into(),
            Type::Param(id) => format!("T{}", id.0),
            Type::Named(name, args) if args.is_empty() => name.clone(),
            Type::Named(name, args) => {
                let inner = args
                    .iter()
                    .map(|a| a.mangle())
                    .collect::<Vec<_>>()
                    .join("_");
                format!("{}_{}", name, inner)
            }
            Type::Ref(false, inner) => format!("ref_{}", inner.mangle()),
            Type::Ref(true, inner) => format!("refmut_{}", inner.mangle()),
            Type::Array(inner, size) => format!("arr{}_{}", size, inner.mangle()),
        }
    }

    /// True for types that Brain copies on move (no heap ownership).
    pub fn is_copy(&self) -> bool {
        matches!(self, Type::Int | Type::Bool | Type::Char)
    }

    /// Convert a legacy type string to Type.
    /// Used during the incremental migration from String -> Type.
    pub fn from_str(s: &str) -> Self {
        match s {
            "int" => Type::Int,
            "bool" => Type::Bool,
            "char" => Type::Char,
            "string" => Type::Str,
            "void" => Type::Void,
            _ => Type::Named(s.to_string(), vec![]),
        }
    }
}

/// How a type argument at a call site was resolved.
#[derive(Clone, Debug, PartialEq)]
pub enum TypeArg {
    /// User wrote it explicitly: `foo::<int>(x)`
    /// Produced by the parser.
    Explicit(Type),

    /// Compiler inferred it from argument types: `foo(42)` -> T=int
    /// Produced by the semantic pass.
    Inferred(Type),

    /// Not yet resolved — only valid between parsing and type inference.
    /// Must never reach codegen.
    Unknown,
}

impl TypeArg {
    pub fn ty(&self) -> Option<&Type> {
        match self {
            TypeArg::Explicit(t) | TypeArg::Inferred(t) => Some(t),
            TypeArg::Unknown => None,
        }
    }
}

/// The full type argument list at a call or struct instantiation site.
///   `push::<int>(v, 42)` -> TypeArgs { args: [Explicit(Int)], def_id: Some(...) }
///   `push(v, 42)`        -> TypeArgs { args: [Inferred(Int)], def_id: Some(...) }
///   non-generic call     -> TypeArgs::empty()
#[derive(Clone, Debug)]
pub struct TypeArgs {
    pub args: Vec<TypeArg>,
    /// Which generic definition these args apply to.
    /// None until the semantic pass resolves names.
    pub def_id: Option<DefId>,
}

impl TypeArgs {
    pub fn empty() -> Self {
        TypeArgs {
            args: vec![],
            def_id: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.args.is_empty()
    }

    /// Returns all resolved types, or None if any are still Unknown.
    /// Monomorphization calls this — None means inference failed.
    pub fn resolved(&self) -> Option<Vec<&Type>> {
        self.args.iter().map(|a| a.ty()).collect()
    }

    /// Suffix appended to a function name during monomorphization.
    ///   <int, string> -> Some("int_str")
    ///   <T> (unresolved) -> None
    pub fn mono_suffix(&self) -> Option<String> {
        let resolved = self.resolved()?;
        Some(
            resolved
                .iter()
                .map(|t| t.mangle())
                .collect::<Vec<_>>()
                .join("_"),
        )
    }
}

// Supporting structures

#[derive(Debug, Clone)]
pub struct Parameter {
    pub is_reference: bool,
    pub is_mutable: bool,
    pub name: String,
    pub param_type: String, // migrates to Type incrementally
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub field_type: String, // migrates to Type incrementally
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
        type_params: Vec<TypeParam>, // empty for non-generic functions
        params: Vec<Parameter>,
        return_type: Option<String>,
        body: Box<AstNode>,
        is_exported: bool,
        is_unsafe: bool,
    },

    StructDef {
        name: String,
        type_params: Vec<TypeParam>, // empty for non-generic structs
        fields: Vec<Field>,
        is_exported: bool,
    },

    StructInit {
        name: String,
        type_args: TypeArgs, // empty for non-generic structs
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
        type_args: TypeArgs, // empty for non-generic calls
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
