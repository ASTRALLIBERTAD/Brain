use super::{CodeGenerator, VarMetadata};
use crate::ast::{AstNode, Parameter};
use crate::codegen::escape::EscapeAnalysis;

impl CodeGenerator {
    pub(super) fn gen_function(
        &mut self,
        name: &str,
        params: &[Parameter],
        body: &AstNode,
        return_type: &Option<String>,
        is_unsafe: bool,
    ) -> String {
        self.current_function_vars.clear();
        self.temp_counter = 0;
        self.label_counter = 0;
        self.is_unsafe_fn = is_unsafe;
        self.guard_vars.clear();

        // Escape analysis: which local bindings escape to heap or other functions
        let escaping = EscapeAnalysis::analyze(params, body);
        self.non_escaping.clear();
        if let AstNode::Block(stmts) = body {
            for stmt in stmts {
                if let AstNode::LetBinding { name, .. } = stmt
                    && !escaping.contains(name)
                {
                    self.non_escaping.insert(name.clone());
                }
            }
        }

        let ret_type = if name == "main" {
            "i32".to_string()
        } else if let Some(rt) = return_type {
            self.type_to_llvm(rt)
        } else {
            "void".to_string()
        };

        self.function_signatures
            .insert(name.to_string(), ret_type.clone());
        self.current_function_name = name.to_string();
        self.current_function_return_type = ret_type.clone();

        let param_list = self.build_param_list(params);
        let mangled = Self::mangle_fn(name);
        let fn_attrs = if name != "main" && self.pure_functions.contains(name) {
            " nounwind readonly willreturn"
        } else {
            " nounwind"
        };

        self.emit(&format!(
            "\ndefine {} @{}({}){} {{",
            ret_type, mangled, param_list, fn_attrs
        ));
        self.emit("entry:");

        self.bind_params(params);

        self.block_terminated = false;
        self.gen_node(body);

        if name == "main" && !self.block_terminated {
            self.emit("  ret i32 0");
        } else if ret_type == "void" && !self.block_terminated {
            self.emit("  ret void");
        } else if !self.block_terminated {
            self.emit("  unreachable");
        }

        self.emit("}");
        String::new()
    }

    fn build_param_list(&self, params: &[Parameter]) -> String {
        if params.is_empty() {
            return String::new();
        }
        params
            .iter()
            .map(|p| {
                let (type_is_ref, type_is_mut, inner_type) = Self::strip_ref_prefix(&p.param_type);
                let type_is_ref = type_is_ref || p.is_reference;
                let type_is_mut = type_is_mut || p.is_mutable;

                let param_type_str = if type_is_ref {
                    if inner_type.starts_with('[') {
                        if let Some(size_str) = inner_type.split(';').nth(1) {
                            let size = size_str
                                .trim()
                                .trim_end_matches(']')
                                .trim()
                                .parse::<usize>()
                                .unwrap_or(100);
                            format!("[{} x i64]*", size)
                        } else {
                            "i64*".to_string()
                        }
                    } else if inner_type.starts_with("Mutex<") {
                        "i8*".to_string()
                    } else {
                        let base = self.type_to_llvm(inner_type);
                        if base.ends_with('*') {
                            base
                        } else {
                            format!("{}*", base)
                        }
                    }
                } else {
                    self.type_to_llvm(&p.param_type)
                };

                let is_mutex_param = inner_type.starts_with("Mutex<");
                let is_simple_ptr = type_is_ref && !inner_type.starts_with('[');
                let is_owned_ptr =
                    !type_is_ref && Self::is_pointer_llvm_type(&p.param_type) && !type_is_mut;

                let attrs = if is_mutex_param {
                    ""
                } else if is_simple_ptr {
                    if !type_is_mut {
                        "noalias readonly"
                    } else {
                        "noalias"
                    }
                } else if is_owned_ptr {
                    "noalias readonly"
                } else {
                    ""
                };

                if attrs.is_empty() {
                    format!("{} %arg_{}", param_type_str, p.name)
                } else {
                    format!("{} {} %arg_{}", param_type_str, attrs, p.name)
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn bind_params(&mut self, params: &[Parameter]) {
        for param in params {
            let (type_is_ref, _, inner_type) = Self::strip_ref_prefix(&param.param_type);
            let type_is_ref = type_is_ref || param.is_reference;

            if type_is_ref {
                let array_size = if inner_type.starts_with('[') {
                    inner_type
                        .split(';')
                        .nth(1)
                        .and_then(|s| s.trim().trim_end_matches(']').trim().parse::<usize>().ok())
                } else {
                    None
                };

                self.current_function_vars.insert(
                    param.name.clone(),
                    VarMetadata {
                        llvm_name: format!("%arg_{}", param.name),
                        var_type: inner_type.to_string(),
                        is_heap: false,
                        array_size,
                        is_string_literal: false,
                    },
                );
            } else {
                let param_type_str = self.type_to_llvm(&param.param_type);
                let ptr = self.new_temp();
                self.emit(&format!("  {} = alloca {}", ptr, param_type_str));
                self.emit(&format!(
                    "  store {} %arg_{}, {}* {}",
                    param_type_str, param.name, param_type_str, ptr
                ));
                self.current_function_vars.insert(
                    param.name.clone(),
                    VarMetadata {
                        llvm_name: ptr,
                        var_type: param.param_type.clone(),
                        is_heap: false,
                        array_size: None,
                        is_string_literal: false,
                    },
                );
            }
        }
    }
}
