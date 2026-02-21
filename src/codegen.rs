use crate::parser::{AstNode, BinOp, Parameter, Pattern};
use std::collections::HashMap;

pub struct CodeGenerator {
    output: String,
    string_counter: usize,
    temp_counter: usize,
    label_counter: usize,
    string_literals: Vec<(String, String)>,
    current_function_vars: HashMap<String, VarMetadata>,
    loop_stack: Vec<LoopLabels>,
    enum_types: HashMap<String, Vec<String>>,
    block_terminated: bool,
    current_function_name: String,
    current_function_return_type: String,
    function_signatures: HashMap<String, String>,
}

#[derive(Clone)]
struct VarMetadata {
    llvm_name: String,
    var_type: String,
    is_heap: bool,
    array_size: Option<usize>,
    is_string_literal: bool,
}

struct LoopLabels {
    continue_label: String,
    break_label: String,
}
fn get_target_triple() -> &'static str {
    if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc"
    } else if cfg!(target_os = "macos") {
        "x86_64-apple-macosx10.15.0"
    } else {
        "x86_64-pc-linux-gnu"
    }
}
impl CodeGenerator {
    pub fn new() -> Self {
        CodeGenerator {
            output: String::new(),
            string_counter: 0,
            temp_counter: 0,
            label_counter: 0,
            string_literals: Vec::new(),
            current_function_vars: HashMap::new(),
            loop_stack: Vec::new(),
            enum_types: HashMap::new(),
            block_terminated: false,
            current_function_name: String::new(),
            current_function_return_type: String::new(),
            function_signatures: HashMap::new(),
        }
    }

    pub fn generate(&mut self, ast: &AstNode) -> String {
        self.emit_header();

        if let AstNode::Program(nodes) = ast {
            for node in nodes {
                self.gen_node(node);
            }
        }

        self.emit_footer();
        self.build_output()
    }

    fn emit_header(&mut self) {
        self.emit("declare i32 @puts(i8*)");
        self.emit("declare i32 @strcmp(i8*, i8*)");
        self.emit("declare i8* @malloc(i64)");
        self.emit("declare void @free(i8*)");
        self.emit("declare i8* @strcpy(i8*, i8*)");
        self.emit("declare i64 @strlen(i8*)");
        self.emit("declare i32 @printf(i8*, ...)");
        self.emit("declare i32 @sprintf(i8*, i8*, ...)");
        self.emit("declare i8* @fopen(i8*, i8*)");
        self.emit("declare i32 @fclose(i8*)");
        self.emit("declare i64 @fread(i8*, i64, i64, i8*)");
        self.emit("declare i64 @fwrite(i8*, i64, i64, i8*)");
        self.emit("declare i32 @fseek(i8*, i64, i32)");
        self.emit("declare i64 @ftell(i8*)");
        self.emit("");

        self.emit("define i8* @read_file_impl(i8* %filename) {");
        self.emit(
            "  %mode = getelementptr inbounds [2 x i8], [2 x i8]* @.str.mode.r, i64 0, i64 0",
        );
        self.emit("  %file = call i8* @fopen(i8* %filename, i8* %mode)");
        self.emit("  %is_null = icmp eq i8* %file, null");
        self.emit("  br i1 %is_null, label %error, label %read");
        self.emit("error:");
        self.emit("  ret i8* null");
        self.emit("read:");
        self.emit("  call i32 @fseek(i8* %file, i64 0, i32 2)");
        self.emit("  %size = call i64 @ftell(i8* %file)");
        self.emit("  call i32 @fseek(i8* %file, i64 0, i32 0)");
        self.emit("  %size_plus_1 = add i64 %size, 1");
        self.emit("  %buffer = call i8* @malloc(i64 %size_plus_1)");
        self.emit("  %read_size = call i64 @fread(i8* %buffer, i64 1, i64 %size, i8* %file)");
        self.emit("  %null_pos = getelementptr i8, i8* %buffer, i64 %size");
        self.emit("  store i8 0, i8* %null_pos");
        self.emit("  call i32 @fclose(i8* %file)");
        self.emit("  ret i8* %buffer");
        self.emit("}");
        self.emit("");

        self.emit("define i32 @write_file_impl(i8* %filename, i8* %content) {");
        self.emit(
            "  %mode = getelementptr inbounds [2 x i8], [2 x i8]* @.str.mode.w, i64 0, i64 0",
        );
        self.emit("  %file = call i8* @fopen(i8* %filename, i8* %mode)");
        self.emit("  %is_null = icmp eq i8* %file, null");
        self.emit("  br i1 %is_null, label %error, label %write");
        self.emit("error:");
        self.emit("  ret i32 0");
        self.emit("write:");
        self.emit("  %len = call i64 @strlen(i8* %content)");
        self.emit("  %written = call i64 @fwrite(i8* %content, i64 1, i64 %len, i8* %file)");
        self.emit("  call i32 @fclose(i8* %file)");
        self.emit("  ret i32 1");
        self.emit("}");
        self.emit("");

        self.string_literals
            .push((".str.mode.r".to_string(), "r".to_string()));
        self.string_literals
            .push((".str.mode.w".to_string(), "w".to_string()));
    }

    fn emit_footer(&mut self) {
        for (id, value) in &self.string_literals {
            let len = value.len() + 1;
            let escaped = self.escape_string(value);
            self.output = format!(
                "@{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1\n{}",
                id, len, escaped, self.output
            );
        }
    }

    fn gen_node(&mut self, node: &AstNode) -> String {
        match node {
            AstNode::EnumDef { name, variants } => {
                let variant_names: Vec<String> = variants.iter().map(|v| v.name.clone()).collect();
                self.enum_types.insert(name.clone(), variant_names);
                "0".to_string()
            }

            AstNode::EnumValue {
                enum_name,
                variant,
                value,
            } => {
                let tag = if let Some(variants) = self.enum_types.get(enum_name) {
                    variants.iter().position(|v| v == variant).unwrap_or(0) as i64
                } else {
                    0
                };

                let ptr = self.new_temp();
                self.emit(&format!("  {} = alloca {{ i32, i64 }}", ptr));

                let tag_ptr = self.new_temp();
                self.emit(&format!(
                    "  {} = getelementptr {{ i32, i64 }}, {{ i32, i64 }}* {}, i32 0, i32 0",
                    tag_ptr, ptr
                ));
                self.emit(&format!("  store i32 {}, i32* {}", tag, tag_ptr));

                let val = if let Some(v) = value {
                    self.gen_node(v)
                } else {
                    "0".to_string()
                };

                let val_ptr = self.new_temp();
                self.emit(&format!(
                    "  {} = getelementptr {{ i32, i64 }}, {{ i32, i64 }}* {}, i32 0, i32 1",
                    val_ptr, ptr
                ));
                self.emit(&format!("  store i64 {}, i64* {}", val, val_ptr));

                ptr
            }

            AstNode::Match { value, arms } => {
                let value_reg = self.gen_node(value);
                let end_label = self.new_label("match_end");

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
                            variant, binding, ..
                        } => {
                            let variant_tag = i as i32;
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

                            if let Some(binding) = binding {
                                let val_ptr = self.new_temp();
                                self.emit(&format!("  {} = getelementptr {{ i32, i64 }}, {{ i32, i64 }}* {}, i32 0, i32 1", val_ptr, value_reg));
                                let val = self.new_temp();
                                self.emit(&format!("  {} = load i64, i64* {}", val, val_ptr));

                                let var_ptr = self.new_temp();
                                self.emit(&format!("  {} = alloca i64", var_ptr));
                                self.emit(&format!("  store i64 {}, i64* {}", val, var_ptr));

                                self.current_function_vars.insert(
                                    binding.clone(),
                                    VarMetadata {
                                        llvm_name: var_ptr,
                                        var_type: "int".to_string(),
                                        is_heap: false,
                                        array_size: None,
                                        is_string_literal: false,
                                    },
                                );
                            }

                            self.gen_node(&arm.body);
                            self.emit(&format!("  br label %{}", end_label));
                        }
                        Pattern::Wildcard => {
                            self.emit(&format!("  br label %{}", arm_label));
                            self.emit(&format!("{}:", arm_label));
                            self.gen_node(&arm.body);
                            self.emit(&format!("  br label %{}", end_label));
                        }
                        Pattern::Identifier(_) => {
                            self.emit(&format!("  br label %{}", arm_label));
                            self.emit(&format!("{}:", arm_label));
                            self.gen_node(&arm.body);
                            self.emit(&format!("  br label %{}", end_label));
                        }
                    }

                    if i < arms.len() - 1 {
                        self.emit(&format!("{}:", next_label));
                    }
                }

                self.emit(&format!("{}:", end_label));
                "0".to_string()
            }

            AstNode::FunctionDef {
                name,
                params,
                body,
                return_type,
            } => self.gen_function(name, params, body, return_type),

            AstNode::LetBinding { name, value, .. } => {
                let value_reg = self.gen_node(value);
                let var_type = self.infer_llvm_type(value);

                let is_string_literal = matches!(value.as_ref(), AstNode::StringLit(_));
                let is_heap = var_type == "string" && !is_string_literal;

                let array_size = if let AstNode::ArrayLit(elements) = value.as_ref() {
                    Some(elements.len())
                } else {
                    None
                };

                let ptr = self.new_temp();
                let llvm_type_str = self.type_to_llvm(&var_type).to_string();
                self.emit(&format!("  {} = alloca {}", ptr, llvm_type_str));
                self.emit(&format!(
                    "  store {} {}, {}* {}",
                    llvm_type_str, value_reg, llvm_type_str, ptr
                ));

                self.current_function_vars.insert(
                    name.clone(),
                    VarMetadata {
                        llvm_name: ptr.clone(),
                        var_type,
                        is_heap,
                        array_size,
                        is_string_literal,
                    },
                );

                ptr
            }

            AstNode::ArrayAssignment {
                array,
                index,
                value,
                ..
            } => {
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

            AstNode::Assignment { name, value, .. } => {
                let value_reg = self.gen_node(value);

                if let Some(meta) = self.current_function_vars.get(name).cloned() {
                    let llvm_type_str = self.type_to_llvm(&meta.var_type).to_string();
                    let llvm_name = meta.llvm_name.clone();
                    self.emit(&format!(
                        "  store {} {}, {}* {}",
                        llvm_type_str, value_reg, llvm_type_str, llvm_name
                    ));
                }

                value_reg
            }

            AstNode::If {
                condition,
                then_block,
                else_block,
            } => {
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
                if let Some(else_block) = else_block {
                    self.emit(&format!("{}:", else_label));
                    self.block_terminated = false;
                    self.gen_node(else_block);
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

            AstNode::While { condition, body } => {
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

            AstNode::For {
                variable,
                iterator,
                body,
            } => {
                let (start_val, end_val) = if let AstNode::BinaryOp {
                    op: BinOp::DotDot,
                    left,
                    right,
                } = iterator.as_ref()
                {
                    (self.gen_node(left), self.gen_node(right))
                } else {
                    ("0".to_string(), self.gen_node(iterator)) // no range, just end bound
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
                    variable.clone(),
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

            AstNode::Break => {
                if let Some(labels) = self.loop_stack.last() {
                    let break_label = labels.break_label.clone();
                    self.emit(&format!("  br label %{}", break_label));
                    self.block_terminated = true;
                }
                "0".to_string()
            }

            AstNode::Continue => {
                if let Some(labels) = self.loop_stack.last() {
                    let continue_label = labels.continue_label.clone();
                    self.emit(&format!("  br label %{}", continue_label));
                    self.block_terminated = true;
                }
                "0".to_string()
            }

            AstNode::Return(value) => {
                if let Some(value) = value {
                    let value_reg = self.gen_node(value);
                    let ret_type = &self.current_function_return_type.clone();
                    self.emit(&format!("  ret {} {}", ret_type, value_reg));
                } else {
                    self.emit("  ret i32 0");
                }
                self.block_terminated = true;
                "0".to_string()
            }

            AstNode::Block(statements) => {
                let mut last_reg = String::new();
                let vars_before = self.current_function_vars.clone();

                for stmt in statements {
                    last_reg = self.gen_node(stmt);
                }

                let vars_to_free: Vec<_> = self
                    .current_function_vars
                    .iter()
                    .filter(|(name, meta)| {
                        meta.is_heap
                            && !meta.is_string_literal
                            && !vars_before.contains_key(name.as_str())
                    })
                    .map(|(_, meta)| meta.llvm_name.clone())
                    .collect();

                for llvm_name in vars_to_free {
                    let ptr_reg = self.new_temp();
                    self.emit(&format!("  {} = load i8*, i8** {}", ptr_reg, llvm_name));
                    self.emit(&format!("  call void @free(i8* {})", ptr_reg));
                }

                last_reg
            }

            AstNode::ExpressionStatement(expr) => self.gen_node(expr),

            AstNode::BinaryOp { op, left, right } => {
                let left_reg = self.gen_node(left);
                let right_reg = self.gen_node(right);

                match op {
                    BinOp::DotDot => right_reg,
                    BinOp::Add => {
                        if self.infer_llvm_type(left) == "string" {
                            self.gen_string_concat(&left_reg, &right_reg)
                        } else {
                            let result = self.new_temp();
                            self.emit(&format!(
                                "  {} = add i64 {}, {}",
                                result, left_reg, right_reg
                            ));
                            result
                        }
                    }
                    BinOp::Sub => {
                        let result = self.new_temp();
                        self.emit(&format!(
                            "  {} = sub i64 {}, {}",
                            result, left_reg, right_reg
                        ));
                        result
                    }
                    BinOp::Mul => {
                        let result = self.new_temp();
                        self.emit(&format!(
                            "  {} = mul i64 {}, {}",
                            result, left_reg, right_reg
                        ));
                        result
                    }
                    BinOp::Div => {
                        let result = self.new_temp();
                        self.emit(&format!(
                            "  {} = sdiv i64 {}, {}",
                            result, left_reg, right_reg
                        ));
                        result
                    }
                    BinOp::Mod => {
                        let result = self.new_temp();
                        self.emit(&format!(
                            "  {} = srem i64 {}, {}",
                            result, left_reg, right_reg
                        ));
                        result
                    }
                    BinOp::Equal => {
                        if self.infer_llvm_type(left) == "string" {
                            let cmp = self.new_temp();
                            self.emit(&format!(
                                "  {} = call i32 @strcmp(i8* {}, i8* {})",
                                cmp, left_reg, right_reg
                            ));

                            let result = self.new_temp();
                            self.emit(&format!("  {} = icmp eq i32 {}, 0", result, cmp));
                            result
                        } else {
                            let result = self.new_temp();
                            self.emit(&format!(
                                "  {} = icmp eq i64 {}, {}",
                                result, left_reg, right_reg
                            ));

                            result
                        }
                    }
                    BinOp::NotEqual => {
                        if self.infer_llvm_type(left) == "string" {
                            let cmp = self.new_temp();
                            self.emit(&format!(
                                "  {} = call i32 @strcmp(i8* {}, i8* {})",
                                cmp, left_reg, right_reg
                            ));
                            let result = self.new_temp();
                            self.emit(&format!("  {} = icmp ne i32 {}, 0", result, cmp));
                            result
                        } else {
                            let result = self.new_temp();
                            self.emit(&format!(
                                "  {} = icmp ne i64 {}, {}",
                                result, left_reg, right_reg
                            ));
                            result
                        }
                    }
                    BinOp::LessThan => {
                        let result = self.new_temp();
                        self.emit(&format!(
                            "  {} = icmp slt i64 {}, {}",
                            result, left_reg, right_reg
                        ));
                        result
                    }
                    BinOp::LessEqual => {
                        let result = self.new_temp();
                        self.emit(&format!(
                            "  {} = icmp sle i64 {}, {}",
                            result, left_reg, right_reg
                        ));
                        result
                    }
                    BinOp::GreaterThan => {
                        let result = self.new_temp();
                        self.emit(&format!(
                            "  {} = icmp sgt i64 {}, {}",
                            result, left_reg, right_reg
                        ));
                        result
                    }
                    BinOp::GreaterEqual => {
                        let result = self.new_temp();
                        self.emit(&format!(
                            "  {} = icmp sge i64 {}, {}",
                            result, left_reg, right_reg
                        ));
                        result
                    }
                    BinOp::And => {
                        let result = self.new_temp();
                        self.emit(&format!(
                            "  {} = and i1 {}, {}",
                            result, left_reg, right_reg
                        ));
                        result
                    }
                    BinOp::Or => {
                        let result = self.new_temp();
                        self.emit(&format!("  {} = or i1 {}, {}", result, left_reg, right_reg));
                        result
                    }
                }
            }

            AstNode::UnaryOp { op, operand } => {
                let operand_reg = self.gen_node(operand);
                let result = self.new_temp();

                match op {
                    crate::parser::UnOp::Not => {
                        self.emit(&format!("  {} = xor i1 {}, true", result, operand_reg));
                    }
                    crate::parser::UnOp::Negate => {
                        self.emit(&format!("  {} = sub i64 0, {}", result, operand_reg));
                    }
                }

                result
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
                let array_type = format!("[{} x i64]", size);
                let ptr = self.new_temp();
                self.emit(&format!("  {} = alloca {}", ptr, array_type));

                for (i, elem) in elements.iter().enumerate() {
                    let value = self.gen_node(elem);
                    let elem_ptr = self.new_temp();
                    self.emit(&format!(
                        "  {} = getelementptr [{} x i64], [{} x i64]* {}, i64 0, i64 {}",
                        elem_ptr, size, size, ptr, i
                    ));
                    self.emit(&format!("  store i64 {}, i64* {}", value, elem_ptr));
                }

                ptr
            }

            AstNode::Index { array, index } => {
                let index_val = self.gen_node(index);

                let (array_ptr, array_size) = match array.as_ref() {
                    AstNode::Identifier { name, .. } => {
                        if let Some(meta) = self.current_function_vars.get(name) {
                            let size = meta.array_size.unwrap_or(100);
                            (meta.llvm_name.clone(), size)
                        } else {
                            eprintln!("CODEGEN ERROR: Array '{}' not found!", name);
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
                    let result = self.new_temp();
                    let llvm_type_str = self.type_to_llvm(&meta.var_type).to_string();
                    let llvm_name = meta.llvm_name.clone();
                    self.emit(&format!(
                        "  {} = load {}, {}* {}",
                        result, llvm_type_str, llvm_type_str, llvm_name
                    ));
                    result
                } else {
                    eprintln!(
                        "CODEGEN ERROR: Variable '{}' not found in current scope!",
                        name
                    );
                    "0".to_string()
                }
            }

            AstNode::Reference(expr) => match expr.as_ref() {
                AstNode::Identifier { name, .. } => {
                    if let Some(meta) = self.current_function_vars.get(name).cloned() {
                        if meta.var_type.starts_with('[') || meta.var_type == "array" {
                            return meta.llvm_name;
                        }
                        meta.llvm_name
                    } else {
                        eprintln!(
                            "CODEGEN ERROR: Variable '{}' not found for reference!",
                            name
                        );
                        "null".to_string()
                    }
                }
                _ => self.gen_node(expr),
            },

            AstNode::Call { name, args } => match name.as_str() {
                "print" if !args.is_empty() => match self.infer_llvm_type(&args[0]) {
                    t if t == "string" => {
                        let arg_reg = self.gen_node(&args[0]);
                        let result = self.new_temp();
                        self.emit(&format!("  {} = call i32 @puts(i8* {})", result, arg_reg));
                        result
                    }
                    _ => {
                        let arg_reg = self.gen_node(&args[0]);
                        let fmt = self.new_string_literal("%lld\n");
                        let fmt_ptr = self.new_temp();
                        let fmt_len = "%lld\n".len() + 1;
                        self.emit(&format!(
                            "  {} = getelementptr inbounds [{} x i8], [{} x i8]* @{}, i64 0, i64 0",
                            fmt_ptr, fmt_len, fmt_len, fmt
                        ));
                        let result = self.new_temp();
                        self.emit(&format!(
                            "  {} = call i32 (i8*, ...) @printf(i8* {}, i64 {})",
                            result, fmt_ptr, arg_reg
                        ));
                        result
                    }
                },
                "read_file" if !args.is_empty() => {
                    let filename_reg = self.gen_node(&args[0]);
                    let result = self.new_temp();
                    self.emit(&format!(
                        "  {} = call i8* @read_file_impl(i8* {})",
                        result, filename_reg
                    ));
                    result
                }
                "write_file" if args.len() >= 2 => {
                    let filename_reg = self.gen_node(&args[0]);
                    let content_reg = self.gen_node(&args[1]);
                    let result = self.new_temp();
                    self.emit(&format!(
                        "  {} = call i32 @write_file_impl(i8* {}, i8* {})",
                        result, filename_reg, content_reg
                    ));
                    let result_i64 = self.new_temp();
                    self.emit(&format!("  {} = sext i32 {} to i64", result_i64, result));
                    result_i64
                }
                _ => {
                    let mut arg_regs = Vec::new();
                    let mut arg_types = Vec::new();

                    for arg_node in args {
                        match arg_node {
                            AstNode::Reference(inner) => match inner.as_ref() {
                                AstNode::Identifier { name: var_name, .. } => {
                                    if let Some(meta) = self.current_function_vars.get(var_name) {
                                        arg_regs.push(meta.llvm_name.clone());

                                        if let Some(size) = meta.array_size {
                                            arg_types.push(format!("[{} x i64]*", size));
                                        } else {
                                            arg_types.push(format!(
                                                "{}*",
                                                self.type_to_llvm(&meta.var_type)
                                            ));
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
                                let arg_type = self.infer_llvm_type(arg_node);
                                arg_regs.push(reg);
                                arg_types.push(self.type_to_llvm(&arg_type).to_string());
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

                    let result = self.new_temp();
                    self.emit(&format!(
                        "  {} = call {} @{}({})",
                        result, return_type, name, args_str
                    ));
                    result
                }
            },

            AstNode::MethodCall {
                object,
                method,
                args,
            } => match method.as_str() {
                "len" => {
                    let obj_reg = self.gen_node(object);
                    let result = self.new_temp();
                    self.emit(&format!("  {} = call i64 @strlen(i8* {})", result, obj_reg));
                    result
                }
                "char_at" if !args.is_empty() => {
                    let obj_reg = self.gen_node(object);
                    let index_reg = self.gen_node(&args[0]);
                    let char_ptr = self.new_temp();
                    self.emit(&format!(
                        "  {} = getelementptr i8, i8* {}, i64 {}",
                        char_ptr, obj_reg, index_reg
                    ));
                    let result = self.new_temp();
                    self.emit(&format!("  {} = load i8, i8* {}", result, char_ptr));
                    let extended = self.new_temp();
                    self.emit(&format!("  {} = sext i8 {} to i64", extended, result));
                    extended
                }
                "push" if !args.is_empty() => "0".to_string(),
                _ => "0".to_string(),
            },

            _ => "0".to_string(),
        }
    }

    fn gen_function(
        &mut self,
        name: &str,
        params: &[Parameter],
        body: &AstNode,
        return_type: &Option<String>,
    ) -> String {
        self.current_function_vars.clear();
        self.temp_counter = 0;

        let ret_type = if name == "main" {
            "i32".to_string()
        } else if let Some(rt) = return_type {
            self.type_to_llvm(rt).to_string()
        } else {
            "void".to_string()
        };

        self.function_signatures
            .insert(name.to_string(), ret_type.clone());
        self.current_function_name = name.to_string();
        self.current_function_return_type = ret_type.clone();

        let param_list = if params.is_empty() {
            String::new()
        } else {
            params
                .iter()
                .map(|p| {
                    let param_type_str = if p.is_reference {
                        if p.param_type.starts_with('[') {
                            if let Some(size_str) = p.param_type.split(';').nth(1) {
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
                        } else {
                            format!("{}*", self.type_to_llvm(&p.param_type))
                        }
                    } else {
                        self.type_to_llvm(&p.param_type).to_string()
                    };

                    format!("{} %arg_{}", param_type_str, p.name)
                })
                .collect::<Vec<_>>()
                .join(", ")
        };

        self.emit(&format!(
            "\ndefine {} @{}({}) {{",
            ret_type, name, param_list
        ));
        self.emit("entry:");

        for param in params {
            if param.is_reference {
                let param_type_name = param.param_type.clone();

                let array_size = if param.param_type.starts_with('[') {
                    if let Some(size_str) = param.param_type.split(';').nth(1) {
                        size_str
                            .trim()
                            .trim_end_matches(']')
                            .trim()
                            .parse::<usize>()
                            .ok()
                    } else {
                        None
                    }
                } else {
                    None
                };

                self.current_function_vars.insert(
                    param.name.clone(),
                    VarMetadata {
                        llvm_name: format!("%arg_{}", param.name),
                        var_type: param_type_name,
                        is_heap: false,
                        array_size,
                        is_string_literal: false,
                    },
                );
            } else {
                let param_type_str = self.type_to_llvm(&param.param_type).to_string();
                let param_type_name = param.param_type.clone();

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
                        var_type: param_type_name,
                        is_heap: false,
                        array_size: None,
                        is_string_literal: false,
                    },
                );
            }
        }

        self.gen_node(body);

        if name == "main" {
            self.emit("  ret i32 0");
        } else if return_type.is_none() {
            self.emit("  ret void");
        }

        self.emit("}");
        String::new()
    }

    fn gen_string_concat(&mut self, left: &str, right: &str) -> String {
        let len1 = self.new_temp();
        let len2 = self.new_temp();
        self.emit(&format!("  {} = call i64 @strlen(i8* {})", len1, left));
        self.emit(&format!("  {} = call i64 @strlen(i8* {})", len2, right));

        let total = self.new_temp();
        let total_plus_one = self.new_temp();
        self.emit(&format!("  {} = add i64 {}, {}", total, len1, len2));
        self.emit(&format!("  {} = add i64 {}, 1", total_plus_one, total));

        let new_ptr = self.new_temp();
        self.emit(&format!(
            "  {} = call i8* @malloc(i64 {})",
            new_ptr, total_plus_one
        ));

        let temp1 = self.new_temp();
        self.emit(&format!(
            "  {} = call i8* @strcpy(i8* {}, i8* {})",
            temp1, new_ptr, left
        ));

        let offset_ptr = self.new_temp();
        self.emit(&format!(
            "  {} = getelementptr i8, i8* {}, i64 {}",
            offset_ptr, new_ptr, len1
        ));

        let temp2 = self.new_temp();
        self.emit(&format!(
            "  {} = call i8* @strcpy(i8* {}, i8* {})",
            temp2, offset_ptr, right
        ));

        new_ptr
    }

    fn infer_llvm_type(&self, node: &AstNode) -> String {
        match node {
            AstNode::Number(_) => "int".to_string(),
            AstNode::Boolean(_) => "bool".to_string(),
            AstNode::Character(_) => "char".to_string(),
            AstNode::StringLit(_) => "string".to_string(),
            AstNode::BinaryOp { left, .. } => self.infer_llvm_type(left),
            AstNode::Identifier { name, .. } => self
                .current_function_vars
                .get(name)
                .map(|m| m.var_type.clone())
                .unwrap_or_else(|| "int".to_string()),
            AstNode::ArrayLit(_) => "array".to_string(),
            AstNode::EnumValue { .. } => "enum".to_string(),
            AstNode::Call { name, .. } => match name.as_str() {
                "read_file" => "string".to_string(),
                "write_file" => "int".to_string(),
                "puts" => "int".to_string(),
                "print_int" => "int".to_string(),
                _ => "int".to_string(),
            },
            _ => "int".to_string(),
        }
    }

    fn type_to_llvm(&self, type_name: &str) -> &str {
        match type_name {
            "int" => "i64",
            "bool" => "i1",
            "char" => "i8",
            "string" => "i8*",
            "array" => "i64*",
            "enum" => "{ i32, i64 }*",
            _ => "i64",
        }
    }

    fn new_temp(&mut self) -> String {
        let temp = format!("%{}", self.temp_counter);
        self.temp_counter += 1;
        temp
    }

    fn new_label(&mut self, prefix: &str) -> String {
        let label = format!("{}{}", prefix, self.label_counter);
        self.label_counter += 1;
        label
    }

    fn new_string_literal(&mut self, value: &str) -> String {
        let id = format!(".str.{}", self.string_counter);
        self.string_counter += 1;
        self.string_literals.push((id.clone(), value.to_string()));
        id
    }

    fn emit(&mut self, line: &str) {
        self.output.push_str(line);
        self.output.push('\n');
    }

    fn escape_string(&self, s: &str) -> String {
        let mut escaped = String::new();
        for c in s.bytes() {
            match c {
                b'\n' => escaped.push_str("\\0A"),
                b'\r' => escaped.push_str("\\0D"),
                b'\t' => escaped.push_str("\\09"),
                b'\\' => escaped.push_str("\\5C"),
                b'\"' => escaped.push_str("\\22"),
                32..=126 => escaped.push(c as char),
                _ => escaped.push_str(&format!("\\{:02x}", c)),
            }
        }
        escaped
    }

    fn build_output(&self) -> String {
        format!(
            "target triple = \"{}\"\n\n{}",
            get_target_triple(),
            self.output
        )
    }
}

