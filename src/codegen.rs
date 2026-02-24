use crate::parser::{AstNode, BinOp, Parameter, Pattern};
use std::collections::HashMap;

pub struct CodeGenerator {
    output: String,
    struct_decls: Vec<String>,
    string_counter: usize,
    temp_counter: usize,
    label_counter: usize,
    string_literals: Vec<(String, String)>,
    current_function_vars: HashMap<String, VarMetadata>,
    loop_stack: Vec<LoopLabels>,
    enum_types: HashMap<String, Vec<String>>,
    struct_types: HashMap<String, Vec<(String, String)>>,
    block_terminated: bool,
    current_function_name: String,
    current_function_return_type: String,
    function_signatures: HashMap<String, String>,
    pure_functions: std::collections::HashSet<String>,
    non_escaping: std::collections::HashSet<String>,
    current_binding: Option<String>,
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

struct EscapeAnalysis {
    escaping: std::collections::HashSet<String>,
}

impl EscapeAnalysis {
    fn analyze(params: &[Parameter], body: &AstNode) -> std::collections::HashSet<String> {
        let mut ea = EscapeAnalysis {
            escaping: std::collections::HashSet::new(),
        };
        ea.visit_body(params, body);
        ea.escaping
    }

    fn visit_body(&mut self, params: &[Parameter], body: &AstNode) {
        // Params passed by value that are pointer types always escape
        // (they came from the caller). Ref params are fine — not our allocation.
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
            AstNode::Call { name, args } => {
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
            AstNode::Return(None)
            | AstNode::Break
            | AstNode::Continue
            | AstNode::Identifier { .. }
            | AstNode::Number(_)
            | AstNode::Boolean(_)
            | AstNode::StringLit(_)
            | AstNode::Character(_)
            | AstNode::ArrayAssignment { .. }
            | AstNode::FunctionDef { .. }
            | AstNode::StructDef { .. }
            | AstNode::EnumDef { .. }
            | AstNode::EnumValue { .. }
            | AstNode::ArrayType { .. }
            | AstNode::Import { .. } => {}
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

impl CodeGenerator {
    pub fn new() -> Self {
        CodeGenerator {
            output: String::new(),
            struct_decls: Vec::new(),
            string_counter: 0,
            temp_counter: 0,
            label_counter: 0,
            string_literals: Vec::new(),
            current_function_vars: HashMap::new(),
            loop_stack: Vec::new(),
            enum_types: HashMap::new(),
            struct_types: HashMap::new(),
            block_terminated: false,
            current_function_name: String::new(),
            current_function_return_type: String::new(),
            function_signatures: HashMap::new(),
            pure_functions: std::collections::HashSet::new(),
            non_escaping: std::collections::HashSet::new(),
            current_binding: None,
        }
    }

    pub fn generate(&mut self, ast: &AstNode) -> String {
        if let AstNode::Program(nodes) = ast {
            for node in nodes {
                if let AstNode::StructDef { name, fields } = node {
                    let field_info: Vec<(String, String)> = fields
                        .iter()
                        .map(|f| (f.name.clone(), f.field_type.clone()))
                        .collect();
                    self.struct_types.insert(name.clone(), field_info);
                }
            }
        }

        if let AstNode::Program(nodes) = ast {
            for node in nodes {
                if let AstNode::FunctionDef {
                    name, params, body, ..
                } = node
                {
                    if Self::infer_purity(params, body) {
                        self.pure_functions.insert(name.clone());
                    }
                }
            }
        }

        // Dead code elimination: only emit functions and top-level let bindings
        // that are reachable from main. Walk the call graph starting at "main",
        // collect every called name transitively, then skip anything not in
        // that set during codegen.
        let reachable = if let AstNode::Program(nodes) = ast {
            Self::collect_reachable(nodes)
        } else {
            std::collections::HashSet::new()
        };

        for (struct_name, fields) in &self.struct_types.clone() {
            let field_types: Vec<String> =
                fields.iter().map(|(_, ft)| self.type_to_llvm(ft)).collect();
            self.struct_decls.push(format!(
                "%{} = type {{ {} }}",
                struct_name,
                field_types.join(", ")
            ));
        }

        self.emit_header();

        if let AstNode::Program(nodes) = ast {
            for node in nodes {
                match node {
                    AstNode::FunctionDef { name, .. } => {
                        if reachable.contains(name.as_str()) {
                            self.gen_node(node);
                        }
                    }
                    AstNode::LetBinding { name, .. } => {
                        if reachable.contains(name.as_str()) {
                            self.gen_node(node);
                        }
                    }
                    _ => {
                        self.gen_node(node);
                    }
                }
            }
        }

        self.emit_footer();
        self.build_output()
    }

    fn collect_reachable(nodes: &[AstNode]) -> std::collections::HashSet<String> {
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

    fn collect_calls(node: &AstNode, queue: &mut Vec<String>) {
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
            AstNode::LetBinding { value, .. } => Self::collect_calls(value, queue),
            AstNode::Assignment { value, .. } => Self::collect_calls(value, queue),
            AstNode::ArrayAssignment { index, value, .. } => {
                Self::collect_calls(index, queue);
                Self::collect_calls(value, queue);
            }
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
            AstNode::Return(v) => {
                if let Some(n) = v {
                    Self::collect_calls(n, queue);
                }
            }
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

    fn emit_header(&mut self) {
        if cfg!(target_os = "windows") {
            // Windows: define everything in terms of kernel32 — no CRT needed
            self.emit("declare i8* @GetProcessHeap()");
            self.emit("declare i8* @HeapAlloc(i8*, i32, i64)");
            self.emit("declare i8* @HeapReAlloc(i8*, i32, i8*, i64)");
            self.emit("declare i32 @HeapFree(i8*, i32, i8*)");
            self.emit("declare i8* @GetStdHandle(i32)");
            self.emit("declare i32 @WriteFile(i8*, i8*, i32, i32*, i8*)");
            self.emit("declare i8* @CreateFileA(i8*, i32, i32, i8*, i32, i32, i8*)");
            self.emit("declare i32 @ReadFile(i8*, i8*, i32, i32*, i8*)");
            self.emit("declare i32 @CloseHandle(i8*)");
            self.emit("declare i32 @SetFilePointer(i8*, i32, i32*, i32)");
            self.emit("");

            self.emit("define i8* @malloc(i64 %size) {");
            self.emit("  %heap = call i8* @GetProcessHeap()");
            self.emit("  %ptr = call i8* @HeapAlloc(i8* %heap, i32 0, i64 %size)");
            self.emit("  ret i8* %ptr");
            self.emit("}");
            self.emit("");

            self.emit("define i8* @realloc(i8* %ptr, i64 %size) {");
            self.emit("  %heap = call i8* @GetProcessHeap()");
            self.emit("  %new = call i8* @HeapReAlloc(i8* %heap, i32 0, i8* %ptr, i64 %size)");
            self.emit("  ret i8* %new");
            self.emit("}");
            self.emit("");

            self.emit("define void @free(i8* %ptr) {");
            self.emit("  %heap = call i8* @GetProcessHeap()");
            self.emit("  call i32 @HeapFree(i8* %heap, i32 0, i8* %ptr)");
            self.emit("  ret void");
            self.emit("}");
            self.emit("");

            // strlen implemented in pure IR
            self.emit("define i64 @strlen(i8* %s) {");
            self.emit("sl_entry:");
            self.emit("  br label %sl_loop");
            self.emit("sl_loop:");
            self.emit("  %sl_i = phi i64 [ 0, %sl_entry ], [ %sl_next, %sl_loop ]");
            self.emit("  %sl_p = getelementptr i8, i8* %s, i64 %sl_i");
            self.emit("  %sl_c = load i8, i8* %sl_p");
            self.emit("  %sl_done = icmp eq i8 %sl_c, 0");
            self.emit("  %sl_next = add i64 %sl_i, 1");
            self.emit("  br i1 %sl_done, label %sl_exit, label %sl_loop");
            self.emit("sl_exit:");
            self.emit("  ret i64 %sl_i");
            self.emit("}");
            self.emit("");

            // strcmp in pure IR
            self.emit("define i32 @strcmp(i8* %a, i8* %b) {");
            self.emit("sc_entry:");
            self.emit("  br label %sc_loop");
            self.emit("sc_loop:");
            self.emit("  %sc_i = phi i64 [ 0, %sc_entry ], [ %sc_next, %sc_cont ]");
            self.emit("  %sc_pa = getelementptr i8, i8* %a, i64 %sc_i");
            self.emit("  %sc_pb = getelementptr i8, i8* %b, i64 %sc_i");
            self.emit("  %sc_ca = load i8, i8* %sc_pa");
            self.emit("  %sc_cb = load i8, i8* %sc_pb");
            self.emit("  %sc_za = icmp eq i8 %sc_ca, 0");
            self.emit("  %sc_zb = icmp eq i8 %sc_cb, 0");
            self.emit("  %sc_end = or i1 %sc_za, %sc_zb");
            self.emit("  br i1 %sc_end, label %sc_exit, label %sc_cont");
            self.emit("sc_cont:");
            self.emit("  %sc_eq = icmp eq i8 %sc_ca, %sc_cb");
            self.emit("  %sc_next = add i64 %sc_i, 1");
            self.emit("  br i1 %sc_eq, label %sc_loop, label %sc_diff");
            self.emit("sc_diff:");
            self.emit("  %sc_da = sext i8 %sc_ca to i32");
            self.emit("  %sc_db = sext i8 %sc_cb to i32");
            self.emit("  %sc_r = sub i32 %sc_da, %sc_db");
            self.emit("  ret i32 %sc_r");
            self.emit("sc_exit:");
            self.emit("  %sc_fa = sext i8 %sc_ca to i32");
            self.emit("  %sc_fb = sext i8 %sc_cb to i32");
            self.emit("  %sc_fr = sub i32 %sc_fa, %sc_fb");
            self.emit("  ret i32 %sc_fr");
            self.emit("}");
            self.emit("");

            // strcpy in pure IR
            self.emit("define i8* @strcpy(i8* %dst, i8* %src) {");
            self.emit("sy_entry:");
            self.emit("  br label %sy_loop");
            self.emit("sy_loop:");
            self.emit("  %sy_i = phi i64 [ 0, %sy_entry ], [ %sy_next, %sy_loop ]");
            self.emit("  %sy_ps = getelementptr i8, i8* %src, i64 %sy_i");
            self.emit("  %sy_pd = getelementptr i8, i8* %dst, i64 %sy_i");
            self.emit("  %sy_c = load i8, i8* %sy_ps");
            self.emit("  store i8 %sy_c, i8* %sy_pd");
            self.emit("  %sy_done = icmp eq i8 %sy_c, 0");
            self.emit("  %sy_next = add i64 %sy_i, 1");
            self.emit("  br i1 %sy_done, label %sy_exit, label %sy_loop");
            self.emit("sy_exit:");
            self.emit("  ret i8* %dst");
            self.emit("}");
            self.emit("");

            // puts via WriteFile to stdout handle (-11)
            self.emit("define i32 @puts(i8* %s) {");
            self.emit("  %pt_out = call i8* @GetStdHandle(i32 -11)");
            self.emit("  %pt_len64 = call i64 @strlen(i8* %s)");
            self.emit("  %pt_len32 = trunc i64 %pt_len64 to i32");
            self.emit("  %pt_written = alloca i32");
            self.emit("  store i32 0, i32* %pt_written");
            self.emit("  call i32 @WriteFile(i8* %pt_out, i8* %s, i32 %pt_len32, i32* %pt_written, i8* null)");
            self.emit("  %pt_nl = alloca i8");
            self.emit("  store i8 10, i8* %pt_nl");
            self.emit(
                "  call i32 @WriteFile(i8* %pt_out, i8* %pt_nl, i32 1, i32* %pt_written, i8* null)",
            );
            self.emit("  ret i32 0");
            self.emit("}");
            self.emit("");

            // fopen via CreateFileA
            self.emit("define i8* @fopen(i8* %filename, i8* %mode) {");
            self.emit("fo_entry:");
            self.emit("  %fo_mc = load i8, i8* %mode");
            self.emit("  %fo_isw = icmp eq i8 %fo_mc, 119");
            self.emit("  br i1 %fo_isw, label %fo_write, label %fo_read");
            self.emit("fo_write:");
            self.emit("  %fo_wh = call i8* @CreateFileA(i8* %filename, i32 1073741824, i32 0, i8* null, i32 2, i32 128, i8* null)");
            self.emit("  ret i8* %fo_wh");
            self.emit("fo_read:");
            self.emit("  %fo_rh = call i8* @CreateFileA(i8* %filename, i32 -2147483648, i32 1, i8* null, i32 3, i32 128, i8* null)");
            self.emit("  ret i8* %fo_rh");
            self.emit("}");
            self.emit("");

            self.emit("define i32 @fclose(i8* %handle) {");
            self.emit("  call i32 @CloseHandle(i8* %handle)");
            self.emit("  ret i32 0");
            self.emit("}");
            self.emit("");

            self.emit("define i64 @fread(i8* %buf, i64 %sz, i64 %count, i8* %handle) {");
            self.emit("  %fr_total = mul i64 %sz, %count");
            self.emit("  %fr_t32 = trunc i64 %fr_total to i32");
            self.emit("  %fr_read = alloca i32");
            self.emit("  store i32 0, i32* %fr_read");
            self.emit(
                "  call i32 @ReadFile(i8* %handle, i8* %buf, i32 %fr_t32, i32* %fr_read, i8* null)",
            );
            self.emit("  %fr_r32 = load i32, i32* %fr_read");
            self.emit("  %fr_r64 = sext i32 %fr_r32 to i64");
            self.emit("  ret i64 %fr_r64");
            self.emit("}");
            self.emit("");

            self.emit("define i64 @fwrite(i8* %buf, i64 %sz, i64 %count, i8* %handle) {");
            self.emit("  %fw_total = mul i64 %sz, %count");
            self.emit("  %fw_t32 = trunc i64 %fw_total to i32");
            self.emit("  %fw_written = alloca i32");
            self.emit("  store i32 0, i32* %fw_written");
            self.emit("  call i32 @WriteFile(i8* %handle, i8* %buf, i32 %fw_t32, i32* %fw_written, i8* null)");
            self.emit("  %fw_w32 = load i32, i32* %fw_written");
            self.emit("  %fw_w64 = sext i32 %fw_w32 to i64");
            self.emit("  ret i64 %fw_w64");
            self.emit("}");
            self.emit("");

            self.emit("define i32 @fseek(i8* %handle, i64 %offset, i32 %whence) {");
            self.emit("  %fsk_off32 = trunc i64 %offset to i32");
            self.emit(
                "  call i32 @SetFilePointer(i8* %handle, i32 %fsk_off32, i32* null, i32 %whence)",
            );
            self.emit("  ret i32 0");
            self.emit("}");
            self.emit("");

            self.emit("define i64 @ftell(i8* %handle) {");
            self.emit(
                "  %ft_pos32 = call i32 @SetFilePointer(i8* %handle, i32 0, i32* null, i32 1)",
            );
            self.emit("  %ft_pos64 = sext i32 %ft_pos32 to i64");
            self.emit("  ret i64 %ft_pos64");
            self.emit("}");
            self.emit("");
        } else {
            // Linux: raw syscalls — zero libc dependency
            // syscall(SYS_brk) based bump allocator
            self.emit("declare i64 @syscall(i64, ...)");
            self.emit("");

            // brk-based malloc: grow heap with SYS_brk (syscall 12 on x86-64)
            self.emit("@brn_heap_end = global i8* null");
            self.emit("@brn_heap_start = global i8* null");
            self.emit("");

            self.emit("define i8* @malloc(i64 %size) {");
            self.emit("  %cur = load i8*, i8** @brn_heap_end");
            self.emit("  %is_null = icmp eq i8* %cur, null");
            self.emit("  br i1 %is_null, label %init, label %alloc");
            self.emit("init:");
            // SYS_brk(0) returns current brk
            self.emit("  %brk0 = call i64 (i64, ...) @syscall(i64 12, i64 0)");
            self.emit("  %start = inttoptr i64 %brk0 to i8*");
            self.emit("  store i8* %start, i8** @brn_heap_start");
            self.emit("  store i8* %start, i8** @brn_heap_end");
            self.emit("  br label %alloc");
            self.emit("alloc:");
            self.emit("  %base = load i8*, i8** @brn_heap_end");
            self.emit("  %base_i = ptrtoint i8* %base to i64");
            // align to 8 bytes
            self.emit("  %align7 = add i64 %size, 7");
            self.emit("  %aligned = and i64 %align7, -8");
            self.emit("  %new_end_i = add i64 %base_i, %aligned");
            self.emit("  %new_end = inttoptr i64 %new_end_i to i8*");
            // SYS_brk(new_end) to extend heap
            self.emit("  call i64 (i64, ...) @syscall(i64 12, i64 %new_end_i)");
            self.emit("  store i8* %new_end, i8** @brn_heap_end");
            self.emit("  ret i8* %base");
            self.emit("}");
            self.emit("");

            // realloc: alloc new, copy, return (bump allocator — no free)
            self.emit("define i8* @realloc(i8* %ptr, i64 %size) {");
            self.emit("  %new = call i8* @malloc(i64 %size)");
            // copy old data (best-effort, copy %size bytes from old ptr)
            self.emit("  br label %rc_loop");
            self.emit("rc_loop:");
            self.emit("  %rc_i = phi i64 [ 0, %0 ], [ %rc_next, %rc_loop ]");
            self.emit("  %rc_done = icmp eq i64 %rc_i, %size");
            self.emit("  br i1 %rc_done, label %rc_exit, label %rc_copy");
            self.emit("rc_copy:");
            self.emit("  %rc_sp = getelementptr i8, i8* %ptr, i64 %rc_i");
            self.emit("  %rc_dp = getelementptr i8, i8* %new, i64 %rc_i");
            self.emit("  %rc_byte = load i8, i8* %rc_sp");
            self.emit("  store i8 %rc_byte, i8* %rc_dp");
            self.emit("  %rc_next = add i64 %rc_i, 1");
            self.emit("  br label %rc_loop");
            self.emit("rc_exit:");
            self.emit("  ret i8* %new");
            self.emit("}");
            self.emit("");

            // free: no-op with bump allocator
            self.emit("define void @free(i8* %ptr) {");
            self.emit("  ret void");
            self.emit("}");
            self.emit("");

            // strlen — pure IR
            self.emit("define i64 @strlen(i8* %s) {");
            self.emit("sl_entry:");
            self.emit("  br label %sl_loop");
            self.emit("sl_loop:");
            self.emit("  %sl_i = phi i64 [ 0, %sl_entry ], [ %sl_next, %sl_loop ]");
            self.emit("  %sl_p = getelementptr i8, i8* %s, i64 %sl_i");
            self.emit("  %sl_c = load i8, i8* %sl_p");
            self.emit("  %sl_done = icmp eq i8 %sl_c, 0");
            self.emit("  %sl_next = add i64 %sl_i, 1");
            self.emit("  br i1 %sl_done, label %sl_exit, label %sl_loop");
            self.emit("sl_exit:");
            self.emit("  ret i64 %sl_i");
            self.emit("}");
            self.emit("");

            // strcmp — pure IR
            self.emit("define i32 @strcmp(i8* %a, i8* %b) {");
            self.emit("sc_entry:");
            self.emit("  br label %sc_loop");
            self.emit("sc_loop:");
            self.emit("  %sc_i = phi i64 [ 0, %sc_entry ], [ %sc_next, %sc_cont ]");
            self.emit("  %sc_pa = getelementptr i8, i8* %a, i64 %sc_i");
            self.emit("  %sc_pb = getelementptr i8, i8* %b, i64 %sc_i");
            self.emit("  %sc_ca = load i8, i8* %sc_pa");
            self.emit("  %sc_cb = load i8, i8* %sc_pb");
            self.emit("  %sc_za = icmp eq i8 %sc_ca, 0");
            self.emit("  %sc_zb = icmp eq i8 %sc_cb, 0");
            self.emit("  %sc_end = or i1 %sc_za, %sc_zb");
            self.emit("  br i1 %sc_end, label %sc_exit, label %sc_cont");
            self.emit("sc_cont:");
            self.emit("  %sc_eq = icmp eq i8 %sc_ca, %sc_cb");
            self.emit("  %sc_next = add i64 %sc_i, 1");
            self.emit("  br i1 %sc_eq, label %sc_loop, label %sc_diff");
            self.emit("sc_diff:");
            self.emit("  %sc_da = sext i8 %sc_ca to i32");
            self.emit("  %sc_db = sext i8 %sc_cb to i32");
            self.emit("  %sc_r = sub i32 %sc_da, %sc_db");
            self.emit("  ret i32 %sc_r");
            self.emit("sc_exit:");
            self.emit("  %sc_fa = sext i8 %sc_ca to i32");
            self.emit("  %sc_fb = sext i8 %sc_cb to i32");
            self.emit("  %sc_fr = sub i32 %sc_fa, %sc_fb");
            self.emit("  ret i32 %sc_fr");
            self.emit("}");
            self.emit("");

            // strcpy — pure IR
            self.emit("define i8* @strcpy(i8* %dst, i8* %src) {");
            self.emit("sy_entry:");
            self.emit("  br label %sy_loop");
            self.emit("sy_loop:");
            self.emit("  %sy_i = phi i64 [ 0, %sy_entry ], [ %sy_next, %sy_loop ]");
            self.emit("  %sy_ps = getelementptr i8, i8* %src, i64 %sy_i");
            self.emit("  %sy_pd = getelementptr i8, i8* %dst, i64 %sy_i");
            self.emit("  %sy_c = load i8, i8* %sy_ps");
            self.emit("  store i8 %sy_c, i8* %sy_pd");
            self.emit("  %sy_done = icmp eq i8 %sy_c, 0");
            self.emit("  %sy_next = add i64 %sy_i, 1");
            self.emit("  br i1 %sy_done, label %sy_exit, label %sy_loop");
            self.emit("sy_exit:");
            self.emit("  ret i8* %dst");
            self.emit("}");
            self.emit("");

            // puts via SYS_write(1, buf, len) + newline — syscall 1 on x86-64
            self.emit("define i32 @puts(i8* %s) {");
            self.emit("  %pt_len = call i64 @strlen(i8* %s)");
            self.emit("  call i64 (i64, ...) @syscall(i64 1, i64 1, i8* %s, i64 %pt_len)");
            self.emit("  %pt_nl = alloca i8");
            self.emit("  store i8 10, i8* %pt_nl");
            self.emit("  call i64 (i64, ...) @syscall(i64 1, i64 1, i8* %pt_nl, i64 1)");
            self.emit("  ret i32 0");
            self.emit("}");
            self.emit("");

            // fopen via SYS_open (syscall 2) / SYS_creat style
            self.emit("define i8* @fopen(i8* %filename, i8* %mode) {");
            self.emit("fo_entry:");
            self.emit("  %fo_mc = load i8, i8* %mode");
            self.emit("  %fo_isw = icmp eq i8 %fo_mc, 119");
            self.emit("  br i1 %fo_isw, label %fo_write, label %fo_read");
            // O_WRONLY|O_CREAT|O_TRUNC = 577, mode 0644
            self.emit("fo_write:");
            self.emit(
                "  %fo_wfd = call i64 (i64, ...) @syscall(i64 2, i8* %filename, i64 577, i64 420)",
            );
            self.emit("  %fo_wh = inttoptr i64 %fo_wfd to i8*");
            self.emit("  ret i8* %fo_wh");
            // O_RDONLY = 0
            self.emit("fo_read:");
            self.emit(
                "  %fo_rfd = call i64 (i64, ...) @syscall(i64 2, i8* %filename, i64 0, i64 0)",
            );
            self.emit("  %fo_rh = inttoptr i64 %fo_rfd to i8*");
            self.emit("  ret i8* %fo_rh");
            self.emit("}");
            self.emit("");

            // fclose via SYS_close (syscall 3)
            self.emit("define i32 @fclose(i8* %handle) {");
            self.emit("  %fc_fd = ptrtoint i8* %handle to i64");
            self.emit("  call i64 (i64, ...) @syscall(i64 3, i64 %fc_fd)");
            self.emit("  ret i32 0");
            self.emit("}");
            self.emit("");

            // fread via SYS_read (syscall 0)
            self.emit("define i64 @fread(i8* %buf, i64 %sz, i64 %count, i8* %handle) {");
            self.emit("  %fr_fd = ptrtoint i8* %handle to i64");
            self.emit("  %fr_total = mul i64 %sz, %count");
            self.emit("  %fr_n = call i64 (i64, ...) @syscall(i64 0, i64 %fr_fd, i8* %buf, i64 %fr_total)");
            self.emit("  ret i64 %fr_n");
            self.emit("}");
            self.emit("");

            // fwrite via SYS_write (syscall 1)
            self.emit("define i64 @fwrite(i8* %buf, i64 %sz, i64 %count, i8* %handle) {");
            self.emit("  %fw_fd = ptrtoint i8* %handle to i64");
            self.emit("  %fw_total = mul i64 %sz, %count");
            self.emit("  %fw_n = call i64 (i64, ...) @syscall(i64 1, i64 %fw_fd, i8* %buf, i64 %fw_total)");
            self.emit("  ret i64 %fw_n");
            self.emit("}");
            self.emit("");

            // fseek via SYS_lseek (syscall 8)
            self.emit("define i32 @fseek(i8* %handle, i64 %offset, i32 %whence) {");
            self.emit("  %fsk_fd = ptrtoint i8* %handle to i64");
            self.emit("  %fsk_wh = sext i32 %whence to i64");
            self.emit(
                "  call i64 (i64, ...) @syscall(i64 8, i64 %fsk_fd, i64 %offset, i64 %fsk_wh)",
            );
            self.emit("  ret i32 0");
            self.emit("}");
            self.emit("");

            // ftell via SYS_lseek(fd, 0, SEEK_CUR=1)
            self.emit("define i64 @ftell(i8* %handle) {");
            self.emit("  %ft_fd = ptrtoint i8* %handle to i64");
            self.emit("  %ft_pos = call i64 (i64, ...) @syscall(i64 8, i64 %ft_fd, i64 0, i64 1)");
            self.emit("  ret i64 %ft_pos");
            self.emit("}");
            self.emit("");
        }

        // int_to_string: pure IR digit extraction, no sprintf needed
        self.emit("define i8* @int_to_string_stack(i64 %n, i8* %buf) {");
        self.emit("its2_entry:");
        self.emit("  %its2_iszero = icmp eq i64 %n, 0");
        self.emit("  br i1 %its2_iszero, label %its2_zero, label %its2_nonzero");
        self.emit("its2_zero:");
        self.emit("  %its2_zp = getelementptr i8, i8* %buf, i64 30");
        self.emit("  store i8 48, i8* %its2_zp");
        self.emit("  %its2_zt = getelementptr i8, i8* %buf, i64 31");
        self.emit("  store i8 0, i8* %its2_zt");
        self.emit("  ret i8* %its2_zp");
        self.emit("its2_nonzero:");
        self.emit("  %its2_isneg = icmp slt i64 %n, 0");
        self.emit("  %its2_neg = sub i64 0, %n");
        self.emit("  %its2_abs = select i1 %its2_isneg, i64 %its2_neg, i64 %n");
        self.emit("  %its2_term = getelementptr i8, i8* %buf, i64 31");
        self.emit("  store i8 0, i8* %its2_term");
        self.emit("  br label %its2_loop");
        self.emit("its2_loop:");
        self.emit("  %its2_cur = phi i64 [ %its2_abs, %its2_nonzero ], [ %its2_quot, %its2_loop ]");
        self.emit("  %its2_pos = phi i64 [ 30, %its2_nonzero ], [ %its2_prev, %its2_loop ]");
        self.emit("  %its2_rem = srem i64 %its2_cur, 10");
        self.emit("  %its2_quot = sdiv i64 %its2_cur, 10");
        self.emit("  %its2_ascii = add i64 %its2_rem, 48");
        self.emit("  %its2_ch = trunc i64 %its2_ascii to i8");
        self.emit("  %its2_wp = getelementptr i8, i8* %buf, i64 %its2_pos");
        self.emit("  store i8 %its2_ch, i8* %its2_wp");
        self.emit("  %its2_prev = sub i64 %its2_pos, 1");
        self.emit("  %its2_done = icmp eq i64 %its2_quot, 0");
        self.emit("  br i1 %its2_done, label %its2_finish, label %its2_loop");
        self.emit("its2_finish:");
        self.emit("  br i1 %its2_isneg, label %its2_addneg, label %its2_ret");
        self.emit("its2_addneg:");
        self.emit("  %its2_np = getelementptr i8, i8* %buf, i64 %its2_prev");
        self.emit("  store i8 45, i8* %its2_np");
        self.emit("  ret i8* %its2_np");
        self.emit("its2_ret:");
        self.emit("  %its2_rp = getelementptr i8, i8* %buf, i64 %its2_pos");
        self.emit("  ret i8* %its2_rp");
        self.emit("}");
        self.emit("");

        self.emit("define i8* @int_to_string_impl(i64 %n) {");
        self.emit("its_entry:");
        self.emit("  %its_buf = call i8* @malloc(i64 32)");
        self.emit("  %its_iszero = icmp eq i64 %n, 0");
        self.emit("  br i1 %its_iszero, label %its_zero, label %its_nonzero");
        self.emit("its_zero:");
        self.emit("  %its_zp = getelementptr i8, i8* %its_buf, i64 30");
        self.emit("  store i8 48, i8* %its_zp");
        self.emit("  %its_term = getelementptr i8, i8* %its_buf, i64 31");
        self.emit("  store i8 0, i8* %its_term");
        self.emit("  ret i8* %its_zp");
        self.emit("its_nonzero:");
        self.emit("  %its_isneg = icmp slt i64 %n, 0");
        self.emit("  %its_neg = sub i64 0, %n");
        self.emit("  %its_abs = select i1 %its_isneg, i64 %its_neg, i64 %n");
        self.emit("  %its_term2 = getelementptr i8, i8* %its_buf, i64 31");
        self.emit("  store i8 0, i8* %its_term2");
        self.emit("  br label %its_loop");
        self.emit("its_loop:");
        self.emit("  %its_cur = phi i64 [ %its_abs, %its_nonzero ], [ %its_quot, %its_loop ]");
        self.emit("  %its_pos = phi i64 [ 30, %its_nonzero ], [ %its_prev, %its_loop ]");
        self.emit("  %its_rem = srem i64 %its_cur, 10");
        self.emit("  %its_quot = sdiv i64 %its_cur, 10");
        self.emit("  %its_ascii = add i64 %its_rem, 48");
        self.emit("  %its_ch = trunc i64 %its_ascii to i8");
        self.emit("  %its_wp = getelementptr i8, i8* %its_buf, i64 %its_pos");
        self.emit("  store i8 %its_ch, i8* %its_wp");
        self.emit("  %its_prev = sub i64 %its_pos, 1");
        self.emit("  %its_done = icmp eq i64 %its_quot, 0");
        self.emit("  br i1 %its_done, label %its_finish, label %its_loop");
        self.emit("its_finish:");
        self.emit("  br i1 %its_isneg, label %its_addneg, label %its_ret");
        self.emit("its_addneg:");
        self.emit("  %its_np = getelementptr i8, i8* %its_buf, i64 %its_prev");
        self.emit("  store i8 45, i8* %its_np");
        self.emit("  ret i8* %its_np");
        self.emit("its_ret:");
        self.emit("  %its_rp = getelementptr i8, i8* %its_buf, i64 %its_pos");
        self.emit("  ret i8* %its_rp");
        self.emit("}");
        self.emit("");

        // brn_print_int: on Windows uses WriteFile, on Unix uses puts
        if cfg!(target_os = "windows") {
            self.emit("define void @brn_print_int(i64 %n) {");
            self.emit("  %bpi_buf = alloca [32 x i8]");
            self.emit(
                "  %bpi_buf_ptr = getelementptr [32 x i8], [32 x i8]* %bpi_buf, i64 0, i64 0",
            );
            self.emit("  %bpi_str = call i8* @int_to_string_stack(i64 %n, i8* %bpi_buf_ptr)");
            self.emit("  %bpi_out = call i8* @GetStdHandle(i32 -11)");
            self.emit("  %bpi_len64 = call i64 @strlen(i8* %bpi_str)");
            self.emit("  %bpi_len32 = trunc i64 %bpi_len64 to i32");
            self.emit("  %bpi_written = alloca i32");
            self.emit("  store i32 0, i32* %bpi_written");
            self.emit("  call i32 @WriteFile(i8* %bpi_out, i8* %bpi_str, i32 %bpi_len32, i32* %bpi_written, i8* null)");
            self.emit("  %bpi_nl = alloca i8");
            self.emit("  store i8 10, i8* %bpi_nl");
            self.emit("  call i32 @WriteFile(i8* %bpi_out, i8* %bpi_nl, i32 1, i32* %bpi_written, i8* null)");
            self.emit("  ret void");
            self.emit("}");
        } else {
            // Linux: SYS_write directly — no libc
            self.emit("define void @brn_print_int(i64 %n) {");
            self.emit("  %bpi_str = call i8* @int_to_string_impl(i64 %n)");
            self.emit("  %bpi_len = call i64 @strlen(i8* %bpi_str)");
            self.emit("  call i64 (i64, ...) @syscall(i64 1, i64 1, i8* %bpi_str, i64 %bpi_len)");
            self.emit("  %bpi_nl = alloca i8");
            self.emit("  store i8 10, i8* %bpi_nl");
            self.emit("  call i64 (i64, ...) @syscall(i64 1, i64 1, i8* %bpi_nl, i64 1)");
            self.emit("  ret void");
            self.emit("}");
        }
        self.emit("");

        // Shared: file I/O helpers, vec helpers
        self.emit("define i8* @read_file_impl(i8* %filename) {");
        self.emit(
            "  %rf_mode = getelementptr inbounds [2 x i8], [2 x i8]* @.str.mode.r, i64 0, i64 0",
        );
        self.emit("  %rf_file = call i8* @fopen(i8* %filename, i8* %rf_mode)");
        self.emit("  %rf_null = icmp eq i8* %rf_file, null");
        self.emit("  br i1 %rf_null, label %rf_error, label %rf_read");
        self.emit("rf_error:");
        self.emit("  ret i8* null");
        self.emit("rf_read:");
        self.emit("  call i32 @fseek(i8* %rf_file, i64 0, i32 2)");
        self.emit("  %rf_size = call i64 @ftell(i8* %rf_file)");
        self.emit("  call i32 @fseek(i8* %rf_file, i64 0, i32 0)");
        self.emit("  %rf_sz1 = add i64 %rf_size, 1");
        self.emit("  %rf_buf = call i8* @malloc(i64 %rf_sz1)");
        self.emit("  call i64 @fread(i8* %rf_buf, i64 1, i64 %rf_size, i8* %rf_file)");
        self.emit("  %rf_np = getelementptr i8, i8* %rf_buf, i64 %rf_size");
        self.emit("  store i8 0, i8* %rf_np");
        self.emit("  call i32 @fclose(i8* %rf_file)");
        self.emit("  ret i8* %rf_buf");
        self.emit("}");
        self.emit("");

        self.emit("define i32 @write_file_impl(i8* %filename, i8* %content) {");
        self.emit(
            "  %wf_mode = getelementptr inbounds [2 x i8], [2 x i8]* @.str.mode.w, i64 0, i64 0",
        );
        self.emit("  %wf_file = call i8* @fopen(i8* %filename, i8* %wf_mode)");
        self.emit("  %wf_null = icmp eq i8* %wf_file, null");
        self.emit("  br i1 %wf_null, label %wf_error, label %wf_write");
        self.emit("wf_error:");
        self.emit("  ret i32 0");
        self.emit("wf_write:");
        self.emit("  %wf_len = call i64 @strlen(i8* %content)");
        self.emit("  call i64 @fwrite(i8* %content, i64 1, i64 %wf_len, i8* %wf_file)");
        self.emit("  call i32 @fclose(i8* %wf_file)");
        self.emit("  ret i32 1");
        self.emit("}");
        self.emit("");

        self.emit("define i8* @vec_new_impl() {");
        self.emit("  %vn_hdr = call i8* @malloc(i64 24)");
        self.emit("  %vn_lp = bitcast i8* %vn_hdr to i64*");
        self.emit("  store i64 0, i64* %vn_lp");
        self.emit("  %vn_cp_raw = getelementptr i8, i8* %vn_hdr, i64 8");
        self.emit("  %vn_cp = bitcast i8* %vn_cp_raw to i64*");
        self.emit("  store i64 4, i64* %vn_cp");
        self.emit("  %vn_buf = call i8* @malloc(i64 32)");
        self.emit("  %vn_dp_raw = getelementptr i8, i8* %vn_hdr, i64 16");
        self.emit("  %vn_dp = bitcast i8* %vn_dp_raw to i8**");
        self.emit("  store i8* %vn_buf, i8** %vn_dp");
        self.emit("  ret i8* %vn_hdr");
        self.emit("}");
        self.emit("");

        self.emit("define void @vec_push_impl(i8* %vec, i64 %val) {");
        self.emit("  %vp_lp = bitcast i8* %vec to i64*");
        self.emit("  %vp_len = load i64, i64* %vp_lp");
        self.emit("  %vp_cp_raw = getelementptr i8, i8* %vec, i64 8");
        self.emit("  %vp_cap_ptr = bitcast i8* %vp_cp_raw to i64*");
        self.emit("  %vp_cap = load i64, i64* %vp_cap_ptr");
        self.emit("  %vp_need = icmp eq i64 %vp_len, %vp_cap");
        self.emit("  br i1 %vp_need, label %vp_grow, label %vp_store");
        self.emit("vp_grow:");
        self.emit("  %vp_nc = mul i64 %vp_cap, 2");
        self.emit("  %vp_nb = mul i64 %vp_nc, 8");
        self.emit("  %vp_dpp_raw = getelementptr i8, i8* %vec, i64 16");
        self.emit("  %vp_dpp = bitcast i8* %vp_dpp_raw to i8**");
        self.emit("  %vp_old = load i8*, i8** %vp_dpp");
        self.emit("  %vp_new = call i8* @realloc(i8* %vp_old, i64 %vp_nb)");
        self.emit("  store i8* %vp_new, i8** %vp_dpp");
        self.emit("  store i64 %vp_nc, i64* %vp_cap_ptr");
        self.emit("  br label %vp_store");
        self.emit("vp_store:");
        self.emit("  %vp_dp2_raw = getelementptr i8, i8* %vec, i64 16");
        self.emit("  %vp_dp2 = bitcast i8* %vp_dp2_raw to i8**");
        self.emit("  %vp_data = load i8*, i8** %vp_dp2");
        self.emit("  %vp_di64 = bitcast i8* %vp_data to i64*");
        self.emit("  %vp_elem = getelementptr i64, i64* %vp_di64, i64 %vp_len");
        self.emit("  store i64 %val, i64* %vp_elem");
        self.emit("  %vp_nl = add i64 %vp_len, 1");
        self.emit("  store i64 %vp_nl, i64* %vp_lp");
        self.emit("  ret void");
        self.emit("}");
        self.emit("");

        self.emit("define i64 @vec_get_impl(i8* %vec, i64 %idx) {");
        self.emit("  %vg_dp_raw = getelementptr i8, i8* %vec, i64 16");
        self.emit("  %vg_dp = bitcast i8* %vg_dp_raw to i8**");
        self.emit("  %vg_data = load i8*, i8** %vg_dp");
        self.emit("  %vg_di64 = bitcast i8* %vg_data to i64*");
        self.emit("  %vg_ep = getelementptr i64, i64* %vg_di64, i64 %idx");
        self.emit("  %vg_val = load i64, i64* %vg_ep");
        self.emit("  ret i64 %vg_val");
        self.emit("}");
        self.emit("");

        self.emit("define void @vec_set_impl(i8* %vec, i64 %idx, i64 %val) {");
        self.emit("  %vs_dp_raw = getelementptr i8, i8* %vec, i64 16");
        self.emit("  %vs_dp = bitcast i8* %vs_dp_raw to i8**");
        self.emit("  %vs_data = load i8*, i8** %vs_dp");
        self.emit("  %vs_di64 = bitcast i8* %vs_data to i64*");
        self.emit("  %vs_ep = getelementptr i64, i64* %vs_di64, i64 %idx");
        self.emit("  store i64 %val, i64* %vs_ep");
        self.emit("  ret void");
        self.emit("}");
        self.emit("");

        self.emit("define i64 @vec_len_impl(i8* %vec) {");
        self.emit("  %vl_lp = bitcast i8* %vec to i64*");
        self.emit("  %vl_len = load i64, i64* %vl_lp");
        self.emit("  ret i64 %vl_len");
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
        for decl in self.struct_decls.clone().iter().rev() {
            self.output = format!("{}\n{}", decl, self.output);
        }
    }

    fn gen_node(&mut self, node: &AstNode) -> String {
        match node {
            AstNode::Import { .. } => "0".to_string(),

            AstNode::StructDef { .. } => "0".to_string(),

            AstNode::StructInit { name, fields } => {
                let struct_fields = self.struct_types.get(name).cloned().unwrap_or_default();
                let num_fields = struct_fields.len();

                let stack_promote = self
                    .current_binding
                    .as_ref()
                    .map(|b| self.non_escaping.contains(b))
                    .unwrap_or(false);

                let struct_ptr = self.new_temp();
                if stack_promote {
                    self.emit(&format!("  {} = alloca %{}", struct_ptr, name));
                } else {
                    let size = (num_fields as i64) * 8;
                    let raw_ptr = self.new_temp();
                    self.emit(&format!("  {} = call i8* @malloc(i64 {})", raw_ptr, size));
                    self.emit(&format!(
                        "  {} = bitcast i8* {} to %{}*",
                        struct_ptr, raw_ptr, name
                    ));
                }

                for (field_name, field_value) in fields.iter() {
                    let val_reg = self.gen_node(field_value);
                    let field_idx = struct_fields
                        .iter()
                        .position(|(n, _)| n == field_name)
                        .unwrap_or(0);
                    let field_type = struct_fields
                        .get(field_idx)
                        .map(|(_, t)| t.clone())
                        .unwrap_or_else(|| "int".to_string());
                    let llvm_field_type = self.type_to_llvm(&field_type);

                    let gep = self.new_temp();
                    self.emit(&format!(
                        "  {} = getelementptr %{}, %{}* {}, i32 0, i32 {}",
                        gep, name, name, struct_ptr, field_idx
                    ));
                    self.emit(&format!(
                        "  store {} {}, {}* {}",
                        llvm_field_type, val_reg, llvm_field_type, gep
                    ));
                }

                struct_ptr
            }

            AstNode::MemberAccess { object, field } => {
                let obj_reg = self.gen_node(object);
                let struct_name = self.infer_struct_name(object);

                if let Some(struct_fields) = self.struct_types.get(&struct_name).cloned() {
                    if let Some(field_idx) = struct_fields.iter().position(|(n, _)| n == field) {
                        let field_type = struct_fields[field_idx].1.clone();
                        let llvm_field_type = self.type_to_llvm(&field_type);

                        let gep = self.new_temp();
                        self.emit(&format!(
                            "  {} = getelementptr %{}, %{}* {}, i32 0, i32 {}",
                            gep, struct_name, struct_name, obj_reg, field_idx
                        ));
                        let result = self.new_temp();
                        self.emit(&format!(
                            "  {} = load {}, {}* {}",
                            result, llvm_field_type, llvm_field_type, gep
                        ));
                        return result;
                    }
                }
                "0".to_string()
            }

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

                let is_enum_match = arms
                    .iter()
                    .any(|a| matches!(a.pattern, Pattern::EnumPattern { .. }));

                if is_enum_match {
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
                                    .and_then(|variants| variants.iter().position(|v| v == variant))
                                    .unwrap_or(i)
                                    as i32;

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

                                self.block_terminated = false;
                                self.gen_node(&arm.body);
                                if !self.block_terminated {
                                    self.emit(&format!("  br label %{}", end_label));
                                }
                            }
                            Pattern::Wildcard | Pattern::Identifier(_) => {
                                self.emit(&format!("  br label %{}", arm_label));
                                self.emit(&format!("{}:", arm_label));
                                self.block_terminated = false;
                                self.gen_node(&arm.body);
                                if !self.block_terminated {
                                    self.emit(&format!("  br label %{}", end_label));
                                }
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
                                self.emit(&format!(
                                    "  {} = icmp eq i64 {}, {}",
                                    cond, value_reg, n
                                ));
                                self.emit(&format!(
                                    "  br i1 {}, label %{}, label %{}",
                                    cond, arm_label, next_label
                                ));
                                self.emit(&format!("{}:", arm_label));
                                self.block_terminated = false;
                                self.gen_node(&arm.body);
                                if !self.block_terminated {
                                    self.emit(&format!("  br label %{}", end_label));
                                }
                            }
                            Pattern::StringPattern(s) => {
                                let str_id = self.new_string_literal(s);
                                let str_len = s.len() + 1;
                                let str_ptr = self.new_temp();
                                self.emit(&format!(
                                    "  {} = getelementptr inbounds [{} x i8], [{} x i8]* @{}, i64 0, i64 0",
                                    str_ptr, str_len, str_len, str_id
                                ));
                                let cmp_result = self.new_temp();
                                self.emit(&format!(
                                    "  {} = call i32 @strcmp(i8* {}, i8* {})",
                                    cmp_result, value_reg, str_ptr
                                ));
                                let cond = self.new_temp();
                                self.emit(&format!("  {} = icmp eq i32 {}, 0", cond, cmp_result));
                                self.emit(&format!(
                                    "  br i1 {}, label %{}, label %{}",
                                    cond, arm_label, next_label
                                ));
                                self.emit(&format!("{}:", arm_label));
                                self.block_terminated = false;
                                self.gen_node(&arm.body);
                                if !self.block_terminated {
                                    self.emit(&format!("  br label %{}", end_label));
                                }
                            }
                            Pattern::Wildcard | Pattern::Identifier(_) => {
                                self.emit(&format!("  br label %{}", arm_label));
                                self.emit(&format!("{}:", arm_label));
                                self.block_terminated = false;
                                self.gen_node(&arm.body);
                                if !self.block_terminated {
                                    self.emit(&format!("  br label %{}", end_label));
                                }
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

            AstNode::FunctionDef {
                name,
                params,
                body,
                return_type,
                ..
            } => self.gen_function(name, params, body, return_type),

            AstNode::LetBinding { name, value, .. } => {
                self.current_binding = Some(name.clone());
                let value_reg = self.gen_node(value);
                self.current_binding = None;
                let var_type = self.infer_type(value);

                let is_string_literal = matches!(value.as_ref(), AstNode::StringLit(_));
                let is_struct = self.struct_types.contains_key(&var_type);
                let stack_promote = self.non_escaping.contains(name);

                // A value is heap-tracked only when it actually lives on the heap
                // AND it isn't being stack-promoted by escape analysis.
                let is_heap = !stack_promote
                    && ((var_type == "string" && !is_string_literal)
                        || (var_type == "Vec")
                        || is_struct);

                if let AstNode::ArrayLit(elements) = value.as_ref() {
                    let size = elements.len();
                    let sized_type = format!("[{}; int]", size);
                    self.current_function_vars.insert(
                        name.clone(),
                        VarMetadata {
                            llvm_name: value_reg.clone(),
                            var_type: sized_type,
                            is_heap: false,
                            array_size: Some(size),
                            is_string_literal: false,
                        },
                    );
                    return value_reg;
                }

                let ptr = self.new_temp();
                let llvm_type_str = self.type_to_llvm(&var_type);
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
                        array_size: None,
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
                    let llvm_type_str = self.type_to_llvm(&meta.var_type);
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
                    let ret_type = self.current_function_return_type.clone();
                    self.emit(&format!("  ret {} {}", ret_type, value_reg));
                } else if self.current_function_return_type == "void" {
                    self.emit("  ret void");
                } else {
                    self.emit("  ret i64 0");
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
                    .map(|(_, meta)| (meta.llvm_name.clone(), meta.var_type.clone()))
                    .collect();

                if !self.block_terminated {
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

                last_reg
            }

            AstNode::ExpressionStatement(expr) => self.gen_node(expr),

            AstNode::BinaryOp { op, left, right } => {
                let left_reg = self.gen_node(left);
                let right_reg = self.gen_node(right);

                match op {
                    BinOp::DotDot => right_reg,
                    BinOp::Add => {
                        if self.infer_type(left) == "string" {
                            let result = self.gen_string_concat(&left_reg, &right_reg);
                            let free_if_owned = |cg: &mut CodeGenerator, node: &AstNode| {
                                if let AstNode::Identifier { name, .. } = node {
                                    if let Some(meta) = cg.current_function_vars.get(name).cloned()
                                    {
                                        if !meta.is_string_literal {
                                            let loaded = cg.new_temp();
                                            cg.emit(&format!(
                                                "  {} = load i8*, i8** {}",
                                                loaded, meta.llvm_name
                                            ));
                                            cg.emit(&format!("  call void @free(i8* {})", loaded));
                                        }
                                    }
                                }
                            };
                            free_if_owned(self, right);
                            free_if_owned(self, left);
                            result
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
                        if self.infer_type(left) == "string" {
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
                        if self.infer_type(left) == "string" {
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
                    let llvm_type_str = self.type_to_llvm(&meta.var_type);
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
                        // Arrays and array refs: llvm_name IS already the pointer
                        if meta.var_type.starts_with('[') || meta.var_type == "array" {
                            return meta.llvm_name;
                        }
                        // For everything else, load the value to get the address
                        let result = self.new_temp();
                        let llvm_type_str = self.type_to_llvm(&meta.var_type);
                        let llvm_name = meta.llvm_name.clone();
                        self.emit(&format!(
                            "  {} = load {}, {}* {}",
                            result, llvm_type_str, llvm_type_str, llvm_name
                        ));
                        result
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
                "print" if !args.is_empty() => match self.infer_type(&args[0]).as_str() {
                    "string" => {
                        let arg_reg = self.gen_node(&args[0]);
                        let result = self.new_temp();
                        self.emit(&format!("  {} = call i32 @puts(i8* {})", result, arg_reg));
                        result
                    }
                    _ => {
                        let arg_reg = self.gen_node(&args[0]);
                        self.emit(&format!("  call void @brn_print_int(i64 {})", arg_reg));
                        "0".to_string()
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
                "vec_new" => {
                    let result = self.new_temp();
                    self.emit(&format!("  {} = call i8* @vec_new_impl()", result));
                    result
                }
                "vec_push" if args.len() >= 2 => {
                    let vec_reg = self.gen_node(&args[0]);
                    let val_reg = self.gen_node(&args[1]);
                    self.emit(&format!(
                        "  call void @vec_push_impl(i8* {}, i64 {})",
                        vec_reg, val_reg
                    ));
                    "0".to_string()
                }
                "vec_get" if args.len() >= 2 => {
                    let vec_reg = self.gen_node(&args[0]);
                    let idx_reg = self.gen_node(&args[1]);
                    let result = self.new_temp();
                    self.emit(&format!(
                        "  {} = call i64 @vec_get_impl(i8* {}, i64 {})",
                        result, vec_reg, idx_reg
                    ));
                    result
                }
                "vec_set" if args.len() >= 3 => {
                    let vec_reg = self.gen_node(&args[0]);
                    let idx_reg = self.gen_node(&args[1]);
                    let val_reg = self.gen_node(&args[2]);
                    self.emit(&format!(
                        "  call void @vec_set_impl(i8* {}, i64 {}, i64 {})",
                        vec_reg, idx_reg, val_reg
                    ));
                    "0".to_string()
                }
                "vec_len" if !args.is_empty() => {
                    let vec_reg = self.gen_node(&args[0]);
                    let result = self.new_temp();
                    self.emit(&format!(
                        "  {} = call i64 @vec_len_impl(i8* {})",
                        result, vec_reg
                    ));
                    result
                }
                "int_to_string" if !args.is_empty() => {
                    let n_reg = self.gen_node(&args[0]);
                    let result = self.new_temp();
                    self.emit(&format!(
                        "  {} = call i8* @int_to_string_impl(i64 {})",
                        result, n_reg
                    ));
                    result
                }
                _ => {
                    let mut arg_regs = Vec::new();
                    let mut arg_types = Vec::new();

                    for arg_node in args {
                        match arg_node {
                            AstNode::Reference(inner) => match inner.as_ref() {
                                AstNode::Identifier { name: var_name, .. } => {
                                    if let Some(meta) =
                                        self.current_function_vars.get(var_name).cloned()
                                    {
                                        if let Some(size) = meta.array_size {
                                            arg_regs.push(meta.llvm_name.clone());
                                            arg_types.push(format!("[{} x i64]*", size));
                                        } else if meta.var_type == "string" {
                                            let loaded = self.new_temp();
                                            self.emit(&format!(
                                                "  {} = load i8*, i8** {}",
                                                loaded, meta.llvm_name
                                            ));
                                            arg_regs.push(loaded);
                                            arg_types.push("i8*".to_string());
                                        } else {
                                            arg_regs.push(meta.llvm_name.clone());
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
                                let arg_type = self.infer_type(arg_node);
                                if arg_type == "string" {
                                    let len = self.new_temp();
                                    let len1 = self.new_temp();
                                    let copy = self.new_temp();
                                    let copied = self.new_temp();
                                    self.emit(&format!(
                                        "  {} = call i64 @strlen(i8* {})",
                                        len, reg
                                    ));
                                    self.emit(&format!("  {} = add i64 {}, 1", len1, len));
                                    self.emit(&format!(
                                        "  {} = call i8* @malloc(i64 {})",
                                        copy, len1
                                    ));
                                    self.emit(&format!(
                                        "  {} = call i8* @strcpy(i8* {}, i8* {})",
                                        copied, copy, reg
                                    ));
                                    arg_regs.push(copy);
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
            },

            AstNode::MethodCall {
                object,
                method,
                args,
            } => {
                let obj_type = self.infer_type(object);
                match method.as_str() {
                    "len" => {
                        let obj_reg = self.gen_node(object);
                        if obj_type == "Vec" {
                            let result = self.new_temp();
                            self.emit(&format!(
                                "  {} = call i64 @vec_len_impl(i8* {})",
                                result, obj_reg
                            ));
                            result
                        } else {
                            let result = self.new_temp();
                            self.emit(&format!("  {} = call i64 @strlen(i8* {})", result, obj_reg));
                            result
                        }
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
                    "push" if !args.is_empty() => {
                        let obj_reg = self.gen_node(object);
                        let val_reg = self.gen_node(&args[0]);
                        self.emit(&format!(
                            "  call void @vec_push_impl(i8* {}, i64 {})",
                            obj_reg, val_reg
                        ));
                        "0".to_string()
                    }
                    "get" if !args.is_empty() => {
                        let obj_reg = self.gen_node(object);
                        let idx_reg = self.gen_node(&args[0]);
                        let result = self.new_temp();
                        self.emit(&format!(
                            "  {} = call i64 @vec_get_impl(i8* {}, i64 {})",
                            result, obj_reg, idx_reg
                        ));
                        result
                    }
                    "set" if args.len() >= 2 => {
                        let obj_reg = self.gen_node(object);
                        let idx_reg = self.gen_node(&args[0]);
                        let val_reg = self.gen_node(&args[1]);
                        self.emit(&format!(
                            "  call void @vec_set_impl(i8* {}, i64 {}, i64 {})",
                            obj_reg, idx_reg, val_reg
                        ));
                        "0".to_string()
                    }
                    _ => "0".to_string(),
                }
            }

            _ => "0".to_string(),
        }
    }

    fn is_pointer_llvm_type(ty: &str) -> bool {
        matches!(ty, "string" | "Vec")
            || ty.starts_with('[')
            || (!matches!(ty, "int" | "bool" | "char" | "void") && !ty.is_empty())
    }

    fn infer_purity(params: &[Parameter], body: &AstNode) -> bool {
        let has_string_param = params.iter().any(|p| {
            let (_, _, inner) = Self::strip_ref_prefix(&p.param_type);
            inner == "string"
        });
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

    fn body_contains_add(node: &AstNode) -> bool {
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
                        .map_or(false, |e| Self::body_contains_add(e))
            }
            AstNode::Call { args, .. } => args.iter().any(Self::body_contains_add),
            AstNode::ExpressionStatement(e) => Self::body_contains_add(e),
            _ => false,
        }
    }

    fn body_is_pure(node: &AstNode) -> bool {
        match node {
            AstNode::Assignment { .. } | AstNode::ArrayAssignment { .. } => false,
            AstNode::Call { name, args } => {
                !matches!(
                    name.as_str(),
                    "print"
                        | "println"
                        | "print_int"
                        | "println_int"
                        | "print_bool"
                        | "println_bool"
                        | "print_char"
                        | "println_char"
                        | "read_file"
                        | "write_file"
                        | "vec_push"
                        | "vec_set"
                        | "send"
                        | "recv"
                        | "spawn"
                ) && args.iter().all(Self::body_is_pure)
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
                    && else_block.as_ref().map_or(true, |e| Self::body_is_pure(e))
            }
            AstNode::While { condition, body } => {
                Self::body_is_pure(condition) && Self::body_is_pure(body)
            }
            AstNode::For { iterator, body, .. } => {
                Self::body_is_pure(iterator) && Self::body_is_pure(body)
            }
            AstNode::Return(v) => v.as_ref().map_or(true, |n| Self::body_is_pure(n)),
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
            AstNode::Identifier { .. }
            | AstNode::Number(_)
            | AstNode::Boolean(_)
            | AstNode::StringLit(_)
            | AstNode::Character(_)
            | AstNode::Break
            | AstNode::Continue
            | AstNode::Import { .. }
            | AstNode::StructDef { .. }
            | AstNode::EnumDef { .. }
            | AstNode::ArrayType { .. }
            | AstNode::EnumValue { value: None, .. } => true,
        }
    }

    fn strip_ref_prefix(ty: &str) -> (bool, bool, &str) {
        if let Some(rest) = ty.strip_prefix("&mut ") {
            (true, true, rest)
        } else if let Some(rest) = ty.strip_prefix('&') {
            (true, false, rest)
        } else {
            (false, false, ty)
        }
    }

    fn mangle_fn(name: &str) -> String {
        match name {
            "main" => "main".to_string(),
            _ => format!("brn_{}", name),
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
        self.label_counter = 0;

        let escaping = EscapeAnalysis::analyze(params, body);
        self.non_escaping.clear();
        if let AstNode::Block(stmts) = body {
            for stmt in stmts {
                if let AstNode::LetBinding { name, .. } = stmt {
                    if !escaping.contains(name) {
                        self.non_escaping.insert(name.clone());
                    }
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

        let param_list = if params.is_empty() {
            String::new()
        } else {
            params
                .iter()
                .map(|p| {
                    let (type_is_ref, type_is_mut, inner_type) =
                        Self::strip_ref_prefix(&p.param_type);
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
                        } else {
                            format!("{}*", self.type_to_llvm(inner_type))
                        }
                    } else {
                        self.type_to_llvm(&p.param_type)
                    };

                    let is_simple_ptr = type_is_ref && !inner_type.starts_with('[');
                    let is_owned_ptr =
                        !type_is_ref && Self::is_pointer_llvm_type(&p.param_type) && !type_is_mut;

                    let attrs = if is_simple_ptr {
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
        };

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

        for param in params {
            let (type_is_ref, _type_is_mut, inner_type) = Self::strip_ref_prefix(&param.param_type);
            let type_is_ref = type_is_ref || param.is_reference;

            if type_is_ref {
                let param_type_name = inner_type.to_string();

                let array_size = if inner_type.starts_with('[') {
                    if let Some(size_str) = inner_type.split(';').nth(1) {
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
                let param_type_str = self.type_to_llvm(&param.param_type);
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

        self.block_terminated = false;
        self.gen_node(body);

        if name == "main" && !self.block_terminated {
            self.emit("  ret i32 0");
        } else if ret_type == "void" && !self.block_terminated {
            self.emit("  ret void");
        }

        self.emit("}");
        String::new()
    }

    fn gen_string_concat(&mut self, left: &str, right: &str) -> String {
        let use_stack = self
            .current_binding
            .as_ref()
            .map(|b| self.non_escaping.contains(b))
            .unwrap_or(false);
        self.gen_string_concat_inner(left, right, use_stack)
    }

    fn gen_string_concat_inner(&mut self, left: &str, right: &str, use_stack: bool) -> String {
        let len1 = self.new_temp();
        let len2 = self.new_temp();
        self.emit(&format!("  {} = call i64 @strlen(i8* {})", len1, left));
        self.emit(&format!("  {} = call i64 @strlen(i8* {})", len2, right));

        let total = self.new_temp();
        let total_plus_one = self.new_temp();
        self.emit(&format!("  {} = add i64 {}, {}", total, len1, len2));
        self.emit(&format!("  {} = add i64 {}, 1", total_plus_one, total));

        let new_ptr = self.new_temp();
        if use_stack {
            // Variable-length stack allocation — no malloc, no free needed.
            // Safe because the binding was proven non-escaping by EscapeAnalysis.
            self.emit(&format!(
                "  {} = alloca i8, i64 {}",
                new_ptr, total_plus_one
            ));
        } else {
            self.emit(&format!(
                "  {} = call i8* @malloc(i64 {})",
                new_ptr, total_plus_one
            ));
        }

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

    fn infer_struct_name(&self, node: &AstNode) -> String {
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

    fn infer_type(&self, node: &AstNode) -> String {
        match node {
            AstNode::Number(_) => "int".to_string(),
            AstNode::Boolean(_) => "bool".to_string(),
            AstNode::Character(_) => "char".to_string(),
            AstNode::StringLit(_) => "string".to_string(),
            AstNode::StructInit { name, .. } => name.clone(),
            AstNode::BinaryOp { left, op, .. } => match op {
                BinOp::Equal
                | BinOp::NotEqual
                | BinOp::LessThan
                | BinOp::LessEqual
                | BinOp::GreaterThan
                | BinOp::GreaterEqual
                | BinOp::And
                | BinOp::Or => "bool".to_string(),
                _ => self.infer_type(left),
            },
            AstNode::Identifier { name, .. } => self
                .current_function_vars
                .get(name)
                .map(|m| m.var_type.clone())
                .unwrap_or_else(|| "int".to_string()),
            AstNode::ArrayLit(_) => "array".to_string(),
            AstNode::EnumValue { .. } => "enum".to_string(),
            AstNode::Call { name, .. } => match name.as_str() {
                "read_file" | "int_to_string" => "string".to_string(),
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
                    _ => obj_type,
                }
            }
            _ => "int".to_string(),
        }
    }

    fn llvm_to_type(&self, llvm: &str) -> String {
        match llvm {
            "i64" => "int".to_string(),
            "i1" => "bool".to_string(),
            "i8" => "char".to_string(),
            "i8*" => "string".to_string(),
            "void" => "void".to_string(),
            _ => "int".to_string(),
        }
    }

    fn type_to_llvm(&self, type_name: &str) -> String {
        match type_name {
            "int" => "i64".to_string(),
            "bool" => "i1".to_string(),
            "char" => "i8".to_string(),
            "string" => "i8*".to_string(),
            "array" => "i64*".to_string(),
            "Vec" => "i8*".to_string(),
            "void" => "void".to_string(),
            "enum" => "{ i32, i64 }*".to_string(),
            t if t.starts_with('*') => {
                let inner = self.type_to_llvm(&t[1..]);
                format!("{}*", inner)
            }
            t if self.struct_types.contains_key(t) => format!("%{}*", t),
            _ => "i64".to_string(),
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
