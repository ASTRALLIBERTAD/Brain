// Type argument inference pass.
// Runs after semantic analysis, before codegen.
// Walks the AST and populates TypeArgs on every Call and StructInit node
// that refers to a generic function or struct.
//
// After this pass, gen_user_call can use type_args.mono_suffix() directly
// instead of doing its own string-based inference.

use crate::ast::AstNode;
use crate::generics::{Type, TypeArg, TypeArgs};
use std::collections::HashMap;

struct GenericFnInfo {
    type_params: Vec<String>,
    param_types: Vec<String>,
    return_type: Option<String>,
}

struct GenericStructInfo {
    type_params: Vec<String>,
    field_types: Vec<(String, String)>,
}

pub struct InferPass {
    generic_fns: HashMap<String, GenericFnInfo>,
    generic_structs: HashMap<String, GenericStructInfo>,
    // known non-generic function return types (for infer_expr_type)
    fn_return_types: HashMap<String, String>,
}

impl InferPass {
    pub fn new() -> Self {
        InferPass {
            generic_fns: HashMap::new(),
            generic_structs: HashMap::new(),
            fn_return_types: HashMap::new(),
        }
    }

    // ── Public API ────────────────────────────────────────────────────────────

    pub fn run(mut self, ast: AstNode) -> AstNode {
        self.collect(&ast);
        self.transform(ast, &mut HashMap::new())
    }

    // ── Collection pass: find all generic defs ────────────────────────────────

    fn collect(&mut self, ast: &AstNode) {
        if let AstNode::Program(nodes) = ast {
            for node in nodes {
                match node {
                    AstNode::FunctionDef {
                        name,
                        type_params,
                        params,
                        return_type,
                        ..
                    } => {
                        if !type_params.is_empty() {
                            self.generic_fns.insert(
                                name.clone(),
                                GenericFnInfo {
                                    type_params: type_params
                                        .iter()
                                        .map(|tp| tp.name.clone())
                                        .collect(),
                                    param_types: params
                                        .iter()
                                        .map(|p| {
                                            let (_, _, inner) = strip_ref(&p.param_type);
                                            inner.to_string()
                                        })
                                        .collect(),
                                    return_type: return_type.clone(),
                                },
                            );
                        } else {
                            let ret = return_type.clone().unwrap_or_else(|| "void".to_string());
                            self.fn_return_types.insert(name.clone(), ret);
                        }
                    }
                    AstNode::StructDef {
                        name,
                        type_params,
                        fields,
                        ..
                    } => {
                        if !type_params.is_empty() {
                            self.generic_structs.insert(
                                name.clone(),
                                GenericStructInfo {
                                    type_params: type_params
                                        .iter()
                                        .map(|tp| tp.name.clone())
                                        .collect(),
                                    field_types: fields
                                        .iter()
                                        .map(|f| (f.name.clone(), f.field_type.clone()))
                                        .collect(),
                                },
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // ── Transform pass: rewrite Call/StructInit with inferred TypeArgs ─────────

    fn transform(&self, node: AstNode, env: &mut HashMap<String, String>) -> AstNode {
        match node {
            AstNode::Program(nodes) => {
                AstNode::Program(nodes.into_iter().map(|n| self.transform(n, env)).collect())
            }

            AstNode::FunctionDef {
                name,
                type_params,
                params,
                return_type,
                body,
                is_exported,
                is_unsafe,
            } => {
                let mut inner_env: HashMap<String, String> = HashMap::new();
                for p in &params {
                    let (_, _, inner) = strip_ref(&p.param_type);
                    inner_env.insert(p.name.clone(), inner.to_string());
                }
                AstNode::FunctionDef {
                    name,
                    type_params,
                    params,
                    return_type,
                    body: Box::new(self.transform(*body, &mut inner_env)),
                    is_exported,
                    is_unsafe,
                }
            }

            AstNode::LetBinding {
                mutable,
                name,
                type_annotation,
                value,
                location,
                is_exported,
            } => {
                let transformed_value = self.transform(*value, env);
                let inferred = self.infer_expr_type(&transformed_value, env);
                env.insert(name.clone(), inferred);
                AstNode::LetBinding {
                    mutable,
                    name,
                    type_annotation,
                    value: Box::new(transformed_value),
                    location,
                    is_exported,
                }
            }

            AstNode::Call {
                name,
                type_args,
                args,
            } => {
                let transformed_args: Vec<AstNode> =
                    args.into_iter().map(|a| self.transform(a, env)).collect();

                let resolved_type_args = if !type_args.is_empty() {
                    // Already has explicit type args — keep them
                    type_args
                } else if let Some(info) = self.generic_fns.get(&name) {
                    // Infer from call-site argument types
                    self.infer_fn_type_args(info, &transformed_args, env)
                } else {
                    type_args
                };

                AstNode::Call {
                    name,
                    type_args: resolved_type_args,
                    args: transformed_args,
                }
            }

            AstNode::StructInit {
                name,
                type_args,
                fields,
            } => {
                let transformed_fields: Vec<(String, AstNode)> = fields
                    .into_iter()
                    .map(|(n, v)| (n, self.transform(v, env)))
                    .collect();

                let resolved_type_args = if !type_args.is_empty() {
                    type_args
                } else if let Some(info) = self.generic_structs.get(&name) {
                    self.infer_struct_type_args(info, &transformed_fields, env)
                } else {
                    type_args
                };

                AstNode::StructInit {
                    name,
                    type_args: resolved_type_args,
                    fields: transformed_fields,
                }
            }

            // All other nodes — recurse without env changes
            AstNode::Block(stmts) => {
                let mut block_env = env.clone();
                AstNode::Block(
                    stmts
                        .into_iter()
                        .map(|s| self.transform(s, &mut block_env))
                        .collect(),
                )
            }
            AstNode::If {
                condition,
                then_block,
                else_block,
            } => AstNode::If {
                condition: Box::new(self.transform(*condition, env)),
                then_block: Box::new(self.transform(*then_block, env)),
                else_block: else_block.map(|e| Box::new(self.transform(*e, env))),
            },
            AstNode::While { condition, body } => AstNode::While {
                condition: Box::new(self.transform(*condition, env)),
                body: Box::new(self.transform(*body, env)),
            },
            AstNode::For {
                variable,
                iterator,
                body,
            } => {
                let mut for_env = env.clone();
                for_env.insert(variable.clone(), "int".to_string());
                AstNode::For {
                    variable,
                    iterator: Box::new(self.transform(*iterator, env)),
                    body: Box::new(self.transform(*body, &mut for_env)),
                }
            }
            AstNode::Return(v) => AstNode::Return(v.map(|n| Box::new(self.transform(*n, env)))),
            AstNode::Assignment {
                name,
                value,
                location,
            } => AstNode::Assignment {
                name,
                value: Box::new(self.transform(*value, env)),
                location,
            },
            AstNode::ExpressionStatement(e) => {
                AstNode::ExpressionStatement(Box::new(self.transform(*e, env)))
            }
            AstNode::BinaryOp { op, left, right } => AstNode::BinaryOp {
                op,
                left: Box::new(self.transform(*left, env)),
                right: Box::new(self.transform(*right, env)),
            },
            AstNode::UnaryOp { op, operand } => AstNode::UnaryOp {
                op,
                operand: Box::new(self.transform(*operand, env)),
            },
            AstNode::MethodCall {
                object,
                method,
                args,
            } => AstNode::MethodCall {
                object: Box::new(self.transform(*object, env)),
                method,
                args: args.into_iter().map(|a| self.transform(a, env)).collect(),
            },
            AstNode::Match { value, arms } => AstNode::Match {
                value: Box::new(self.transform(*value, env)),
                arms: arms
                    .into_iter()
                    .map(|arm| crate::ast::MatchArm {
                        pattern: arm.pattern,
                        body: self.transform(arm.body, env),
                    })
                    .collect(),
            },
            AstNode::Reference(e) => AstNode::Reference(Box::new(self.transform(*e, env))),
            AstNode::ArrayLit(elems) => {
                AstNode::ArrayLit(elems.into_iter().map(|e| self.transform(e, env)).collect())
            }
            AstNode::Index { array, index } => AstNode::Index {
                array: Box::new(self.transform(*array, env)),
                index: Box::new(self.transform(*index, env)),
            },
            AstNode::ArrayAssignment {
                array,
                index,
                value,
                location,
            } => AstNode::ArrayAssignment {
                array,
                index: Box::new(self.transform(*index, env)),
                value: Box::new(self.transform(*value, env)),
                location,
            },
            AstNode::MemberAssignment {
                object,
                field,
                value,
                location,
            } => AstNode::MemberAssignment {
                object,
                field,
                value: Box::new(self.transform(*value, env)),
                location,
            },
            // Leaf nodes pass through unchanged
            other => other,
        }
    }

    // ── Type inference helpers ─────────────────────────────────────────────────

    fn infer_fn_type_args(
        &self,
        info: &GenericFnInfo,
        args: &[AstNode],
        env: &HashMap<String, String>,
    ) -> TypeArgs {
        let mut subst: HashMap<String, Type> = HashMap::new();

        for (i, arg) in args.iter().enumerate() {
            if let Some(formal) = info.param_types.get(i)
                && info.type_params.contains(formal)
            {
                let concrete = self.infer_expr_type(arg, env);
                subst
                    .entry(formal.clone())
                    .or_insert_with(|| Type::from_str(&concrete));
            }
        }

        let type_args: Vec<TypeArg> = info
            .type_params
            .iter()
            .map(|tp| match subst.get(tp) {
                Some(t) => TypeArg::Inferred(t.clone()),
                None => TypeArg::Unknown,
            })
            .collect();

        TypeArgs {
            args: type_args,
            def_id: None,
        }
    }

    fn infer_struct_type_args(
        &self,
        info: &GenericStructInfo,
        fields: &[(String, AstNode)],
        env: &HashMap<String, String>,
    ) -> TypeArgs {
        let mut subst: HashMap<String, Type> = HashMap::new();

        for (field_name, field_val) in fields {
            if let Some((_, formal_type)) = info.field_types.iter().find(|(n, _)| n == field_name)
                && info.type_params.contains(formal_type)
            {
                let concrete = self.infer_expr_type(field_val, env);
                subst
                    .entry(formal_type.clone())
                    .or_insert_with(|| Type::from_str(&concrete));
            }
        }

        let type_args: Vec<TypeArg> = info
            .type_params
            .iter()
            .map(|tp| match subst.get(tp) {
                Some(t) => TypeArg::Inferred(t.clone()),
                None => TypeArg::Unknown,
            })
            .collect();

        TypeArgs {
            args: type_args,
            def_id: None,
        }
    }

    fn infer_expr_type(&self, node: &AstNode, env: &HashMap<String, String>) -> String {
        match node {
            AstNode::Number(_) => "int".to_string(),
            AstNode::Boolean(_) => "bool".to_string(),
            AstNode::Character(_) => "char".to_string(),
            AstNode::StringLit(_) => "string".to_string(),
            AstNode::Identifier { name, .. } => {
                env.get(name).cloned().unwrap_or_else(|| "int".to_string())
            }
            AstNode::BinaryOp { op, left, .. } => match op {
                crate::ast::BinOp::Equal
                | crate::ast::BinOp::NotEqual
                | crate::ast::BinOp::LessThan
                | crate::ast::BinOp::LessEqual
                | crate::ast::BinOp::GreaterThan
                | crate::ast::BinOp::GreaterEqual
                | crate::ast::BinOp::And
                | crate::ast::BinOp::Or => "bool".to_string(),
                _ => self.infer_expr_type(left, env),
            },
            AstNode::Call { name, .. } => {
                // Non-generic functions: use known return type directly.
                if let Some(ret) = self.fn_return_types.get(name) {
                    return ret.clone();
                }
                // Generic functions: use the declared return type if it is a
                // concrete type (not itself a type parameter like T).
                if let Some(info) = self.generic_fns.get(name)
                    && let Some(rt) = &info.return_type
                    && !info.type_params.contains(rt)
                {
                    return rt.clone();
                }
                "int".to_string()
            }
            AstNode::StructInit { name, .. } => name.clone(),
            AstNode::Reference(inner) => self.infer_expr_type(inner, env),
            _ => "int".to_string(),
        }
    }
}

fn strip_ref(ty: &str) -> (bool, bool, &str) {
    if let Some(rest) = ty.strip_prefix("&mut ") {
        (true, true, rest)
    } else if let Some(rest) = ty.strip_prefix('&') {
        (true, false, rest)
    } else {
        (false, false, ty)
    }
}
