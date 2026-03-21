// Generics infrastructure.
//
// Several items here are populated by the parser and infer pass but not yet
// consumed by later passes (bounds checking, def-id resolution).  They are
// suppressed individually rather than with a blanket module-level allow so
// that genuinely unused code in other modules still gets caught.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TypeParamId(pub u32);

/// Ties a TypeArgs site back to the specific generic definition it
/// instantiates. Two generics `fn foo<T>` and `fn bar<T>` have the same
/// param name but different DefIds, so call sites remain unambiguous.
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

/// A type parameter declared on a generic function or struct.
///
/// `id` is assigned by the parser and is globally unique across all generic
/// definitions in a compilation unit — it distinguishes `T` in `fn foo<T>`
/// from `T` in `fn bar<T>`.
///
/// `constraints` holds the trait bounds parsed from `<T: Add + Eq>` syntax
/// and will be enforced by the semantic/bounds-checking pass.
#[derive(Clone, Debug)]
pub struct TypeParam {
    pub name: String,
    #[allow(dead_code)] // assigned by parser; used by future bounds-checking pass
    pub id: TypeParamId,
    #[allow(dead_code)] // populated from <T: Add + Eq> syntax; enforced by semantic pass
    pub constraints: Vec<TraitBound>,
}

impl TypeParam {
    #[allow(dead_code)] // called by parser via parse_type_params
    pub fn new(name: impl Into<String>, id: TypeParamId) -> Self {
        TypeParam {
            name: name.into(),
            id,
            constraints: vec![],
        }
    }

    #[allow(dead_code)] // used by parse_trait_bounds to attach constraints
    pub fn with_bounds(mut self, bounds: Vec<TraitBound>) -> Self {
        self.constraints = bounds;
        self
    }

    /// Returns true if this type param satisfies the given trait bound.
    /// Used by the semantic pass to enforce `<T: Add>` constraints.
    #[allow(dead_code)] // called once bounds-checking is wired in
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
    #[allow(dead_code)] // used by from_str and return-type inference
    Void,
    /// An unresolved type parameter, identified by its unique id.
    /// Present during inference before substitution replaces it with a
    /// concrete type.  Mangled as `"T0"`, `"T1"`, etc. so partially-resolved
    /// names are still unique and debuggable.
    #[allow(dead_code)]
    Param(TypeParamId),
    /// A named type, e.g. a user struct or a future generic instantiation.
    Named(String, Vec<Type>),
    /// A reference type: `&T` (false) or `&mut T` (true).
    /// Produced when a type param is instantiated with a reference type.
    #[allow(dead_code)]
    Ref(bool, Box<Type>),
    /// A fixed-size array type: `[T; N]`.
    /// Produced when a type param is instantiated with an array type.
    #[allow(dead_code)]
    Array(Box<Type>, usize),
}

impl Type {
    /// Produce the suffix used to mangle a monomorphized name,
    /// e.g. `Type::Int` → `"int"`, `Type::Named("Vec", [Int])` → `"Vec_int"`.
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

/// Type arguments at a call or struct-init site, e.g. `foo::<int, bool>(...)`.
///
/// `def_id` identifies which generic definition these args belong to.
/// It is `None` until the resolver/type-checker assigns it; codegen does
/// not require it (it uses `mono_suffix()` directly), but future passes
/// that need to look up the original `TypeParam` list by identity will.
#[derive(Clone, Debug)]
pub struct TypeArgs {
    pub args: Vec<TypeArg>,
    #[allow(dead_code)] // assigned by resolver; identifies which generic def this instantiates
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

    /// Resolved concrete types for all args, or `None` if any is `Unknown`.
    #[allow(dead_code)] // used by mono_suffix; also available for future passes
    pub fn resolved(&self) -> Option<Vec<&Type>> {
        self.args.iter().map(|a| a.ty()).collect()
    }

    /// Returns the mangled suffix for a monomorphized name, e.g. `"int_bool"`.
    /// Returns `None` if any arg is `Unknown`.
    pub fn mono_suffix(&self) -> Option<String> {
        Some(
            self.resolved()?
                .iter()
                .map(|t| t.mangle())
                .collect::<Vec<_>>()
                .join("_"),
        )
    }
}

