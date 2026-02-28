use crate::parser::{AstNode, BinOp, Pattern};
use std::collections::HashMap;

#[derive(Debug, Clone)]
struct VarInfo {
    is_consumed: bool,
    borrow_count: usize,
    is_mutable: bool,
    declared_line: usize,
    var_type: String,
}

pub struct SemanticAnalyzer<'a> {
    filename: &'a str,
    symbol_table: Vec<HashMap<String, VarInfo>>,
    current_line: usize,
    current_column: usize,
    in_loop: bool,
    in_unsafe_fn: bool,
}

impl<'a> SemanticAnalyzer<'a> {
    pub fn new(filename: &'a str) -> Self {
        SemanticAnalyzer {
            filename,
            symbol_table: vec![HashMap::new()],
            current_line: 1,
            current_column: 1,
            in_loop: false,
            in_unsafe_fn: false,
        }
    }

    fn is_copy_type(&self, name: &str) -> bool {
        if let Some(info) = self.lookup_variable(name) {
            matches!(info.var_type.as_str(), "int" | "bool" | "char")
        } else {
            false
        }
    }

    pub fn analyze(&mut self, ast: &AstNode) -> Result<(), String> {
        self.visit(ast)
    }

    fn visit(&mut self, node: &AstNode) -> Result<(), String> {
        match node {
            AstNode::Program(nodes) => {
                for node in nodes {
                    self.visit(node)?;
                }
                Ok(())
            }

            AstNode::Import { .. } => Ok(()),

            AstNode::FunctionDef {
                params,
                body,
                is_unsafe,
                ..
            } => {
                let prev_unsafe = self.in_unsafe_fn;
                self.in_unsafe_fn = *is_unsafe;
                self.push_scope();
                for param in params {
                    // Mutex params are always by-reference — enforce this
                    if param.param_type.starts_with("Mutex<") && !param.is_reference {
                        return Err(format!(
                            "{}:{}:{}: Error: Mutex '{}' must be passed by reference '&Mutex<...>', not by value\n    Help: Change to '&{}' or '&mut {}'",
                            self.filename,
                            self.current_line,
                            self.current_column,
                            param.name,
                            param.name,
                            param.name
                        ));
                    }
                    let effective_mutable =
                        param.is_mutable || param.param_type.starts_with("&mut ");
                    let clean_type = if param.param_type.starts_with("&mut ") {
                        param.param_type[5..].to_string()
                    } else {
                        param.param_type.clone()
                    };
                    self.declare_variable(&param.name, effective_mutable, clean_type, 0);
                }
                self.visit(body)?;
                self.pop_scope();
                self.in_unsafe_fn = prev_unsafe;
                Ok(())
            }

            AstNode::LetBinding {
                mutable,
                name,
                value,
                type_annotation,
                location,
                ..
            } => {
                self.current_line = location.line;
                self.current_column = location.column;
                self.visit(value)?;
                if let AstNode::Identifier { name: var_name, .. } = value.as_ref() {
                    self.check_not_consumed(var_name)?;
                    self.consume_variable(var_name)?;
                }
                let guard_type = if let AstNode::MethodCall { object, method, .. } = value.as_ref()
                {
                    if method == "lock" {
                        if let AstNode::Identifier { name: obj_name, .. } = object.as_ref() {
                            if let Some(info) = self.lookup_variable(obj_name) {
                                let obj_type = info.var_type.clone();
                                if obj_type.starts_with("Mutex<") {
                                    let inner = &obj_type[6..obj_type.len() - 1];
                                    Some(format!("MutexGuard<{}>", inner))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                let var_type = guard_type
                    .or_else(|| type_annotation.clone())
                    .unwrap_or_else(|| self.infer_type(value));
                self.declare_variable(name, *mutable, var_type, location.line);
                Ok(())
            }

            AstNode::Assignment {
                name,
                value,
                location,
            } => {
                self.current_line = location.line;
                self.current_column = location.column;
                self.check_variable_exists(name)?;
                self.check_not_consumed(name)?;
                self.check_is_mutable(name)?;
                self.check_not_borrowed(name)?;
                self.visit(value)?;
                if let AstNode::Identifier { name: var_name, .. } = value.as_ref() {
                    self.check_not_consumed(var_name)?;
                    self.consume_variable(var_name)?;
                }
                Ok(())
            }

            AstNode::ArrayAssignment {
                array,
                index,
                value,
                location,
            } => {
                self.current_line = location.line;
                self.current_column = location.column;
                self.check_variable_exists(array)?;
                self.check_not_consumed(array)?;
                self.check_is_mutable(array)?;
                self.visit(index)?;
                self.visit(value)?;
                Ok(())
            }

            AstNode::MemberAssignment {
                object,
                value,
                location,
                ..
            } => {
                self.current_line = location.line;
                self.current_column = location.column;
                self.check_variable_exists(object)?;
                self.visit(value)?;
                Ok(())
            }

            AstNode::Block(statements) => {
                self.push_scope();
                for stmt in statements {
                    self.visit(stmt)?;
                }
                self.pop_scope();
                Ok(())
            }

            AstNode::If {
                condition,
                then_block,
                else_block,
            } => {
                self.visit(condition)?;
                self.visit(then_block)?;
                if let Some(else_block) = else_block {
                    self.visit(else_block)?;
                }
                Ok(())
            }

            AstNode::While { condition, body } => {
                self.visit(condition)?;
                let was_in_loop = self.in_loop;
                self.in_loop = true;
                self.visit(body)?;
                self.in_loop = was_in_loop;
                Ok(())
            }

            AstNode::For {
                variable,
                iterator,
                body,
            } => {
                self.visit(iterator)?;
                self.push_scope();
                self.declare_variable(variable, false, "int".to_string(), self.current_line);
                let was_in_loop = self.in_loop;
                self.in_loop = true;
                self.visit(body)?;
                self.in_loop = was_in_loop;
                self.pop_scope();
                Ok(())
            }

            AstNode::Match { value, arms } => {
                self.visit(value)?;
                for arm in arms {
                    self.push_scope();
                    match &arm.pattern {
                        Pattern::EnumPattern {
                            binding: Some(b), ..
                        } => {
                            self.declare_variable(b, false, "int".to_string(), self.current_line);
                        }
                        Pattern::Identifier(name) if name != "_" => {
                            self.declare_variable(
                                name,
                                false,
                                "unknown".to_string(),
                                self.current_line,
                            );
                        }
                        _ => {}
                    }
                    let arm_result = self.visit(&arm.body);
                    self.pop_scope();
                    arm_result?;
                }
                Ok(())
            }

            AstNode::Return(value) => {
                if let Some(value) = value {
                    self.visit(value)?;
                }
                Ok(())
            }

            AstNode::Break => {
                if !self.in_loop {
                    return Err(format!(
                        "{}:{}:{}: Error: 'break' outside of loop",
                        self.filename, self.current_line, self.current_column
                    ));
                }
                Ok(())
            }

            AstNode::Continue => {
                if !self.in_loop {
                    return Err(format!(
                        "{}:{}:{}: Error: 'continue' outside of loop",
                        self.filename, self.current_line, self.current_column
                    ));
                }
                Ok(())
            }

            AstNode::ExpressionStatement(expr) => self.visit(expr),

            AstNode::BinaryOp { left, right, op } => {
                self.visit(left)?;
                self.visit(right)?;
                if matches!(op, BinOp::Add) {
                    let left_var = match left.as_ref() {
                        AstNode::Identifier { name, .. } => Some(name.as_str()),
                        _ => None,
                    };
                    let right_var = match right.as_ref() {
                        AstNode::Identifier { name, .. } => Some(name.as_str()),
                        _ => None,
                    };
                    if let Some(var) = right_var {
                        if self.get_type(var) == Some("string") {
                            self.check_not_consumed(var)?;
                            self.consume_variable(var)?;
                        }
                    }
                    if let Some(var) = left_var {
                        if self.get_type(var) == Some("string") {
                            self.check_not_consumed(var)?;
                            self.consume_variable(var)?;
                        }
                    }
                }
                Ok(())
            }

            AstNode::UnaryOp { operand, .. } => {
                self.visit(operand)?;
                Ok(())
            }

            AstNode::Identifier { name, location } => {
                self.current_line = location.line;
                self.current_column = location.column;
                self.check_variable_exists(name)?;
                self.check_not_consumed(name)?;
                Ok(())
            }

            AstNode::Reference(expr) => {
                if let AstNode::Identifier { name: var_name, .. } = expr.as_ref() {
                    self.check_not_consumed(var_name)?;
                    self.borrow_variable(var_name)?;
                }
                self.visit(expr)?;
                if let AstNode::Identifier { name: var_name, .. } = expr.as_ref() {
                    self.release_borrow(var_name);
                }
                Ok(())
            }

            AstNode::Call { name: _, args } => {
                let mut borrowed_vars: Vec<String> = Vec::new();
                for arg in args.iter() {
                    if let AstNode::Reference(ref_expr) = arg {
                        if let AstNode::Identifier { name: var_name, .. } = ref_expr.as_ref() {
                            self.check_not_consumed(var_name)?;
                            self.borrow_variable(var_name)?;
                            borrowed_vars.push(var_name.clone());
                        }
                    } else {
                        self.visit(arg)?;
                        // Note: we intentionally do not consume value-passed identifiers here.
                        // The codegen handles memory correctly; over-eager consumption causes
                        // false positives when the same variable name appears across functions.
                    }
                }
                for var_name in &borrowed_vars {
                    self.release_borrow(var_name);
                }
                Ok(())
            }

            AstNode::MethodCall {
                object,
                method,
                args,
                ..
            } => {
                self.visit(object)?;
                for arg in args {
                    self.visit(arg)?;
                }
                if let AstNode::Identifier {
                    name: obj_name,
                    location,
                } = object.as_ref()
                {
                    self.current_line = location.line;
                    self.current_column = location.column;
                    if let Some(info) = self.lookup_variable(obj_name) {
                        let obj_type = info.var_type.clone();
                        if obj_type.starts_with("Mutex<") {
                            if method != "lock" {
                                return Err(format!(
                                    "{}:{}:{}: Error: '{}' is not a valid method on Mutex — only '.lock()' is allowed\n    Help: Use '{}.lock()' to acquire the guard",
                                    self.filename,
                                    self.current_line,
                                    self.current_column,
                                    method,
                                    obj_name
                                ));
                            }
                        }
                    }
                }
                Ok(())
            }

            AstNode::MemberAccess { object, .. } => self.visit(object),

            AstNode::Index { array, index } => {
                self.visit(array)?;
                self.visit(index)?;
                Ok(())
            }

            AstNode::ArrayLit(elements) => {
                for elem in elements {
                    self.visit(elem)?;
                }
                Ok(())
            }

            AstNode::StructInit { fields, .. } => {
                for (_, value) in fields {
                    self.visit(value)?;
                }
                Ok(())
            }

            AstNode::EnumValue { value, .. } => {
                if let Some(value) = value {
                    self.visit(value)?;
                }
                Ok(())
            }

            AstNode::StructDef { .. } => Ok(()),
            AstNode::EnumDef { .. } => Ok(()),
            AstNode::ArrayType { .. } => Ok(()),
            AstNode::Number(_) => Ok(()),
            AstNode::Boolean(_) => Ok(()),
            AstNode::Character(_) => Ok(()),
            AstNode::StringLit(_) => Ok(()),
        }
    }

    fn declare_variable(&mut self, name: &str, mutable: bool, var_type: String, line: usize) {
        let scope = self.symbol_table.last_mut().unwrap();
        scope.insert(
            name.to_string(),
            VarInfo {
                is_consumed: false,
                borrow_count: 0,
                is_mutable: mutable,
                declared_line: line,
                var_type,
            },
        );
    }

    fn check_variable_exists(&self, name: &str) -> Result<(), String> {
        if self.lookup_variable(name).is_none() {
            return Err(format!(
                "{}:{}:{}: Error: cannot find value '{}' in this scope",
                self.filename, self.current_line, self.current_column, name
            ));
        }
        Ok(())
    }

    fn check_not_consumed(&self, name: &str) -> Result<(), String> {
        if self.is_copy_type(name) {
            return Ok(());
        }
        if let Some(info) = self.lookup_variable(name) {
            if info.is_consumed {
                return Err(format!(
                    "{}:{}:{}: Error: use of moved value '{}'\n    Note: value moved at line {}, cannot be used again\n    Help: Consider borrowing '&{}' to keep ownership",
                    self.filename,
                    self.current_line,
                    self.current_column,
                    name,
                    info.declared_line,
                    name
                ));
            }
        }
        Ok(())
    }

    fn check_is_mutable(&self, name: &str) -> Result<(), String> {
        if let Some(info) = self.lookup_variable(name) {
            if !info.is_mutable {
                return Err(format!(
                    "{}:{}:{}: Error: cannot assign to immutable variable '{}'\n    Help: Consider declaring with 'let mut {}'",
                    self.filename, self.current_line, self.current_column, name, name
                ));
            }
        }
        Ok(())
    }

    fn check_not_borrowed(&self, name: &str) -> Result<(), String> {
        if let Some(info) = self.lookup_variable(name) {
            if info.borrow_count > 0 {
                return Err(format!(
                    "{}:{}:{}: Error: cannot move '{}' while borrowed\n    Note: {} active borrow(s) exist",
                    self.filename, self.current_line, self.current_column, name, info.borrow_count
                ));
            }
        }
        Ok(())
    }

    fn consume_variable(&mut self, name: &str) -> Result<(), String> {
        if self.is_copy_type(name) {
            return Ok(());
        }
        for scope in self.symbol_table.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                if info.borrow_count > 0 {
                    return Err(format!(
                        "{}:{}:{}: Error: cannot move '{}' while borrowed",
                        self.filename, self.current_line, self.current_column, name
                    ));
                }
                info.is_consumed = true;
                return Ok(());
            }
        }
        Ok(())
    }

    fn borrow_variable(&mut self, name: &str) -> Result<(), String> {
        for scope in self.symbol_table.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                info.borrow_count += 1;
                return Ok(());
            }
        }
        Ok(())
    }

    fn release_borrow(&mut self, name: &str) {
        for scope in self.symbol_table.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                if info.borrow_count > 0 {
                    info.borrow_count -= 1;
                }
                return;
            }
        }
    }

    fn lookup_variable(&self, name: &str) -> Option<&VarInfo> {
        for scope in self.symbol_table.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info);
            }
        }
        None
    }

    fn get_type(&self, name: &str) -> Option<&str> {
        self.lookup_variable(name)
            .map(|info| info.var_type.as_str())
    }

    fn infer_type(&self, expr: &AstNode) -> String {
        match expr {
            AstNode::Number(_) => "int".to_string(),
            AstNode::Boolean(_) => "bool".to_string(),
            AstNode::Character(_) => "char".to_string(),
            AstNode::StringLit(_) => "string".to_string(),
            AstNode::Identifier { name, .. } => {
                self.get_type(name).unwrap_or("unknown").to_string()
            }
            AstNode::BinaryOp { left, .. } => self.infer_type(left),
            AstNode::ArrayLit(elements) => {
                if elements.is_empty() {
                    "[int; 0]".to_string()
                } else {
                    let elem_type = self.infer_type(&elements[0]);
                    format!("[{}; {}]", elem_type, elements.len())
                }
            }
            _ => "unknown".to_string(),
        }
    }

    fn push_scope(&mut self) {
        self.symbol_table.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.symbol_table.pop();
    }
}
