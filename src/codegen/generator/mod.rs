// The CodeGenerator struct lives here.  Each other file in this module adds
// impl blocks via Rust's multi-file impl mechanism.

mod exprs;
mod functions;
mod stmts;

use crate::ast::AstNode;
use std::collections::{HashMap, HashSet};

// Internal types

#[derive(Clone)]
pub(super) struct VarMetadata {
    pub(super) llvm_name: String,
    pub(super) var_type: String,
    pub(super) is_heap: bool,
    pub(super) array_size: Option<usize>,
    pub(super) is_string_literal: bool,
}

pub(super) struct LoopLabels {
    pub(super) continue_label: String,
    pub(super) break_label: String,
}

// CodeGenerator

pub struct CodeGenerator {
    pub(super) output: String,
    pub(super) struct_decls: Vec<String>,
    pub(super) string_counter: usize,
    pub(super) temp_counter: usize,
    pub(super) label_counter: usize,
    pub(super) string_literals: Vec<(String, String)>,
    pub(super) string_literal_map: HashMap<String, String>,
    pub(super) current_function_vars: HashMap<String, VarMetadata>,
    pub(super) loop_stack: Vec<LoopLabels>,
    pub(super) enum_types: HashMap<String, Vec<String>>,
    pub(super) struct_types: HashMap<String, Vec<(String, String)>>,
    pub(super) block_terminated: bool,
    pub(super) current_function_name: String,
    pub(super) current_function_return_type: String,
    pub(super) function_signatures: HashMap<String, String>,
    pub(super) pure_functions: HashSet<String>,
    pub(super) non_escaping: HashSet<String>,

    // StructInit / LetBinding allocation coordination
    // LetBinding sets these before calling gen_node(value) so StructInit reads
    // the *same* decision instead of recalculating independently.
    pub(super) current_binding: Option<String>,
    pub(super) current_binding_is_heap: bool,

    pub(super) is_unsafe_fn: bool,
    pub(super) guard_vars: HashSet<String>,
}

impl CodeGenerator {
    pub fn new() -> Self {
        CodeGenerator {
            output: String::with_capacity(64 * 1024),
            struct_decls: Vec::new(),
            string_counter: 0,
            temp_counter: 0,
            label_counter: 0,
            string_literals: Vec::new(),
            string_literal_map: HashMap::new(),
            current_function_vars: HashMap::new(),
            loop_stack: Vec::new(),
            enum_types: HashMap::new(),
            struct_types: HashMap::new(),
            block_terminated: false,
            current_function_name: String::new(),
            current_function_return_type: String::new(),
            function_signatures: HashMap::new(),
            pure_functions: HashSet::new(),
            non_escaping: HashSet::new(),
            current_binding: None,
            current_binding_is_heap: false,
            is_unsafe_fn: false,
            guard_vars: HashSet::new(),
        }
    }

    pub fn generate(&mut self, ast: &AstNode) -> String {
        // Single pre-pass: collect structs, enums, fn signatures, purity
        if let AstNode::Program(nodes) = ast {
            for node in nodes {
                match node {
                    AstNode::StructDef { name, fields, .. } => {
                        let field_info = fields
                            .iter()
                            .map(|f| (f.name.clone(), f.field_type.clone()))
                            .collect();
                        self.struct_types.insert(name.clone(), field_info);
                    }
                    AstNode::EnumDef { name, variants, .. } => {
                        let variant_names = variants.iter().map(|v| v.name.clone()).collect();
                        self.enum_types.insert(name.clone(), variant_names);
                    }
                    AstNode::FunctionDef {
                        name,
                        params,
                        body,
                        return_type,
                        ..
                    } => {
                        let ret_llvm = if name == "main" {
                            "i32".to_string()
                        } else if let Some(rt) = return_type {
                            self.type_to_llvm(rt)
                        } else {
                            "void".to_string()
                        };
                        self.function_signatures.insert(name.clone(), ret_llvm);
                        if Self::infer_purity(params, body) {
                            self.pure_functions.insert(name.clone());
                        }
                    }
                    _ => {}
                }
            }
        }

        let reachable = if let AstNode::Program(nodes) = ast {
            Self::collect_reachable(nodes)
        } else {
            HashSet::new()
        };

        // Emit struct type declarations
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
                    AstNode::FunctionDef { name, .. } if reachable.contains(name.as_str()) => {
                        self.gen_node(node);
                    }
                    AstNode::LetBinding { name, .. } if reachable.contains(name.as_str()) => {
                        self.gen_node(node);
                    }
                    AstNode::FunctionDef { .. } | AstNode::LetBinding { .. } => {}
                    _ => {
                        self.gen_node(node);
                    }
                }
            }
        }

        self.emit_footer();
        self.build_output()
    }

    // Primitive helpers

    pub(super) fn new_temp(&mut self) -> String {
        let t = format!("%{}", self.temp_counter);
        self.temp_counter += 1;
        t
    }

    pub(super) fn new_label(&mut self, prefix: &str) -> String {
        let l = format!("{}{}", prefix, self.label_counter);
        self.label_counter += 1;
        l
    }

    pub(super) fn new_string_literal(&mut self, value: &str) -> String {
        if let Some(id) = self.string_literal_map.get(value) {
            return id.clone();
        }
        let id = format!(".str.{}", self.string_counter);
        self.string_counter += 1;
        self.string_literal_map
            .insert(value.to_string(), id.clone());
        self.string_literals.push((id.clone(), value.to_string()));
        id
    }

    pub(super) fn emit(&mut self, line: &str) {
        self.output.push_str(line);
        self.output.push('\n');
    }

    pub(super) fn escape_string(&self, s: &str) -> String {
        let mut out = String::new();
        for c in s.bytes() {
            match c {
                b'\n' => out.push_str("\\0A"),
                b'\r' => out.push_str("\\0D"),
                b'\t' => out.push_str("\\09"),
                b'\\' => out.push_str("\\5C"),
                b'"' => out.push_str("\\22"),
                32..=126 => out.push(c as char),
                _ => out.push_str(&format!("\\{:02x}", c)),
            }
        }
        out
    }

    fn build_output(&self) -> String {
        format!(
            "target triple = \"{}\"\n\n{}",
            super::runtime::target_triple(),
            self.output
        )
    }
}
