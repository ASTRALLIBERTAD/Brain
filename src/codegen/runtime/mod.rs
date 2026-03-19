mod linux;
mod windows;

use super::generator::CodeGenerator;

pub(super) fn target_triple() -> &'static str {
    if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc"
    } else if cfg!(target_os = "macos") {
        "x86_64-apple-macosx10.15.0"
    } else {
        "x86_64-pc-linux-gnu"
    }
}

impl CodeGenerator {
    pub(super) fn emit_header(&mut self) {
        if cfg!(target_os = "windows") {
            self.emit_windows_runtime();
        } else {
            self.emit_linux_runtime();
        }

        self.emit_int_to_string();
        self.emit_file_helpers();
        self.emit_vec_helpers();

        // Mode string constants used by read_file_impl / write_file_impl
        self.string_literals
            .push((".str.mode.r".to_string(), "r".to_string()));
        self.string_literals
            .push((".str.mode.w".to_string(), "w".to_string()));
    }

    pub(super) fn emit_footer(&mut self) {
        // Prepend struct type declarations and string literal globals.
        // Building a header string and prepending is cheaper than shifting
        // the entire output buffer on every push.
        let mut header = String::with_capacity(4096);
        for decl in self.struct_decls.iter().rev() {
            header.push_str(decl);
            header.push('\n');
        }
        for (id, value) in &self.string_literals {
            let len = value.len() + 1;
            let escaped = self.escape_string(value);
            header.push_str(&format!(
                "@{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1\n",
                id, len, escaped
            ));
        }
        header.push_str(&self.output);
        self.output = header;
    }

    // Shared string primitives (strlen/strcmp/strcpy)
    // Emitted as pure IR — no libc or syscall dependency.

    pub(super) fn emit_shared_string_ops(&mut self) {
        // strlen
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

        // strcmp
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

        // strcpy
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
    }

    // int_to_string (stack and heap variants)

    fn emit_int_to_string(&mut self) {
        // Stack variant — used by brn_print_int on Windows to avoid malloc
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

        // Heap variant — used by int_to_string() builtin
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
    }

    // File I/O helpers (platform-neutral wrappers around fopen/fclose/etc.)

    fn emit_file_helpers(&mut self) {
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
    }

    // Vec helpers

    fn emit_vec_helpers(&mut self) {
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
    }
}
