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

        Ok(exports.all_definitions.clone())
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

        for (dep_canonical, dep_names) in &transitive_imports {
            if let Some(dep_exports) = self.cache.get(dep_canonical) {
                // Access check for this module's own imports
                for name in dep_names {
                    if !dep_exports.exported_names.contains(name) {
                        return Err(format!(
                            "Error: '{}' is not exported from '{}' (imported by '{}')",
                            name, dep_canonical, canonical_path
                        ));
                    }
                }
                all_definitions.extend(dep_exports.all_definitions.clone());
            }
        }

        if let AstNode::Program(nodes) = ast {
            for node in nodes {
                match &node {
                    AstNode::Import { .. } => {} // already handled above

                    AstNode::FunctionDef {
                        name, is_exported, ..
                    } => {
                        if *is_exported {
                            exported_names.insert(name.clone());
                        }
                        all_definitions.push(node);
                    }

                    AstNode::LetBinding {
                        name, is_exported, ..
                    } => {
                        if *is_exported {
                            exported_names.insert(name.clone());
                        }
                        all_definitions.push(node);
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
        for node in nodes {
            match node {
                AstNode::Import { names, path } => {
                    let defs = cache.import(file, &path, &names)?;
                    resolved.extend(defs);
                }
                other => resolved.push(other),
            }
        }
        Ok(AstNode::Program(resolved))
    } else {
        Ok(ast)
    }
}
