use super::super::generator::CodeGenerator;

impl CodeGenerator {
    pub(in super::super) fn emit_windows_runtime(&mut self) {
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
        self.emit("declare void @InitializeCriticalSection(i8*)");
        self.emit("declare void @EnterCriticalSection(i8*)");
        self.emit("declare void @LeaveCriticalSection(i8*)");
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

        self.emit_shared_string_ops();

        // puts via WriteFile to stdout (-11 = STD_OUTPUT_HANDLE)
        self.emit("define i32 @puts(i8* %s) {");
        self.emit("  %pt_out = call i8* @GetStdHandle(i32 -11)");
        self.emit("  %pt_len64 = call i64 @strlen(i8* %s)");
        self.emit("  %pt_len32 = trunc i64 %pt_len64 to i32");
        self.emit("  %pt_written = alloca i32");
        self.emit("  store i32 0, i32* %pt_written");
        self.emit(
            "  call i32 @WriteFile(i8* %pt_out, i8* %s, i32 %pt_len32, i32* %pt_written, i8* null)",
        );
        self.emit("  %pt_nl = alloca i8");
        self.emit("  store i8 10, i8* %pt_nl");
        self.emit(
            "  call i32 @WriteFile(i8* %pt_out, i8* %pt_nl, i32 1, i32* %pt_written, i8* null)",
        );
        self.emit("  ret i32 0");
        self.emit("}");
        self.emit("");

        self.emit_windows_file_ops();

        // brn_print_int via WriteFile
        self.emit("define void @brn_print_int(i64 %n) {");
        self.emit("  %bpi_buf = alloca [32 x i8]");
        self.emit("  %bpi_buf_ptr = getelementptr [32 x i8], [32 x i8]* %bpi_buf, i64 0, i64 0");
        self.emit("  %bpi_str = call i8* @int_to_string_stack(i64 %n, i8* %bpi_buf_ptr)");
        self.emit("  %bpi_out = call i8* @GetStdHandle(i32 -11)");
        self.emit("  %bpi_len64 = call i64 @strlen(i8* %bpi_str)");
        self.emit("  %bpi_len32 = trunc i64 %bpi_len64 to i32");
        self.emit("  %bpi_written = alloca i32");
        self.emit("  store i32 0, i32* %bpi_written");
        self.emit("  call i32 @WriteFile(i8* %bpi_out, i8* %bpi_str, i32 %bpi_len32, i32* %bpi_written, i8* null)");
        self.emit("  %bpi_nl = alloca i8");
        self.emit("  store i8 10, i8* %bpi_nl");
        self.emit(
            "  call i32 @WriteFile(i8* %bpi_out, i8* %bpi_nl, i32 1, i32* %bpi_written, i8* null)",
        );
        self.emit("  ret void");
        self.emit("}");
        self.emit("");

        // read_input via ReadFile from stdin (-10 = STD_INPUT_HANDLE)
        self.emit("define i8* @read_input_impl() {");
        self.emit("  %ri_buf = call i8* @malloc(i64 256)");
        self.emit("  %ri_stdin = call i8* @GetStdHandle(i32 -10)");
        self.emit("  %ri_read = alloca i32");
        self.emit("  store i32 0, i32* %ri_read");
        self.emit(
            "  call i32 @ReadFile(i8* %ri_stdin, i8* %ri_buf, i32 254, i32* %ri_read, i8* null)",
        );
        self.emit("  %ri_n32 = load i32, i32* %ri_read");
        self.emit("  %ri_n = sext i32 %ri_n32 to i64");
        self.emit("  %ri_endp = getelementptr i8, i8* %ri_buf, i64 %ri_n");
        self.emit("  store i8 0, i8* %ri_endp");
        self.emit("  %ri_has = icmp sgt i64 %ri_n, 0");
        self.emit("  br i1 %ri_has, label %ri_chk_n, label %ri_done");
        self.emit("ri_chk_n:");
        self.emit("  %ri_n1 = sub i64 %ri_n, 1");
        self.emit("  %ri_p1 = getelementptr i8, i8* %ri_buf, i64 %ri_n1");
        self.emit("  %ri_c1 = load i8, i8* %ri_p1");
        self.emit("  %ri_is_n = icmp eq i8 %ri_c1, 10");
        self.emit("  br i1 %ri_is_n, label %ri_strip_n, label %ri_chk_r");
        self.emit("ri_strip_n:");
        self.emit("  store i8 0, i8* %ri_p1");
        self.emit("  %ri_has2 = icmp sgt i64 %ri_n1, 0");
        self.emit("  br i1 %ri_has2, label %ri_chk_r, label %ri_done");
        self.emit("ri_chk_r:");
        self.emit("  %ri_n2 = sub i64 %ri_n1, 1");
        self.emit("  %ri_p2 = getelementptr i8, i8* %ri_buf, i64 %ri_n2");
        self.emit("  %ri_c2 = load i8, i8* %ri_p2");
        self.emit("  %ri_is_r = icmp eq i8 %ri_c2, 13");
        self.emit("  br i1 %ri_is_r, label %ri_strip_r, label %ri_done");
        self.emit("ri_strip_r:");
        self.emit("  store i8 0, i8* %ri_p2");
        self.emit("  br label %ri_done");
        self.emit("ri_done:");
        self.emit("  ret i8* %ri_buf");
        self.emit("}");
        self.emit("");
    }

    fn emit_windows_file_ops(&mut self) {
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
        self.emit(
            "  call i32 @WriteFile(i8* %handle, i8* %buf, i32 %fw_t32, i32* %fw_written, i8* null)",
        );
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
        self.emit("  %ft_pos32 = call i32 @SetFilePointer(i8* %handle, i32 0, i32* null, i32 1)");
        self.emit("  %ft_pos64 = sext i32 %ft_pos32 to i64");
        self.emit("  ret i64 %ft_pos64");
        self.emit("}");
        self.emit("");
    }
}
