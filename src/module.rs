// I think this implementation is not good, I don't know hahaha

use crate::lexer::Lexer;
use crate::parser::{AstNode, Parser};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

pub struct ModuleExports {
    pub exported_names: HashSet<String>,
    pub all_definitions: Vec<AstNode>,
}

pub struct ModuleCache {
    cache: HashMap<String, ModuleExports>,
    currently_loading: HashSet<String>,
}

impl ModuleCache {
    pub fn new() -> Self {
        ModuleCache {
            cache: HashMap::new(),
            currently_loading: HashSet::new(),
        }
    }

    pub fn import(
        &mut self,
        requesting_file: &str,
        import_path: &str,
        requested_names: &[String],
    ) -> Result<Vec<AstNode>, String> {
        let canonical = Self::resolve_path(requesting_file, import_path)?;

        if !self.cache.contains_key(&canonical) {
            self.load_module(&canonical)?;
        }

        let exports = self.cache.get(&canonical).unwrap();

        for name in requested_names {
            if !exports.exported_names.contains(name) {
                return Err(format!(
                    "Error: '{}' is not exported from '{}'.\n  Exported symbols: {}\n  Hint: add 'export' before the declaration in '{}'",
                    name,
                    import_path,
                    Self::format_names(&exports.exported_names),
                    import_path,
                ));
            }
        }

        // Expand the requested set to include every internal helper that is
        // transitively called by the requested functions.  Without this, a
        // function like `enemy_take_damage` that calls a private helper
        // `_clamp` would produce an LLVM call to `@brn__clamp` with no
        // definition, causing a linker error.
        let needed = Self::transitive_needed(requested_names, &exports.all_definitions);

        Ok(exports
            .all_definitions
            .iter()
            .filter(|node| match node {
                AstNode::FunctionDef { name, .. }
                | AstNode::LetBinding { name, .. }
                | AstNode::StructDef { name, .. }
                | AstNode::EnumDef { name, .. } => needed.contains(name.as_str()),
                _ => true,
            })
            .cloned()
            .collect())
    }

    pub fn resolve_path(requesting_file: &str, import_path: &str) -> Result<String, String> {
        let base = Path::new(requesting_file)
            .parent()
            .unwrap_or(Path::new("."));
        let full = base.join(import_path);
        full.canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .map_err(|_| {
                format!(
                    "Error: cannot find module '{}' (resolved from '{}')",
                    import_path, requesting_file
                )
            })
    }

    fn load_module(&mut self, canonical_path: &str) -> Result<(), String> {
        if self.currently_loading.contains(canonical_path) {
            return Err(format!(
                "Error: circular import detected — '{}' is already being loaded",
                canonical_path
            ));
        }
        self.currently_loading.insert(canonical_path.to_string());

        let source = fs::read_to_string(canonical_path)
            .map_err(|e| format!("Error: cannot read module '{}': {}", canonical_path, e))?;

        let path_owned = canonical_path.to_string();
        let mut lexer = Lexer::new(&source, &path_owned);
        let tokens = lexer
            .tokenize()
            .map_err(|e| format!("Lex error in '{}': {}", canonical_path, e))?;

        let mut parser = Parser::new(tokens, &path_owned);
        let ast = parser
            .parse()
            .map_err(|e| format!("Parse error in '{}': {}", canonical_path, e))?;

        let mut transitive_imports: Vec<(String, Vec<String>)> = Vec::new();
        if let AstNode::Program(ref nodes) = ast {
            for node in nodes {
                if let AstNode::Import { names, path } = node {
                    let dep = Self::resolve_path(canonical_path, path)?;
                    transitive_imports.push((dep, names.clone()));
                }
            }
        }

        for (dep_canonical, _) in &transitive_imports {
            if !self.cache.contains_key(dep_canonical) {
                self.load_module(dep_canonical)?;
            }
        }

        let mut exported_names = HashSet::new();
        let mut all_definitions: Vec<AstNode> = Vec::new();
        let mut seen_names: HashSet<String> = HashSet::new();

        for (dep_canonical, dep_names) in &transitive_imports {
            if let Some(dep_exports) = self.cache.get(dep_canonical) {
                for name in dep_names {
                    if !dep_exports.exported_names.contains(name) {
                        return Err(format!(
                            "Error: '{}' is not exported from '{}' (imported by '{}')",
                            name, dep_canonical, canonical_path
                        ));
                    }
                }
                for node in &dep_exports.all_definitions {
                    match node {
                        AstNode::FunctionDef { name, .. }
                        | AstNode::LetBinding { name, .. }
                        | AstNode::StructDef { name, .. }
                        | AstNode::EnumDef { name, .. } => {
                            if seen_names.insert(name.clone()) {
                                all_definitions.push(node.clone());
                            }
                        }
                        other => all_definitions.push(other.clone()),
                    }
                }
            }
        }

        if let AstNode::Program(nodes) = ast {
            for node in nodes {
                match &node {
                    AstNode::Import { .. } => {}

                    AstNode::FunctionDef {
                        name, is_exported, ..
                    } => {
                        if *is_exported {
                            exported_names.insert(name.clone());
                        }
                        if seen_names.insert(name.clone()) {
                            all_definitions.push(node);
                        }
                    }

                    AstNode::LetBinding {
                        name, is_exported, ..
                    } => {
                        if *is_exported {
                            exported_names.insert(name.clone());
                        }
                        if seen_names.insert(name.clone()) {
                            all_definitions.push(node);
                        }
                    }

                    AstNode::StructDef {
                        name, is_exported, ..
                    } => {
                        if *is_exported {
                            exported_names.insert(name.clone());
                        }
                        if seen_names.insert(name.clone()) {
                            all_definitions.push(node);
                        }
                    }

                    AstNode::EnumDef {
                        name, is_exported, ..
                    } => {
                        if *is_exported {
                            exported_names.insert(name.clone());
                        }
                        if seen_names.insert(name.clone()) {
                            all_definitions.push(node);
                        }
                    }

                    other => {
                        all_definitions.push(other.clone());
                    }
                }
            }
        }

        self.currently_loading.remove(canonical_path);

        self.cache.insert(
            canonical_path.to_string(),
            ModuleExports {
                exported_names,
                all_definitions,
            },
        );

        Ok(())
    }

    /// Starting from `roots`, walk call-graph edges within `definitions` to
    /// find every function (exported or not) that must be included so that
    /// all call sites have a definition available.
    fn transitive_needed<'a>(roots: &'a [String], definitions: &'a [AstNode]) -> HashSet<&'a str> {
        // Build a quick name → body map for every FunctionDef in the module.
        let body_map: HashMap<&str, &AstNode> = definitions
            .iter()
            .filter_map(|n| {
                if let AstNode::FunctionDef { name, body, .. } = n {
                    Some((name.as_str(), body.as_ref()))
                } else {
                    None
                }
            })
            .collect();

        let mut needed: HashSet<&str> = HashSet::new();
        let mut queue: Vec<&str> = roots.iter().map(|s| s.as_str()).collect();

        while let Some(current) = queue.pop() {
            if !needed.insert(current) {
                continue; // already visited
            }
            if let Some(body) = body_map.get(current) {
                Self::collect_calls_from_body(body, &mut queue);
            }
        }

        needed
    }

    /// Recursively collect all direct Call targets from an AST node.
    fn collect_calls_from_body<'a>(node: &'a AstNode, out: &mut Vec<&'a str>) {
        match node {
            AstNode::Call { name, args } => {
                out.push(name.as_str());
                for a in args {
                    Self::collect_calls_from_body(a, out);
                }
            }
            AstNode::Block(stmts) | AstNode::Program(stmts) => {
                for s in stmts {
                    Self::collect_calls_from_body(s, out);
                }
            }
            AstNode::FunctionDef { body, .. } => Self::collect_calls_from_body(body, out),
            AstNode::LetBinding { value, .. } | AstNode::Assignment { value, .. } => {
                Self::collect_calls_from_body(value, out)
            }
            AstNode::If {
                condition,
                then_block,
                else_block,
            } => {
                Self::collect_calls_from_body(condition, out);
                Self::collect_calls_from_body(then_block, out);
                if let Some(e) = else_block {
                    Self::collect_calls_from_body(e, out);
                }
            }
            AstNode::While { condition, body } => {
                Self::collect_calls_from_body(condition, out);
                Self::collect_calls_from_body(body, out);
            }
            AstNode::For { iterator, body, .. } => {
                Self::collect_calls_from_body(iterator, out);
                Self::collect_calls_from_body(body, out);
            }
            AstNode::Return(Some(v)) => Self::collect_calls_from_body(v, out),
            AstNode::BinaryOp { left, right, .. } => {
                Self::collect_calls_from_body(left, out);
                Self::collect_calls_from_body(right, out);
            }
            AstNode::UnaryOp { operand, .. } => Self::collect_calls_from_body(operand, out),
            AstNode::ExpressionStatement(e) => Self::collect_calls_from_body(e, out),
            AstNode::Match { value, arms } => {
                Self::collect_calls_from_body(value, out);
                for arm in arms {
                    Self::collect_calls_from_body(&arm.body, out);
                }
            }
            AstNode::ArrayLit(elems) => {
                for e in elems {
                    Self::collect_calls_from_body(e, out);
                }
            }
            AstNode::StructInit { fields, .. } => {
                for (_, v) in fields {
                    Self::collect_calls_from_body(v, out);
                }
            }
            AstNode::Index { array, index } => {
                Self::collect_calls_from_body(array, out);
                Self::collect_calls_from_body(index, out);
            }
            AstNode::Reference(e) | AstNode::EnumValue { value: Some(e), .. } => {
                Self::collect_calls_from_body(e, out)
            }
            AstNode::MethodCall { object, args, .. } => {
                Self::collect_calls_from_body(object, out);
                for a in args {
                    Self::collect_calls_from_body(a, out);
                }
            }
            AstNode::MemberAccess { object, .. } => Self::collect_calls_from_body(object, out),
            AstNode::ArrayAssignment { index, value, .. } => {
                Self::collect_calls_from_body(index, out);
                Self::collect_calls_from_body(value, out);
            }
            AstNode::MemberAssignment { value, .. } => Self::collect_calls_from_body(value, out),
            _ => {}
        }
    }

    fn format_names(names: &HashSet<String>) -> String {
        if names.is_empty() {
            return "(none — no symbols are exported from this module)".to_string();
        }
        let mut sorted: Vec<&String> = names.iter().collect();
        sorted.sort();
        sorted
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub fn resolve_imports(
    ast: AstNode,
    cache: &mut ModuleCache,
    file: &str,
) -> Result<AstNode, String> {
    if let AstNode::Program(nodes) = ast {
        let mut resolved: Vec<AstNode> = Vec::new();
        // Global dedup across all import statements in this file.
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for node in nodes {
            match node {
                AstNode::Import { names, path } => {
                    let defs = cache.import(file, &path, &names)?;
                    for def in defs {
                        match &def {
                            AstNode::FunctionDef { name, .. }
                            | AstNode::LetBinding { name, .. }
                            | AstNode::StructDef { name, .. }
                            | AstNode::EnumDef { name, .. } => {
                                if seen.insert(name.clone()) {
                                    resolved.push(def);
                                }
                            }
                            other => resolved.push(other.clone()),
                        }
                    }
                }
                other => resolved.push(other),
            }
        }
        Ok(AstNode::Program(resolved))
    } else {
        Ok(ast)
    }
}
