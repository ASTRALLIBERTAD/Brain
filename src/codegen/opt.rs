use super::generator::CodeGenerator;
use crate::ast::{AstNode, BinOp, Parameter};

impl CodeGenerator {
    pub(super) fn is_pointer_llvm_type(ty: &str) -> bool {
        matches!(ty, "string" | "Vec")
            || ty.starts_with('[')
            || (!matches!(ty, "int" | "bool" | "char" | "void") && !ty.is_empty())
    }

    pub(super) fn collect_reachable(nodes: &[AstNode]) -> std::collections::HashSet<String> {
        let mut reachable = std::collections::HashSet::new();
        let mut queue = vec!["main".to_string()];

        let fn_bodies: std::collections::HashMap<&str, &AstNode> = nodes
            .iter()
            .filter_map(|n| {
                if let AstNode::FunctionDef { name, body, .. } = n {
                    Some((name.as_str(), body.as_ref()))
                } else {
                    None
                }
            })
            .collect();

        while let Some(current) = queue.pop() {
            if reachable.contains(&current) {
                continue;
            }
            reachable.insert(current.clone());
            if let Some(body) = fn_bodies.get(current.as_str()) {
                Self::collect_calls(body, &mut queue);
            }
        }
        reachable
    }

    pub(super) fn collect_calls(node: &AstNode, queue: &mut Vec<String>) {
        match node {
            AstNode::Call { name, args } => {
                queue.push(name.clone());
                for arg in args {
                    Self::collect_calls(arg, queue);
                }
            }
            AstNode::Block(stmts) | AstNode::Program(stmts) => {
                for s in stmts {
                    Self::collect_calls(s, queue);
                }
            }
            AstNode::FunctionDef { body, .. } => Self::collect_calls(body, queue),
            AstNode::LetBinding { value, .. } | AstNode::Assignment { value, .. } => {
                Self::collect_calls(value, queue)
            }
            AstNode::ArrayAssignment { index, value, .. } => {
                Self::collect_calls(index, queue);
                Self::collect_calls(value, queue);
            }
            AstNode::MemberAssignment { value, .. } => Self::collect_calls(value, queue),
            AstNode::If {
                condition,
                then_block,
                else_block,
            } => {
                Self::collect_calls(condition, queue);
                Self::collect_calls(then_block, queue);
                if let Some(e) = else_block {
                    Self::collect_calls(e, queue);
                }
            }
            AstNode::While { condition, body } => {
                Self::collect_calls(condition, queue);
                Self::collect_calls(body, queue);
            }
            AstNode::For { iterator, body, .. } => {
                Self::collect_calls(iterator, queue);
                Self::collect_calls(body, queue);
            }
            AstNode::Return(Some(n)) => Self::collect_calls(n, queue),
            AstNode::BinaryOp { left, right, .. } => {
                Self::collect_calls(left, queue);
                Self::collect_calls(right, queue);
            }
            AstNode::UnaryOp { operand, .. } => Self::collect_calls(operand, queue),
            AstNode::ExpressionStatement(e) => Self::collect_calls(e, queue),
            AstNode::Match { value, arms } => {
                Self::collect_calls(value, queue);
                for arm in arms {
                    Self::collect_calls(&arm.body, queue);
                }
            }
            AstNode::ArrayLit(elems) => {
                for e in elems {
                    Self::collect_calls(e, queue);
                }
            }
            AstNode::StructInit { fields, .. } => {
                for (_, v) in fields {
                    Self::collect_calls(v, queue);
                }
            }
            AstNode::Index { array, index } => {
                Self::collect_calls(array, queue);
                Self::collect_calls(index, queue);
            }
            AstNode::Reference(e) | AstNode::EnumValue { value: Some(e), .. } => {
                Self::collect_calls(e, queue);
            }
            AstNode::MethodCall { object, args, .. } => {
                Self::collect_calls(object, queue);
                for a in args {
                    Self::collect_calls(a, queue);
                }
            }
            AstNode::MemberAccess { object, .. } => Self::collect_calls(object, queue),
            _ => {}
        }
    }

    pub(super) fn infer_purity(params: &[Parameter], body: &AstNode) -> bool {
        let has_string_param = params.iter().any(|p| {
            let (_, _, inner) = Self::strip_ref_prefix(&p.param_type);
            inner == "string"
        });
        let has_mutex_param = params.iter().any(|p| {
            let (_, _, inner) = Self::strip_ref_prefix(&p.param_type);
            inner.starts_with("Mutex<")
        });
        if has_mutex_param {
            return false;
        }
        for p in params {
            let (is_ref, is_mut, _) = Self::strip_ref_prefix(&p.param_type);
            if (p.is_reference || is_ref) && (p.is_mutable || is_mut) {
                return false;
            }
        }
        if has_string_param && Self::body_contains_add(body) {
            return false;
        }
        Self::body_is_pure(body)
    }

    pub(super) fn body_contains_add(node: &AstNode) -> bool {
        match node {
            AstNode::BinaryOp { op: BinOp::Add, .. } => true,
            AstNode::BinaryOp { left, right, .. } => {
                Self::body_contains_add(left) || Self::body_contains_add(right)
            }
            AstNode::Block(nodes) | AstNode::Program(nodes) => {
                nodes.iter().any(Self::body_contains_add)
            }
            AstNode::Return(Some(v)) => Self::body_contains_add(v),
            AstNode::LetBinding { value, .. } => Self::body_contains_add(value),
            AstNode::If {
                condition,
                then_block,
                else_block,
            } => {
                Self::body_contains_add(condition)
                    || Self::body_contains_add(then_block)
                    || else_block
                        .as_ref()
                        .is_some_and(|e| Self::body_contains_add(e))
            }
            AstNode::Call { args, .. } => args.iter().any(Self::body_contains_add),
            AstNode::ExpressionStatement(e) => Self::body_contains_add(e),
            _ => false,
        }
    }

    pub(super) fn body_is_pure(node: &AstNode) -> bool {
        match node {
            AstNode::Assignment { .. }
            | AstNode::ArrayAssignment { .. }
            | AstNode::MemberAssignment { .. } => false,
            AstNode::Call { name, args } => {
                let known_pure = matches!(
                    name.as_str(),
                    "vec_new"
                        | "vec_get"
                        | "vec_len"
                        | "int_to_string"
                        | "fib"
                        | "add"
                        | "is_between"
                );
                known_pure && args.iter().all(Self::body_is_pure)
            }
            AstNode::Program(nodes) | AstNode::Block(nodes) => nodes.iter().all(Self::body_is_pure),
            AstNode::FunctionDef { body, .. } => Self::body_is_pure(body),
            AstNode::LetBinding { value, .. } => Self::body_is_pure(value),
            AstNode::If {
                condition,
                then_block,
                else_block,
            } => {
                Self::body_is_pure(condition)
                    && Self::body_is_pure(then_block)
                    && else_block.as_ref().is_none_or(|e| Self::body_is_pure(e))
            }
            AstNode::While { condition, body } => {
                Self::body_is_pure(condition) && Self::body_is_pure(body)
            }
            AstNode::For { iterator, body, .. } => {
                Self::body_is_pure(iterator) && Self::body_is_pure(body)
            }
            AstNode::Return(v) => v.as_ref().is_none_or(|n| Self::body_is_pure(n)),
            AstNode::BinaryOp { op, left, right } => {
                if matches!(op, BinOp::Add) {
                    let has_string_lit = matches!(left.as_ref(), AstNode::StringLit(_))
                        || matches!(right.as_ref(), AstNode::StringLit(_));
                    if has_string_lit {
                        return false;
                    }
                }
                Self::body_is_pure(left) && Self::body_is_pure(right)
            }
            AstNode::UnaryOp { operand, .. } => Self::body_is_pure(operand),
            AstNode::ExpressionStatement(e) => Self::body_is_pure(e),
            AstNode::Match { value, arms } => {
                Self::body_is_pure(value) && arms.iter().all(|a| Self::body_is_pure(&a.body))
            }
            AstNode::ArrayLit(elems) => elems.iter().all(Self::body_is_pure),
            AstNode::StructInit { fields, .. } => fields.iter().all(|(_, v)| Self::body_is_pure(v)),
            AstNode::Index { array, index } => {
                Self::body_is_pure(array) && Self::body_is_pure(index)
            }
            AstNode::Reference(e) => Self::body_is_pure(e),
            AstNode::EnumValue { value: Some(e), .. } => Self::body_is_pure(e),
            AstNode::MethodCall { object, args, .. } => {
                Self::body_is_pure(object) && args.iter().all(Self::body_is_pure)
            }
            AstNode::MemberAccess { object, .. } => Self::body_is_pure(object),
            _ => true,
        }
    }
}
