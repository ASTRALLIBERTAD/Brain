#![allow(dead_code)]

// Generics infrastructure — not yet wired into the compiler.
// These types are populated incrementally as generics are implemented.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TypeParamId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DefId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub enum BuiltinTrait {
    Copy,  // int, bool, char — no heap ownership
    Add,   // + operator
    Eq,    // == != operators
    Ord,   // < > <= >= operators
    Print, // print() builtin
}

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

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Int,
    Bool,
    Char,
    Str,
    Void,
    Param(TypeParamId),
    Named(String, Vec<Type>),
    Ref(bool, Box<Type>),
    Array(Box<Type>, usize),
}

impl Type {
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

    pub fn is_copy(&self) -> bool {
        matches!(self, Type::Int | Type::Bool | Type::Char)
    }

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

#[derive(Clone, Debug, PartialEq)]
pub enum TypeArg {
    Explicit(Type),
    Inferred(Type),
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

#[derive(Clone, Debug)]
pub struct TypeArgs {
    pub args: Vec<TypeArg>,
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

    pub fn resolved(&self) -> Option<Vec<&Type>> {
        self.args.iter().map(|a| a.ty()).collect()
    }

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
