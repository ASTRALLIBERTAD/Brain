// Statement-level codegen: bindings, assignments, blocks, control flow.
// The key design decision here is that LetBinding sets `current_binding_is_heap`
// BEFORE calling gen_node(value) so StructInit reads the same decision
// instead of recalculating it independently.

use super::{CodeGenerator, LoopLabels, VarMetadata};
use crate::ast::{AstNode, BinOp};

impl CodeGenerator {
    pub(super) fn gen_let_binding(&mut self, name: &str, value: &AstNode) -> String {
        let is_string_literal = matches!(value, AstNode::StringLit(_));
        let var_type = self.infer_type(value);
        let is_struct = self.struct_types.contains_key(&var_type);
        let is_mutex = var_type.starts_with("Mutex<") || var_type.starts_with("MutexGuard<");
        let stack_promote = self.non_escaping.contains(name) && !is_struct;

        // is_heap: this binding owns heap memory that block cleanup must free()
        let is_heap = !stack_promote
            && !is_mutex
            && ((var_type == "string" && !is_string_literal) || var_type == "Vec" || is_struct);

        // Thread the decision to StructInit BEFORE generating the value so
        // both agree on how the memory was allocated.
        self.current_binding = Some(name.to_string());
        self.current_binding_is_heap = is_heap;
        let value_reg = self.gen_node(value);
        self.current_binding = None;
        self.current_binding_is_heap = false;

        // Guard tracking for .lock() calls
        if let AstNode::MethodCall { method, .. } = value
            && method == "lock"
            && !self.is_unsafe_fn
        {
            self.guard_vars.insert(name.to_string());
        }

        // Arrays: store as-is with size metadata
        if let AstNode::ArrayLit(elements) = value {
            let size = elements.len();
            self.current_function_vars.insert(
                name.to_string(),
                VarMetadata {
                    llvm_name: value_reg.clone(),
                    var_type: format!("[{}; int]", size),
                    is_heap: false,
                    array_size: Some(size),
                    is_string_literal: false,
                },
            );
            return value_reg;
        }

        // Stack-allocated structs: already a %StructName*, no alloca wrapper
        if is_struct && !is_heap {
            self.current_function_vars.insert(
                name.to_string(),
                VarMetadata {
                    llvm_name: value_reg.clone(),
                    var_type,
                    is_heap: false,
                    array_size: None,
                    is_string_literal,
                },
            );
            return value_reg;
        }

        // Everything else: alloca + store.
        //
        // For Call nodes we MUST use the actual LLVM return type from
        // function_signatures rather than infer_type's guess.  infer_type
        // returns "unknown" for calls (→ i64 via type_to_llvm), but a
        // bool-returning function produces an i1 register — storing i1 into
        // an i64 slot is a verifier error.
        let llvm_type = match value {
            AstNode::Call {
                name,
                type_args,
                args: call_args,
                ..
            } => {
                // Reconstruct the exact monomorphic name gen_user_call produced
                // so we can look up its return type in function_signatures.
                // The old prefix-search was non-deterministic when multiple
                // specializations existed (e.g. identity_int and identity_bool
                // both start with "identity_" — HashMap iteration picks randomly).
                let resolved = if self.generic_fn_defs.contains_key(name.as_str()) {
                    if !type_args.is_empty() {
                        // Explicit type args: reconstruct suffix directly.
                        let suffix = type_args
                            .args
                            .iter()
                            .map(|ta| match ta {
                                crate::generics::TypeArg::Explicit(t)
                                | crate::generics::TypeArg::Inferred(t) => t.mangle(),
                                crate::generics::TypeArg::Unknown => "int".to_string(),
                            })
                            .collect::<Vec<_>>()
                            .join("_");
                        format!("{}_{}", name, suffix)
                    } else {
                        // Inferred: replicate gen_user_call's type inference to
                        // get the exact same mono name that was registered.
                        let (type_params, param_types) = if let Some(AstNode::FunctionDef {
                            type_params,
                            params,
                            ..
                        }) =
                            self.generic_fn_defs.get(name.as_str())
                        {
                            let tps: Vec<String> =
                                type_params.iter().map(|tp| tp.name.clone()).collect();
                            let pts: Vec<String> = params
                                .iter()
                                .map(|p| {
                                    let (_, _, inner) = Self::strip_ref_prefix(&p.param_type);
                                    inner.to_string()
                                })
                                .collect();
                            (tps, pts)
                        } else {
                            (vec![], vec![])
                        };
                        let mut subst: std::collections::HashMap<String, String> =
                            std::collections::HashMap::new();
                        for (i, arg_node) in call_args.iter().enumerate() {
                            if let Some(formal) = param_types.get(i)
                                && type_params.contains(formal)
                            {
                                let concrete = self.infer_type(arg_node);
                                subst.entry(formal.clone()).or_insert(concrete);
                            }
                        }
                        let concrete_args: Vec<String> = type_params
                            .iter()
                            .map(|tp| subst.get(tp).cloned().unwrap_or_else(|| "int".to_string()))
                            .collect();
                        format!("{}_{}", name, concrete_args.join("_"))
                    }
                } else {
                    name.clone()
                };
                self.function_signatures
                    .get(resolved.as_str())
                    .cloned()
                    .unwrap_or_else(|| self.type_to_llvm(&var_type))
            }
            _ => self.type_to_llvm(&var_type),
        };

        // Derive the source-level type for VarMetadata.  When var_type is
        // "unknown" (all call returns not in the semantic table), convert the
        // resolved LLVM type back so subsequent loads use the right width.
        let effective_var_type = if var_type == "unknown" {
            self.llvm_to_type(&llvm_type)
        } else {
            var_type.clone()
        };

        let ptr = self.new_temp();
        self.emit(&format!("  {} = alloca {}", ptr, llvm_type));
        self.emit(&format!(
            "  store {} {}, {}* {}",
            llvm_type, value_reg, llvm_type, ptr
        ));

        self.current_function_vars.insert(
            name.to_string(),
            VarMetadata {
                llvm_name: ptr.clone(),
                var_type: effective_var_type,
                is_heap,
                array_size: None,
                is_string_literal,
            },
        );

        ptr
    }

    pub(super) fn gen_assignment(&mut self, name: &str, value: &AstNode) -> String {
        let value_reg = self.gen_node(value);
        if let Some(meta) = self.current_function_vars.get(name).cloned() {
            let llvm_type = self.type_to_llvm(&meta.var_type);
            let llvm_name = meta.llvm_name.clone();
            self.emit(&format!(
                "  store {} {}, {}* {}",
                llvm_type, value_reg, llvm_type, llvm_name
            ));
        }
        value_reg
    }

    pub(super) fn gen_array_assignment(
        &mut self,
        array: &str,
        index: &AstNode,
        value: &AstNode,
    ) -> String {
        let index_val = self.gen_node(index);
        let value_reg = self.gen_node(value);
        if let Some(meta) = self.current_function_vars.get(array).cloned() {
            let array_size = meta.array_size.unwrap_or(100);
            let elem_ptr = self.new_temp();
            self.emit(&format!(
                "  {} = getelementptr [{} x i64], [{} x i64]* {}, i64 0, i64 {}",
                elem_ptr, array_size, array_size, meta.llvm_name, index_val
            ));
            self.emit(&format!("  store i64 {}, i64* {}", value_reg, elem_ptr));
        }
        value_reg
    }

    pub(super) fn gen_member_assignment(
        &mut self,
        object: &str,
        field: &str,
        value: &AstNode,
    ) -> String {
        let value_reg = self.gen_node(value);

        let is_guard = self.guard_vars.contains(object)
            || self
                .current_function_vars
                .get(object)
                .map(|m| m.var_type.starts_with("MutexGuard<"))
                .unwrap_or(false);

        if is_guard && field == "value" && !self.is_unsafe_fn {
            if let Some(meta) = self.current_function_vars.get(object).cloned() {
                let guard_ptr = if meta.llvm_name.starts_with("%arg_") {
                    meta.llvm_name.clone()
                } else {
                    let loaded = self.new_temp();
                    self.emit(&format!("  {} = load i8*, i8** {}", loaded, meta.llvm_name));
                    loaded
                };
                let val_gep = self.new_temp();
                self.emit(&format!(
                    "  {} = getelementptr i8, i8* {}, i64 40",
                    val_gep, guard_ptr
                ));
                let val_ptr = self.new_temp();
                self.emit(&format!("  {} = bitcast i8* {} to i64*", val_ptr, val_gep));
                self.emit(&format!(
                    "  store volatile i64 {}, i64* {}",
                    value_reg, val_ptr
                ));
            }
        } else if let Some(struct_fields) = self
            .current_function_vars
            .get(object)
            .map(|m| m.var_type.clone())
            .and_then(|t| self.struct_types.get(&t).cloned())
            && let Some(meta) = self.current_function_vars.get(object).cloned()
            && let Some(field_idx) = struct_fields.iter().position(|(n, _)| n == field)
        {
            let struct_name = meta.var_type.clone();
            let obj_ptr = if meta.llvm_name.starts_with("%arg_") {
                meta.llvm_name.clone()
            } else {
                let loaded = self.new_temp();
                self.emit(&format!(
                    "  {} = load %{}*, %{}** {}",
                    loaded, struct_name, struct_name, meta.llvm_name
                ));
                loaded
            };
            let field_type = struct_fields[field_idx].1.clone();
            let llvm_ft = self.type_to_llvm(&field_type);
            let gep = self.new_temp();
            self.emit(&format!(
                "  {} = getelementptr %{}, %{}* {}, i32 0, i32 {}",
                gep, struct_name, struct_name, obj_ptr, field_idx
            ));
            self.emit(&format!(
                "  store {} {}, {}* {}",
                llvm_ft, value_reg, llvm_ft, gep
            ));
        }

        value_reg
    }

    pub(super) fn gen_block(&mut self, statements: &[AstNode]) -> String {
        let mut last_reg = String::new();
        let keys_before: std::collections::HashSet<String> =
            self.current_function_vars.keys().cloned().collect();
        let guards_before = self.guard_vars.clone();

        for stmt in statements {
            last_reg = self.gen_node(stmt);
        }

        let guards_to_unlock: Vec<_> = self
            .current_function_vars
            .iter()
            .filter(|(name, meta)| {
                meta.var_type.starts_with("MutexGuard<")
                    && !keys_before.contains(name.as_str())
                    && !self.is_unsafe_fn
            })
            .map(|(_, meta)| meta.llvm_name.clone())
            .collect();

        let vars_to_free: Vec<_> = self
            .current_function_vars
            .iter()
            .filter(|(name, meta)| {
                meta.is_heap && !meta.is_string_literal && !keys_before.contains(name.as_str())
            })
            .map(|(_, meta)| (meta.llvm_name.clone(), meta.var_type.clone()))
            .collect();

        if !self.block_terminated {
            for guard_slot in guards_to_unlock {
                let mutex_ptr = self.new_temp();
                self.emit(&format!("  {} = load i8*, i8** {}", mutex_ptr, guard_slot));
                self.emit(&format!(
                    "  call void @LeaveCriticalSection(i8* {})",
                    mutex_ptr
                ));
            }

            for (llvm_name, var_type) in vars_to_free {
                if self.struct_types.contains_key(&var_type) {
                    let struct_ptr = self.new_temp();
                    self.emit(&format!(
                        "  {} = load %{}*, %{}** {}",
                        struct_ptr, var_type, var_type, llvm_name
                    ));
                    let i8_ptr = self.new_temp();
                    self.emit(&format!(
                        "  {} = bitcast %{}* {} to i8*",
                        i8_ptr, var_type, struct_ptr
                    ));
                    self.emit(&format!("  call void @free(i8* {})", i8_ptr));
                } else if var_type == "Vec" {
                    let ptr_reg = self.new_temp();
                    self.emit(&format!("  {} = load i8*, i8** {}", ptr_reg, llvm_name));
                    let dp_raw = self.new_temp();
                    self.emit(&format!(
                        "  {} = getelementptr i8, i8* {}, i64 16",
                        dp_raw, ptr_reg
                    ));
                    let dp = self.new_temp();
                    self.emit(&format!("  {} = bitcast i8* {} to i8**", dp, dp_raw));
                    let data = self.new_temp();
                    self.emit(&format!("  {} = load i8*, i8** {}", data, dp));
                    self.emit(&format!("  call void @free(i8* {})", data));
                    self.emit(&format!("  call void @free(i8* {})", ptr_reg));
                } else {
                    let ptr_reg = self.new_temp();
                    self.emit(&format!("  {} = load i8*, i8** {}", ptr_reg, llvm_name));
                    self.emit(&format!("  call void @free(i8* {})", ptr_reg));
                }
            }
        }

        self.current_function_vars
            .retain(|k, _| keys_before.contains(k));
        self.guard_vars = guards_before;
        last_reg
    }

    pub(super) fn gen_if(
        &mut self,
        condition: &AstNode,
        then_block: &AstNode,
        else_block: &Option<Box<AstNode>>,
    ) -> String {
        let cond_reg = self.gen_node(condition);
        let then_label = self.new_label("then");
        let else_label = self.new_label("else");
        let end_label = self.new_label("endif");

        if else_block.is_some() {
            self.emit(&format!(
                "  br i1 {}, label %{}, label %{}",
                cond_reg, then_label, else_label
            ));
        } else {
            self.emit(&format!(
                "  br i1 {}, label %{}, label %{}",
                cond_reg, then_label, end_label
            ));
        }

        self.emit(&format!("{}:", then_label));
        self.block_terminated = false;
        self.gen_node(then_block);
        let then_terminated = self.block_terminated;
        if !self.block_terminated {
            self.emit(&format!("  br label %{}", end_label));
        }

        let mut else_terminated = false;
        if let Some(else_blk) = else_block {
            self.emit(&format!("{}:", else_label));
            self.block_terminated = false;
            self.gen_node(else_blk);
            else_terminated = self.block_terminated;
            if !self.block_terminated {
                self.emit(&format!("  br label %{}", end_label));
            }
        }

        self.emit(&format!("{}:", end_label));
        if then_terminated && else_terminated {
            self.emit("  unreachable");
        }
        self.block_terminated = false;
        "0".to_string()
    }

    pub(super) fn gen_while(&mut self, condition: &AstNode, body: &AstNode) -> String {
        let cond_label = self.new_label("while_cond");
        let body_label = self.new_label("while_body");
        let end_label = self.new_label("while_end");

        self.loop_stack.push(LoopLabels {
            continue_label: cond_label.clone(),
            break_label: end_label.clone(),
        });

        self.emit(&format!("  br label %{}", cond_label));
        self.emit(&format!("{}:", cond_label));
        let cond_reg = self.gen_node(condition);
        self.emit(&format!(
            "  br i1 {}, label %{}, label %{}",
            cond_reg, body_label, end_label
        ));

        self.emit(&format!("{}:", body_label));
        self.block_terminated = false;
        self.gen_node(body);
        if !self.block_terminated {
            self.emit(&format!("  br label %{}", cond_label));
        }

        self.emit(&format!("{}:", end_label));
        self.loop_stack.pop();
        self.block_terminated = false;
        "0".to_string()
    }

    pub(super) fn gen_for(&mut self, variable: &str, iterator: &AstNode, body: &AstNode) -> String {
        let (start_val, end_val) = if let AstNode::BinaryOp {
            op: BinOp::DotDot,
            left,
            right,
        } = iterator
        {
            (self.gen_node(left), self.gen_node(right))
        } else {
            ("0".to_string(), self.gen_node(iterator))
        };

        let start_label = self.new_label("for_start");
        let body_label = self.new_label("for_body");
        let end_label = self.new_label("for_end");

        self.loop_stack.push(LoopLabels {
            continue_label: start_label.clone(),
            break_label: end_label.clone(),
        });

        let loop_var = self.new_temp();
        self.emit(&format!("  {} = alloca i64", loop_var));
        self.emit(&format!("  store i64 {}, i64* {}", start_val, loop_var));

        let end_ptr = self.new_temp();
        self.emit(&format!("  {} = alloca i64", end_ptr));
        self.emit(&format!("  store i64 {}, i64* {}", end_val, end_ptr));

        self.current_function_vars.insert(
            variable.to_string(),
            VarMetadata {
                llvm_name: loop_var.clone(),
                var_type: "int".to_string(),
                is_heap: false,
                array_size: None,
                is_string_literal: false,
            },
        );

        self.emit(&format!("  br label %{}", start_label));
        self.emit(&format!("{}:", start_label));

        let current = self.new_temp();
        let end_loaded = self.new_temp();
        self.emit(&format!("  {} = load i64, i64* {}", current, loop_var));
        self.emit(&format!("  {} = load i64, i64* {}", end_loaded, end_ptr));
        let cond = self.new_temp();
        self.emit(&format!(
            "  {} = icmp slt i64 {}, {}",
            cond, current, end_loaded
        ));
        self.emit(&format!(
            "  br i1 {}, label %{}, label %{}",
            cond, body_label, end_label
        ));

        self.emit(&format!("{}:", body_label));
        self.gen_node(body);

        let curr2 = self.new_temp();
        let next = self.new_temp();
        self.emit(&format!("  {} = load i64, i64* {}", curr2, loop_var));
        self.emit(&format!("  {} = add i64 {}, 1", next, curr2));
        self.emit(&format!("  store i64 {}, i64* {}", next, loop_var));
        self.emit(&format!("  br label %{}", start_label));

        self.emit(&format!("{}:", end_label));
        self.loop_stack.pop();
        "0".to_string()
    }
}
