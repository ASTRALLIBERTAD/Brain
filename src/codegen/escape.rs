use super::generator::CodeGenerator;
use crate::ast::{AstNode, Parameter};

pub(super) struct EscapeAnalysis {
    pub(super) escaping: std::collections::HashSet<String>,
}

impl EscapeAnalysis {
    pub(super) fn analyze(
        params: &[Parameter],
        body: &AstNode,
    ) -> std::collections::HashSet<String> {
        let mut ea = EscapeAnalysis {
            escaping: std::collections::HashSet::new(),
        };
        ea.visit_body(params, body);
        ea.escaping
    }

    fn visit_body(&mut self, params: &[Parameter], body: &AstNode) {
        for p in params {
            let (is_ref, _, _) = CodeGenerator::strip_ref_prefix(&p.param_type);
            if !is_ref && !p.is_reference {
                let inner = p.param_type.as_str();
                if matches!(inner, "string" | "Vec")
                    || (!matches!(inner, "int" | "bool" | "char") && !inner.is_empty())
                {
                    self.escaping.insert(p.name.clone());
                }
            }
        }
        self.visit(body);
    }

    fn visit(&mut self, node: &AstNode) {
        match node {
            AstNode::Return(Some(val)) => {
                self.mark_escaping(val);
                self.visit(val);
            }
            AstNode::Call { name, args, .. } => {
                let safe_builtins = matches!(
                    name.as_str(),
                    "print"
                        | "println"
                        | "print_int"
                        | "println_int"
                        | "print_bool"
                        | "println_bool"
                        | "print_char"
                        | "println_char"
                        | "write_file"
                        | "read_file"
                        | "read_input"
                        | "vec_len"
                        | "vec_get"
                        | "vec_push"
                        | "vec_set"
                        | "int_to_string"
                        | "len"
                );
                for arg in args {
                    match arg {
                        AstNode::Reference(_) => {}
                        _ if !safe_builtins => {
                            let t = Self::rough_type(arg);
                            if Self::is_heap_type(&t) {
                                self.mark_escaping(arg);
                            }
                        }
                        _ => {}
                    }
                    self.visit(arg);
                }
            }
            AstNode::LetBinding { value, .. } => self.visit(value),
            AstNode::Assignment { value, .. } => self.visit(value),
            AstNode::Block(stmts) | AstNode::Program(stmts) => {
                for s in stmts {
                    self.visit(s);
                }
            }
            AstNode::If {
                condition,
                then_block,
                else_block,
            } => {
                self.visit(condition);
                self.visit(then_block);
                if let Some(e) = else_block {
                    self.visit(e);
                }
            }
            AstNode::While { condition, body } => {
                self.visit(condition);
                self.visit(body);
            }
            AstNode::For { iterator, body, .. } => {
                self.visit(iterator);
                self.visit(body);
            }
            AstNode::BinaryOp { left, right, .. } => {
                self.visit(left);
                self.visit(right);
            }
            AstNode::UnaryOp { operand, .. } => self.visit(operand),
            AstNode::ExpressionStatement(e) => self.visit(e),
            AstNode::Match { value, arms } => {
                self.visit(value);
                for arm in arms {
                    self.visit(&arm.body);
                }
            }
            AstNode::ArrayLit(elems) => {
                for e in elems {
                    self.visit(e);
                }
            }
            AstNode::StructInit { fields, .. } => {
                for (_, v) in fields {
                    self.visit(v);
                }
            }
            AstNode::Index { array, index } => {
                self.visit(array);
                self.visit(index);
            }
            AstNode::Reference(e) => self.visit(e),
            AstNode::MemberAccess { object, .. } => self.visit(object),
            AstNode::MethodCall { object, args, .. } => {
                self.visit(object);
                for a in args {
                    self.visit(a);
                }
            }
            _ => {}
        }
    }

    fn mark_escaping(&mut self, node: &AstNode) {
        match node {
            AstNode::Identifier { name, .. } => {
                self.escaping.insert(name.clone());
            }
            AstNode::Reference(inner) => self.mark_escaping(inner),
            _ => {}
        }
    }

    fn rough_type(node: &AstNode) -> String {
        match node {
            AstNode::StringLit(_) => "string".to_string(),
            AstNode::Identifier { .. } => "unknown".to_string(),
            AstNode::BinaryOp { left, .. } => Self::rough_type(left),
            _ => String::new(),
        }
    }

    fn is_heap_type(t: &str) -> bool {
        matches!(t, "string" | "Vec" | "unknown")
    }
}
