use super::generator::CodeGenerator;
use crate::ast::AstNode;

impl CodeGenerator {
    pub(super) fn type_to_llvm(&self, type_name: &str) -> String {
        match type_name {
            "int" => "i64".to_string(),
            "bool" => "i1".to_string(),
            "char" => "i8".to_string(),
            "string" => "i8*".to_string(),
            "array" => "i64*".to_string(),
            "Vec" => "i8*".to_string(),
            "void" => "void".to_string(),
            "enum" => "{ i32, i64 }*".to_string(),
            t if t.starts_with("Mutex<") => "i8*".to_string(),
            t if t.starts_with("MutexGuard<") => "i8*".to_string(),
            t if t.starts_with('*') => {
                let inner = self.type_to_llvm(&t[1..]);
                format!("{}*", inner)
            }
            t if self.struct_types.contains_key(t) => format!("%{}*", t),
            t if self.enum_types.contains_key(t) => "{ i32, i64 }*".to_string(),
            _ => "i64".to_string(),
        }
    }

    pub(super) fn llvm_to_type(&self, llvm: &str) -> String {
        match llvm {
            "i64" => "int".to_string(),
            "i1" => "bool".to_string(),
            "i8" => "char".to_string(),
            "i8*" => "string".to_string(),
            "void" => "void".to_string(),
            "{ i32, i64 }*" => "enum".to_string(),
            s if s.starts_with('%') && s.ends_with('*') => s[1..s.len() - 1].to_string(),
            _ => "int".to_string(),
        }
    }

    pub(super) fn infer_type(&self, node: &AstNode) -> String {
        match node {
            AstNode::Number(_) => "int".to_string(),
            AstNode::Boolean(_) => "bool".to_string(),
            AstNode::Character(_) => "char".to_string(),
            AstNode::StringLit(_) => "string".to_string(),
            AstNode::StructInit { name, .. } => name.clone(),
            AstNode::BinaryOp { left, op, .. } => match op {
                crate::ast::BinOp::Equal
                | crate::ast::BinOp::NotEqual
                | crate::ast::BinOp::LessThan
                | crate::ast::BinOp::LessEqual
                | crate::ast::BinOp::GreaterThan
                | crate::ast::BinOp::GreaterEqual
                | crate::ast::BinOp::And
                | crate::ast::BinOp::Or => "bool".to_string(),
                _ => self.infer_type(left),
            },
            AstNode::Identifier { name, .. } => self
                .current_function_vars
                .get(name)
                .map(|m| m.var_type.clone())
                .unwrap_or_else(|| "int".to_string()),
            AstNode::ArrayLit(_) => "array".to_string(),
            AstNode::EnumValue { enum_name, .. } => {
                if enum_name == "Mutex" {
                    "Mutex<int>".to_string()
                } else {
                    "enum".to_string()
                }
            }
            AstNode::Call { name, .. } => match name.as_str() {
                "read_file" | "int_to_string" | "read_input" => "string".to_string(),
                "write_file" => "int".to_string(),
                "vec_new" => "Vec".to_string(),
                "vec_get" | "vec_len" => "int".to_string(),
                _ => self
                    .function_signatures
                    .get(name.as_str())
                    .map(|t| self.llvm_to_type(t))
                    .unwrap_or_else(|| "int".to_string()),
            },
            AstNode::Reference(inner) => self.infer_type(inner),
            AstNode::MethodCall { object, method, .. } => {
                let obj_type = self.infer_type(object);
                match method.as_str() {
                    "len" | "char_at" | "get" => "int".to_string(),
                    "lock" => {
                        if obj_type.starts_with("Mutex<") {
                            let inner = &obj_type[6..obj_type.len() - 1];
                            format!("MutexGuard<{}>", inner)
                        } else {
                            obj_type
                        }
                    }
                    _ => obj_type,
                }
            }
            AstNode::MemberAccess { object, field } => {
                let obj_type = self.infer_type(object);
                self.struct_types
                    .get(&obj_type)
                    .and_then(|fields| {
                        fields
                            .iter()
                            .find(|(n, _)| n == field)
                            .map(|(_, ty)| ty.clone())
                    })
                    .unwrap_or_else(|| "int".to_string())
            }
            _ => "int".to_string(),
        }
    }

    pub(super) fn infer_struct_name(&self, node: &AstNode) -> String {
        match node {
            AstNode::Identifier { name, .. } => self
                .current_function_vars
                .get(name)
                .map(|m| m.var_type.clone())
                .unwrap_or_default(),
            AstNode::StructInit { name, .. } => name.clone(),
            _ => String::new(),
        }
    }

    /// Strips `&`, `&mut ` prefix from a type string.
    /// Returns `(is_ref, is_mut, inner_type)`.
    pub(crate) fn strip_ref_prefix(ty: &str) -> (bool, bool, &str) {
        if let Some(rest) = ty.strip_prefix("&mut ") {
            (true, true, rest)
        } else if let Some(rest) = ty.strip_prefix('&') {
            (true, false, rest)
        } else {
            (false, false, ty)
        }
    }

    pub(super) fn mangle_fn(name: &str) -> String {
        match name {
            "main" => "main".to_string(),
            _ => format!("brn_{}", name),
        }
    }
}
