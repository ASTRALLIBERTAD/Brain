// Expression codegen — gen_node() lives here as the main dispatch, plus
// all expression-level arms (literals, binary ops, calls, method calls, match).

use super::{CodeGenerator, VarMetadata};
use crate::ast::{AstNode, BinOp, Pattern, UnOp};

impl CodeGenerator {
    /// Main dispatch — routes each AST node to the appropriate generator.
    pub(in super::super) fn gen_node(&mut self, node: &AstNode) -> String {
        match node {
            AstNode::Import { .. } | AstNode::StructDef { .. } => "0".to_string(),

            AstNode::EnumDef { name, variants, .. } => {
                let variant_names = variants.iter().map(|v| v.name.clone()).collect();
                self.enum_types.insert(name.clone(), variant_names);
                "0".to_string()
            }

            AstNode::FunctionDef {
                name,
                params,
                body,
                return_type,
                is_unsafe,
                ..
            } => self.gen_function(name, params, body, return_type, *is_unsafe),

            AstNode::LetBinding { name, value, .. } => self.gen_let_binding(name, value),

            AstNode::Assignment { name, value, .. } => self.gen_assignment(name, value),

            AstNode::ArrayAssignment {
                array,
                index,
                value,
                ..
            } => self.gen_array_assignment(array, index, value),

            AstNode::MemberAssignment {
                object,
                field,
                value,
                ..
            } => self.gen_member_assignment(object, field, value),

            AstNode::Block(stmts) => self.gen_block(stmts),

            AstNode::ExpressionStatement(expr) => self.gen_node(expr),

            AstNode::If {
                condition,
                then_block,
                else_block,
            } => self.gen_if(condition, then_block, else_block),

            AstNode::While { condition, body } => self.gen_while(condition, body),

            AstNode::For {
                variable,
                iterator,
                body,
            } => self.gen_for(variable, iterator, body),

            AstNode::Match { value, arms } => self.gen_match(value, arms),

            AstNode::Return(value) => {
                if let Some(val) = value {
                    let reg = self.gen_node(val);
                    let ret_type = self.current_function_return_type.clone();
                    self.emit(&format!("  ret {} {}", ret_type, reg));
                } else if self.current_function_return_type == "void" {
                    self.emit("  ret void");
                } else {
                    let ret_type = self.current_function_return_type.clone();
                    self.emit(&format!("  ret {} 0", ret_type));
                }
                self.block_terminated = true;
                "0".to_string()
            }

            AstNode::Break => {
                if let Some(labels) = self.loop_stack.last() {
                    let label = labels.break_label.clone();
                    self.emit(&format!("  br label %{}", label));
                    self.block_terminated = true;
                }
                "0".to_string()
            }

            AstNode::Continue => {
                if let Some(labels) = self.loop_stack.last() {
                    let label = labels.continue_label.clone();
                    self.emit(&format!("  br label %{}", label));
                    self.block_terminated = true;
                }
                "0".to_string()
            }

            AstNode::Number(n) => n.to_string(),
            AstNode::Boolean(b) => if *b { "1" } else { "0" }.to_string(),
            AstNode::Character(c) => (*c as i64).to_string(),

            AstNode::StringLit(s) => {
                let id = self.new_string_literal(s);
                let ptr = self.new_temp();
                let len = s.len() + 1;
                self.emit(&format!(
                    "  {} = getelementptr inbounds [{} x i8], [{} x i8]* @{}, i64 0, i64 0",
                    ptr, len, len, id
                ));
                ptr
            }

            AstNode::ArrayLit(elements) => {
                if elements.is_empty() {
                    return "null".to_string();
                }
                let size = elements.len();
                let ptr = self.new_temp();
                self.emit(&format!("  {} = alloca [{} x i64]", ptr, size));
                for (i, elem) in elements.iter().enumerate() {
                    let val = self.gen_node(elem);
                    let elem_ptr = self.new_temp();
                    self.emit(&format!(
                        "  {} = getelementptr [{} x i64], [{} x i64]* {}, i64 0, i64 {}",
                        elem_ptr, size, size, ptr, i
                    ));
                    self.emit(&format!("  store i64 {}, i64* {}", val, elem_ptr));
                }
                ptr
            }

            AstNode::StructInit { name, fields, .. } => self.gen_struct_init(name, fields),

            AstNode::MemberAccess { object, field } => self.gen_member_access(object, field),

            AstNode::Index { array, index } => {
                let index_val = self.gen_node(index);
                let (array_ptr, array_size) = match array.as_ref() {
                    AstNode::Identifier { name, .. } => {
                        if let Some(meta) = self.current_function_vars.get(name) {
                            (meta.llvm_name.clone(), meta.array_size.unwrap_or(100))
                        } else {
                            eprintln!("CODEGEN ERROR: array '{}' not found", name);
                            return "0".to_string();
                        }
                    }
                    _ => (self.gen_node(array), 100),
                };
                let elem_ptr = self.new_temp();
                let result = self.new_temp();
                self.emit(&format!(
                    "  {} = getelementptr [{} x i64], [{} x i64]* {}, i64 0, i64 {}",
                    elem_ptr, array_size, array_size, array_ptr, index_val
                ));
                self.emit(&format!("  {} = load i64, i64* {}", result, elem_ptr));
                result
            }

            AstNode::Identifier { name, .. } => {
                if let Some(meta) = self.current_function_vars.get(name).cloned() {
                    if meta.llvm_name.starts_with("%arg_")
                        || (self.struct_types.contains_key(&meta.var_type) && !meta.is_heap)
                    {
                        return meta.llvm_name;
                    }
                    let result = self.new_temp();
                    let llvm_type = self.type_to_llvm(&meta.var_type);
                    let llvm_name = meta.llvm_name.clone();
                    self.emit(&format!(
                        "  {} = load {}, {}* {}",
                        result, llvm_type, llvm_type, llvm_name
                    ));
                    result
                } else {
                    eprintln!("CODEGEN ERROR: variable '{}' not found", name);
                    "0".to_string()
                }
            }

            AstNode::Reference(expr) => self.gen_reference(expr),

            AstNode::EnumValue {
                enum_name,
                variant,
                value,
            } => self.gen_enum_value(enum_name, variant, value),

            AstNode::BinaryOp { op, left, right } => self.gen_binary_op(op, left, right),

            AstNode::UnaryOp { op, operand } => {
                let reg = self.gen_node(operand);
                let result = self.new_temp();
                match op {
                    UnOp::Not => self.emit(&format!("  {} = xor i1 {}, true", result, reg)),
                    UnOp::Negate => self.emit(&format!("  {} = sub i64 0, {}", result, reg)),
                }
                result
            }

            AstNode::Call { name, args, .. } => self.gen_call(name, args),

            AstNode::MethodCall {
                object,
                method,
                args,
            } => self.gen_method_call(object, method, args),

            _ => "0".to_string(),
        }
    }

    fn gen_struct_init(&mut self, name: &str, fields: &[(String, AstNode)]) -> String {
        let struct_fields = self.struct_types.get(name).cloned().unwrap_or_default();
        let num_fields = struct_fields.len();

        // Read the decision LetBinding made — they now always agree.
        // If current_binding_is_heap is true (all struct bindings are), use malloc.
        // Structs never stack-promote: see codegen/gen/mod.rs for explanation.
        let size = (num_fields as i64) * 8;
        let raw_ptr = self.new_temp();
        let struct_ptr = self.new_temp();
        self.emit(&format!("  {} = call i8* @malloc(i64 {})", raw_ptr, size));
        self.emit(&format!(
            "  {} = bitcast i8* {} to %{}*",
            struct_ptr, raw_ptr, name
        ));

        for (field_name, field_value) in fields {
            let val_reg = self.gen_node(field_value);
            let field_idx = struct_fields
                .iter()
                .position(|(n, _)| n == field_name)
                .unwrap_or(0);
            let field_type = struct_fields
                .get(field_idx)
                .map(|(_, t)| t.clone())
                .unwrap_or_else(|| "int".to_string());
            let llvm_ft = self.type_to_llvm(&field_type);
            let gep = self.new_temp();
            self.emit(&format!(
                "  {} = getelementptr %{}, %{}* {}, i32 0, i32 {}",
                gep, name, name, struct_ptr, field_idx
            ));
            self.emit(&format!(
                "  store {} {}, {}* {}",
                llvm_ft, val_reg, llvm_ft, gep
            ));
        }

        struct_ptr
    }

    fn gen_member_access(&mut self, object: &AstNode, field: &str) -> String {
        // Mutex guard .value — volatile load through lock boundary
        if let AstNode::Identifier { name: obj_name, .. } = object
            && (self.guard_vars.contains(obj_name.as_str())
                || self
                    .current_function_vars
                    .get(obj_name.as_str())
                    .map(|m| m.var_type.starts_with("MutexGuard<"))
                    .unwrap_or(false))
            && field == "value"
            && !self.is_unsafe_fn
        {
            let guard_ptr =
                if let Some(meta) = self.current_function_vars.get(obj_name.as_str()).cloned() {
                    if meta.llvm_name.starts_with("%arg_") {
                        meta.llvm_name.clone()
                    } else {
                        let loaded = self.new_temp();
                        self.emit(&format!("  {} = load i8*, i8** {}", loaded, meta.llvm_name));
                        loaded
                    }
                } else {
                    obj_name.clone()
                };
            let val_gep = self.new_temp();
            self.emit(&format!(
                "  {} = getelementptr i8, i8* {}, i64 40",
                val_gep, guard_ptr
            ));
            let val_ptr = self.new_temp();
            self.emit(&format!("  {} = bitcast i8* {} to i64*", val_ptr, val_gep));
            let result = self.new_temp();
            self.emit(&format!(
                "  {} = load volatile i64, i64* {}",
                result, val_ptr
            ));
            return result;
        }

        let obj_reg = self.gen_node(object);
        let struct_name = self.infer_struct_name(object);

        if let Some(struct_fields) = self.struct_types.get(&struct_name).cloned()
            && let Some(field_idx) = struct_fields.iter().position(|(n, _)| n == field)
        {
            let field_type = struct_fields[field_idx].1.clone();
            let llvm_ft = self.type_to_llvm(&field_type);
            let gep = self.new_temp();
            self.emit(&format!(
                "  {} = getelementptr %{}, %{}* {}, i32 0, i32 {}",
                gep, struct_name, struct_name, obj_reg, field_idx
            ));
            let result = self.new_temp();
            self.emit(&format!(
                "  {} = load {}, {}* {}",
                result, llvm_ft, llvm_ft, gep
            ));
            return result;
        }
        "0".to_string()
    }

    fn gen_reference(&mut self, expr: &AstNode) -> String {
        match expr {
            AstNode::Identifier { name, .. } => {
                if let Some(meta) = self.current_function_vars.get(name).cloned() {
                    if meta.var_type.starts_with('[') || meta.var_type == "array" {
                        return meta.llvm_name;
                    }
                    if self.struct_types.contains_key(&meta.var_type) && !meta.is_heap {
                        return meta.llvm_name;
                    }
                    if meta.llvm_name.starts_with("%arg_") {
                        return meta.llvm_name;
                    }
                    let result = self.new_temp();
                    let llvm_type = self.type_to_llvm(&meta.var_type);
                    let llvm_name = meta.llvm_name.clone();
                    self.emit(&format!(
                        "  {} = load {}, {}* {}",
                        result, llvm_type, llvm_type, llvm_name
                    ));
                    result
                } else {
                    "null".to_string()
                }
            }
            _ => self.gen_node(expr),
        }
    }

    fn gen_enum_value(
        &mut self,
        enum_name: &str,
        variant: &str,
        value: &Option<Box<AstNode>>,
    ) -> String {
        if enum_name == "Mutex" && variant == "new" {
            let inner_val = value
                .as_ref()
                .map(|v| self.gen_node(v))
                .unwrap_or_else(|| "0".to_string());
            let mutex_raw = self.new_temp();
            self.emit(&format!("  {} = call i8* @malloc(i64 48)", mutex_raw));
            self.emit(&format!(
                "  call void @InitializeCriticalSection(i8* {})",
                mutex_raw
            ));
            let val_gep = self.new_temp();
            self.emit(&format!(
                "  {} = getelementptr i8, i8* {}, i64 40",
                val_gep, mutex_raw
            ));
            let val_ptr = self.new_temp();
            self.emit(&format!("  {} = bitcast i8* {} to i64*", val_ptr, val_gep));
            self.emit(&format!("  store i64 {}, i64* {}", inner_val, val_ptr));
            return mutex_raw;
        }

        let tag = self
            .enum_types
            .get(enum_name)
            .and_then(|variants| variants.iter().position(|v| v == variant))
            .unwrap_or(0) as i64;

        let ptr = self.new_temp();
        self.emit(&format!("  {} = alloca {{ i32, i64 }}", ptr));
        let tag_ptr = self.new_temp();
        self.emit(&format!(
            "  {} = getelementptr {{ i32, i64 }}, {{ i32, i64 }}* {}, i32 0, i32 0",
            tag_ptr, ptr
        ));
        self.emit(&format!("  store i32 {}, i32* {}", tag, tag_ptr));

        let val = value
            .as_ref()
            .map(|v| self.gen_node(v))
            .unwrap_or_else(|| "0".to_string());
        let val_ptr = self.new_temp();
        self.emit(&format!(
            "  {} = getelementptr {{ i32, i64 }}, {{ i32, i64 }}* {}, i32 0, i32 1",
            val_ptr, ptr
        ));
        self.emit(&format!("  store i64 {}, i64* {}", val, val_ptr));
        ptr
    }

    fn gen_binary_op(&mut self, op: &BinOp, left: &AstNode, right: &AstNode) -> String {
        let left_reg = self.gen_node(left);
        let right_reg = self.gen_node(right);

        match op {
            BinOp::DotDot => right_reg,
            BinOp::Add if self.infer_type(left) == "string" => {
                let result = self.gen_string_concat(&left_reg, &right_reg);
                // Free owned string operands after concat
                let free_if_owned = |cg: &mut CodeGenerator, node: &AstNode| {
                    if let AstNode::Identifier { name, .. } = node
                        && let Some(meta) = cg.current_function_vars.get(name).cloned()
                        && !meta.is_string_literal
                    {
                        let loaded = cg.new_temp();
                        cg.emit(&format!("  {} = load i8*, i8** {}", loaded, meta.llvm_name));
                        cg.emit(&format!("  call void @free(i8* {})", loaded));
                    }
                };
                free_if_owned(self, right);
                free_if_owned(self, left);
                result
            }
            BinOp::Add => {
                let r = self.new_temp();
                self.emit(&format!("  {} = add i64 {}, {}", r, left_reg, right_reg));
                r
            }
            BinOp::Sub => {
                let r = self.new_temp();
                self.emit(&format!("  {} = sub i64 {}, {}", r, left_reg, right_reg));
                r
            }
            BinOp::Mul => {
                let r = self.new_temp();
                self.emit(&format!("  {} = mul i64 {}, {}", r, left_reg, right_reg));
                r
            }
            BinOp::Div => {
                let r = self.new_temp();
                self.emit(&format!("  {} = sdiv i64 {}, {}", r, left_reg, right_reg));
                r
            }
            BinOp::Mod => {
                let r = self.new_temp();
                self.emit(&format!("  {} = srem i64 {}, {}", r, left_reg, right_reg));
                r
            }
            BinOp::Equal if self.infer_type(left) == "string" => {
                let cmp = self.new_temp();
                self.emit(&format!(
                    "  {} = call i32 @strcmp(i8* {}, i8* {})",
                    cmp, left_reg, right_reg
                ));
                let r = self.new_temp();
                self.emit(&format!("  {} = icmp eq i32 {}, 0", r, cmp));
                r
            }
            BinOp::Equal => {
                let r = self.new_temp();
                self.emit(&format!(
                    "  {} = icmp eq i64 {}, {}",
                    r, left_reg, right_reg
                ));
                r
            }
            BinOp::NotEqual if self.infer_type(left) == "string" => {
                let cmp = self.new_temp();
                self.emit(&format!(
                    "  {} = call i32 @strcmp(i8* {}, i8* {})",
                    cmp, left_reg, right_reg
                ));
                let r = self.new_temp();
                self.emit(&format!("  {} = icmp ne i32 {}, 0", r, cmp));
                r
            }
            BinOp::NotEqual => {
                let r = self.new_temp();
                self.emit(&format!(
                    "  {} = icmp ne i64 {}, {}",
                    r, left_reg, right_reg
                ));
                r
            }
            BinOp::LessThan => {
                let r = self.new_temp();
                self.emit(&format!(
                    "  {} = icmp slt i64 {}, {}",
                    r, left_reg, right_reg
                ));
                r
            }
            BinOp::LessEqual => {
                let r = self.new_temp();
                self.emit(&format!(
                    "  {} = icmp sle i64 {}, {}",
                    r, left_reg, right_reg
                ));
                r
            }
            BinOp::GreaterThan => {
                let r = self.new_temp();
                self.emit(&format!(
                    "  {} = icmp sgt i64 {}, {}",
                    r, left_reg, right_reg
                ));
                r
            }
            BinOp::GreaterEqual => {
                let r = self.new_temp();
                self.emit(&format!(
                    "  {} = icmp sge i64 {}, {}",
                    r, left_reg, right_reg
                ));
                r
            }
            BinOp::And => {
                let r = self.new_temp();
                self.emit(&format!("  {} = and i1 {}, {}", r, left_reg, right_reg));
                r
            }
            BinOp::Or => {
                let r = self.new_temp();
                self.emit(&format!("  {} = or i1 {}, {}", r, left_reg, right_reg));
                r
            }
        }
    }

    fn gen_match(&mut self, value: &AstNode, arms: &[crate::ast::MatchArm]) -> String {
        let value_reg = self.gen_node(value);
        let end_label = self.new_label("match_end");
        let is_enum = arms
            .iter()
            .any(|a| matches!(a.pattern, Pattern::EnumPattern { .. }));

        if is_enum {
            let tag_ptr = self.new_temp();
            self.emit(&format!(
                "  {} = getelementptr {{ i32, i64 }}, {{ i32, i64 }}* {}, i32 0, i32 0",
                tag_ptr, value_reg
            ));
            let tag = self.new_temp();
            self.emit(&format!("  {} = load i32, i32* {}", tag, tag_ptr));

            for (i, arm) in arms.iter().enumerate() {
                let arm_label = self.new_label(&format!("match_arm_{}", i));
                let next_label = if i < arms.len() - 1 {
                    self.new_label(&format!("match_check_{}", i + 1))
                } else {
                    end_label.clone()
                };

                match &arm.pattern {
                    Pattern::EnumPattern {
                        enum_name,
                        variant,
                        binding,
                    } => {
                        let variant_tag = self
                            .enum_types
                            .get(enum_name)
                            .and_then(|vs| vs.iter().position(|v| v == variant))
                            .unwrap_or(i) as i32;
                        let cond = self.new_temp();
                        self.emit(&format!(
                            "  {} = icmp eq i32 {}, {}",
                            cond, tag, variant_tag
                        ));
                        self.emit(&format!(
                            "  br i1 {}, label %{}, label %{}",
                            cond, arm_label, next_label
                        ));
                        self.emit(&format!("{}:", arm_label));

                        if let Some(b) = binding {
                            let val_ptr = self.new_temp();
                            self.emit(&format!(
                                "  {} = getelementptr {{ i32, i64 }}, {{ i32, i64 }}* {}, i32 0, i32 1",
                                val_ptr, value_reg
                            ));
                            let val = self.new_temp();
                            self.emit(&format!("  {} = load i64, i64* {}", val, val_ptr));
                            let var_ptr = self.new_temp();
                            self.emit(&format!("  {} = alloca i64", var_ptr));
                            self.emit(&format!("  store i64 {}, i64* {}", val, var_ptr));
                            self.current_function_vars.insert(
                                b.clone(),
                                VarMetadata {
                                    llvm_name: var_ptr,
                                    var_type: "int".to_string(),
                                    is_heap: false,
                                    array_size: None,
                                    is_string_literal: false,
                                },
                            );
                        }

                        self.block_terminated = false;
                        let arm_val = self.gen_node(&arm.body);
                        self.finish_match_arm(&arm_val, &end_label);
                    }
                    Pattern::Wildcard | Pattern::Identifier(_) => {
                        self.emit(&format!("  br label %{}", arm_label));
                        self.emit(&format!("{}:", arm_label));
                        self.block_terminated = false;
                        let arm_val = self.gen_node(&arm.body);
                        self.finish_match_arm(&arm_val, &end_label);
                    }
                    _ => {}
                }

                if i < arms.len() - 1 {
                    self.emit(&format!("{}:", next_label));
                }
            }
        } else {
            for (i, arm) in arms.iter().enumerate() {
                let arm_label = self.new_label(&format!("match_arm_{}", i));
                let next_label = if i < arms.len() - 1 {
                    self.new_label(&format!("match_check_{}", i + 1))
                } else {
                    end_label.clone()
                };

                match &arm.pattern {
                    Pattern::NumberPattern(n) => {
                        let cond = self.new_temp();
                        self.emit(&format!("  {} = icmp eq i64 {}, {}", cond, value_reg, n));
                        self.emit(&format!(
                            "  br i1 {}, label %{}, label %{}",
                            cond, arm_label, next_label
                        ));
                        self.emit(&format!("{}:", arm_label));
                        self.block_terminated = false;
                        let arm_val = self.gen_node(&arm.body);
                        self.finish_match_arm(&arm_val, &end_label);
                    }
                    Pattern::StringPattern(s) => {
                        let str_id = self.new_string_literal(s);
                        let str_len = s.len() + 1;
                        let str_ptr = self.new_temp();
                        self.emit(&format!(
                            "  {} = getelementptr inbounds [{} x i8], [{} x i8]* @{}, i64 0, i64 0",
                            str_ptr, str_len, str_len, str_id
                        ));
                        let cmp = self.new_temp();
                        self.emit(&format!(
                            "  {} = call i32 @strcmp(i8* {}, i8* {})",
                            cmp, value_reg, str_ptr
                        ));
                        let cond = self.new_temp();
                        self.emit(&format!("  {} = icmp eq i32 {}, 0", cond, cmp));
                        self.emit(&format!(
                            "  br i1 {}, label %{}, label %{}",
                            cond, arm_label, next_label
                        ));
                        self.emit(&format!("{}:", arm_label));
                        self.block_terminated = false;
                        let arm_val = self.gen_node(&arm.body);
                        self.finish_match_arm(&arm_val, &end_label);
                    }
                    Pattern::Wildcard | Pattern::Identifier(_) => {
                        self.emit(&format!("  br label %{}", arm_label));
                        self.emit(&format!("{}:", arm_label));
                        self.block_terminated = false;
                        let arm_val = self.gen_node(&arm.body);
                        self.finish_match_arm(&arm_val, &end_label);
                    }
                    _ => {}
                }

                if i < arms.len() - 1 {
                    self.emit(&format!("{}:", next_label));
                }
            }
        }

        self.emit(&format!("{}:", end_label));
        self.block_terminated = false;
        "0".to_string()
    }

    fn finish_match_arm(&mut self, arm_val: &str, end_label: &str) {
        if !self.block_terminated {
            if self.current_function_return_type != "void" {
                let ret_type = self.current_function_return_type.clone();
                self.emit(&format!("  ret {} {}", ret_type, arm_val));
                self.block_terminated = true;
            } else {
                self.emit(&format!("  br label %{}", end_label));
            }
        }
    }

    fn gen_call(&mut self, name: &str, args: &[AstNode]) -> String {
        match name {
            "print" if !args.is_empty() => match self.infer_type(&args[0]).as_str() {
                "string" => {
                    let reg = self.gen_node(&args[0]);
                    let r = self.new_temp();
                    self.emit(&format!("  {} = call i32 @puts(i8* {})", r, reg));
                    r
                }
                "bool" => {
                    let reg = self.gen_node(&args[0]);
                    let ext = self.new_temp();
                    self.emit(&format!("  {} = zext i1 {} to i64", ext, reg));
                    self.emit(&format!("  call void @brn_print_int(i64 {})", ext));
                    "0".to_string()
                }
                _ => {
                    let reg = self.gen_node(&args[0]);
                    self.emit(&format!("  call void @brn_print_int(i64 {})", reg));
                    "0".to_string()
                }
            },
            "read_file" if !args.is_empty() => {
                let reg = self.gen_node(&args[0]);
                let r = self.new_temp();
                self.emit(&format!("  {} = call i8* @read_file_impl(i8* {})", r, reg));
                r
            }
            "read_input" => {
                let r = self.new_temp();
                self.emit(&format!("  {} = call i8* @read_input_impl()", r));
                r
            }
            "write_file" if args.len() >= 2 => {
                let f = self.gen_node(&args[0]);
                let c = self.gen_node(&args[1]);
                let r = self.new_temp();
                self.emit(&format!(
                    "  {} = call i32 @write_file_impl(i8* {}, i8* {})",
                    r, f, c
                ));
                let r64 = self.new_temp();
                self.emit(&format!("  {} = sext i32 {} to i64", r64, r));
                r64
            }
            "vec_new" => {
                let r = self.new_temp();
                self.emit(&format!("  {} = call i8* @vec_new_impl()", r));
                r
            }
            "vec_push" if args.len() >= 2 => {
                let v = self.gen_node(&args[0]);
                let val = self.gen_node(&args[1]);
                self.emit(&format!(
                    "  call void @vec_push_impl(i8* {}, i64 {})",
                    v, val
                ));
                "0".to_string()
            }
            "vec_get" if args.len() >= 2 => {
                let v = self.gen_node(&args[0]);
                let idx = self.gen_node(&args[1]);
                let r = self.new_temp();
                self.emit(&format!(
                    "  {} = call i64 @vec_get_impl(i8* {}, i64 {})",
                    r, v, idx
                ));
                r
            }
            "vec_set" if args.len() >= 3 => {
                let v = self.gen_node(&args[0]);
                let idx = self.gen_node(&args[1]);
                let val = self.gen_node(&args[2]);
                self.emit(&format!(
                    "  call void @vec_set_impl(i8* {}, i64 {}, i64 {})",
                    v, idx, val
                ));
                "0".to_string()
            }
            "vec_len" if !args.is_empty() => {
                let v = self.gen_node(&args[0]);
                let r = self.new_temp();
                self.emit(&format!("  {} = call i64 @vec_len_impl(i8* {})", r, v));
                r
            }
            "int_to_string" if !args.is_empty() => {
                let n = self.gen_node(&args[0]);
                let r = self.new_temp();
                self.emit(&format!(
                    "  {} = call i8* @int_to_string_impl(i64 {})",
                    r, n
                ));
                r
            }
            _ => self.gen_user_call(name, args),
        }
    }

    fn gen_user_call(&mut self, name: &str, args: &[AstNode]) -> String {
        let mut arg_regs: Vec<String> = Vec::new();
        let mut arg_types: Vec<String> = Vec::new();

        for arg_node in args {
            match arg_node {
                AstNode::Reference(inner) => match inner.as_ref() {
                    AstNode::Identifier { name: var_name, .. } => {
                        if let Some(meta) = self.current_function_vars.get(var_name).cloned() {
                            if let Some(size) = meta.array_size {
                                arg_regs.push(meta.llvm_name.clone());
                                arg_types.push(format!("[{} x i64]*", size));
                            } else if meta.var_type.starts_with("Mutex<")
                                || meta.var_type == "string"
                            {
                                let loaded = self.new_temp();
                                self.emit(&format!(
                                    "  {} = load i8*, i8** {}",
                                    loaded, meta.llvm_name
                                ));
                                arg_regs.push(loaded);
                                arg_types.push("i8*".to_string());
                            } else if self.struct_types.contains_key(&meta.var_type) {
                                let struct_name = meta.var_type.clone();
                                if meta.llvm_name.starts_with("%arg_") {
                                    arg_regs.push(meta.llvm_name.clone());
                                } else {
                                    let loaded = self.new_temp();
                                    self.emit(&format!(
                                        "  {} = load %{}*, %{}** {}",
                                        loaded, struct_name, struct_name, meta.llvm_name
                                    ));
                                    arg_regs.push(loaded);
                                }
                                arg_types.push(format!("%{}*", struct_name));
                            } else {
                                arg_regs.push(meta.llvm_name.clone());
                                arg_types.push(format!("{}*", self.type_to_llvm(&meta.var_type)));
                            }
                        } else {
                            arg_regs.push("null".to_string());
                            arg_types.push("i8*".to_string());
                        }
                    }
                    _ => {
                        let reg = self.gen_node(inner);
                        arg_regs.push(reg);
                        arg_types.push("i8*".to_string());
                    }
                },
                _ => {
                    let reg = self.gen_node(arg_node);
                    let arg_type = self.infer_type(arg_node);
                    if arg_type == "string" {
                        // Pass string by value: copy so the callee can free safely
                        let len = self.new_temp();
                        let len1 = self.new_temp();
                        let copy = self.new_temp();
                        let copied = self.new_temp();
                        self.emit(&format!("  {} = call i64 @strlen(i8* {})", len, reg));
                        self.emit(&format!("  {} = add i64 {}, 1", len1, len));
                        self.emit(&format!("  {} = call i8* @malloc(i64 {})", copy, len1));
                        self.emit(&format!(
                            "  {} = call i8* @strcpy(i8* {}, i8* {})",
                            copied, copy, reg
                        ));
                        arg_regs.push(copied);
                    } else {
                        arg_regs.push(reg);
                    }
                    arg_types.push(self.type_to_llvm(&arg_type));
                }
            }
        }

        let args_str = arg_types
            .iter()
            .zip(&arg_regs)
            .map(|(ty, reg)| format!("{} {}", ty, reg))
            .collect::<Vec<_>>()
            .join(", ");

        let return_type = self
            .function_signatures
            .get(name)
            .cloned()
            .unwrap_or_else(|| "i64".to_string());

        let mangled = Self::mangle_fn(name);
        if return_type == "void" {
            self.emit(&format!("  call void @{}({})", mangled, args_str));
            "0".to_string()
        } else {
            let result = self.new_temp();
            self.emit(&format!(
                "  {} = call {} @{}({})",
                result, return_type, mangled, args_str
            ));
            result
        }
    }

    fn gen_method_call(&mut self, object: &AstNode, method: &str, args: &[AstNode]) -> String {
        let obj_type = self.infer_type(object);
        match method {
            "len" => {
                let obj_reg = self.gen_node(object);
                let result = self.new_temp();
                if obj_type == "Vec" {
                    self.emit(&format!(
                        "  {} = call i64 @vec_len_impl(i8* {})",
                        result, obj_reg
                    ));
                } else {
                    self.emit(&format!("  {} = call i64 @strlen(i8* {})", result, obj_reg));
                }
                result
            }
            "char_at" if !args.is_empty() => {
                let obj_reg = self.gen_node(object);
                let idx = self.gen_node(&args[0]);
                let char_ptr = self.new_temp();
                self.emit(&format!(
                    "  {} = getelementptr i8, i8* {}, i64 {}",
                    char_ptr, obj_reg, idx
                ));
                let result = self.new_temp();
                self.emit(&format!("  {} = load i8, i8* {}", result, char_ptr));
                let ext = self.new_temp();
                self.emit(&format!("  {} = sext i8 {} to i64", ext, result));
                ext
            }
            "push" if !args.is_empty() => {
                let obj_reg = self.gen_node(object);
                let val = self.gen_node(&args[0]);
                self.emit(&format!(
                    "  call void @vec_push_impl(i8* {}, i64 {})",
                    obj_reg, val
                ));
                "0".to_string()
            }
            "get" if !args.is_empty() => {
                let obj_reg = self.gen_node(object);
                let idx = self.gen_node(&args[0]);
                let r = self.new_temp();
                self.emit(&format!(
                    "  {} = call i64 @vec_get_impl(i8* {}, i64 {})",
                    r, obj_reg, idx
                ));
                r
            }
            "set" if args.len() >= 2 => {
                let obj_reg = self.gen_node(object);
                let idx = self.gen_node(&args[0]);
                let val = self.gen_node(&args[1]);
                self.emit(&format!(
                    "  call void @vec_set_impl(i8* {}, i64 {}, i64 {})",
                    obj_reg, idx, val
                ));
                "0".to_string()
            }
            "lock" if !self.is_unsafe_fn => {
                if let AstNode::Identifier { name: obj_name, .. } = object
                    && let Some(meta) = self.current_function_vars.get(obj_name).cloned()
                {
                    let mutex_ptr = if meta.llvm_name.starts_with("%arg_") {
                        meta.llvm_name.clone()
                    } else {
                        let loaded = self.new_temp();
                        self.emit(&format!("  {} = load i8*, i8** {}", loaded, meta.llvm_name));
                        loaded
                    };
                    self.emit(&format!(
                        "  call void @EnterCriticalSection(i8* {})",
                        mutex_ptr
                    ));
                    self.guard_vars.insert(obj_name.clone());
                    return mutex_ptr;
                }
                "null".to_string()
            }
            "lock" => self.gen_node(object),
            _ => "0".to_string(),
        }
    }
}
