use super::generator::CodeGenerator;

impl CodeGenerator {
    pub(super) fn gen_string_concat(&mut self, left: &str, right: &str) -> String {
        // Whether to stack-alloc the result: only when the binding is non-escaping.
        let use_stack = self
            .current_binding
            .as_ref()
            .map(|b| self.non_escaping.contains(b))
            .unwrap_or(false);
        self.gen_string_concat_inner(left, right, use_stack)
    }

    pub(super) fn gen_string_concat_inner(
        &mut self,
        left: &str,
        right: &str,
        use_stack: bool,
    ) -> String {
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
}
