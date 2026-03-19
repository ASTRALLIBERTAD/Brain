use super::super::generator::CodeGenerator;

impl CodeGenerator {
    pub(in super::super) fn emit_linux_runtime(&mut self) {
        self.emit("declare i64 @syscall(i64, ...)");
        self.emit("");
        self.emit("@brn_heap_end = global i8* null");
        self.emit("@brn_heap_start = global i8* null");
        self.emit("");

        // brk-based bump allocator
        self.emit("define i8* @malloc(i64 %size) {");
        self.emit("  %cur = load i8*, i8** @brn_heap_end");
        self.emit("  %is_null = icmp eq i8* %cur, null");
        self.emit("  br i1 %is_null, label %init, label %alloc");
        self.emit("init:");
        self.emit("  %brk0 = call i64 (i64, ...) @syscall(i64 12, i64 0)");
        self.emit("  %start = inttoptr i64 %brk0 to i8*");
        self.emit("  store i8* %start, i8** @brn_heap_start");
        self.emit("  store i8* %start, i8** @brn_heap_end");
        self.emit("  br label %alloc");
        self.emit("alloc:");
        self.emit("  %base = load i8*, i8** @brn_heap_end");
        self.emit("  %base_i = ptrtoint i8* %base to i64");
        self.emit("  %align7 = add i64 %size, 7");
        self.emit("  %aligned = and i64 %align7, -8");
        self.emit("  %new_end_i = add i64 %base_i, %aligned");
        self.emit("  %new_end = inttoptr i64 %new_end_i to i8*");
        self.emit("  call i64 (i64, ...) @syscall(i64 12, i64 %new_end_i)");
        self.emit("  store i8* %new_end, i8** @brn_heap_end");
        self.emit("  ret i8* %base");
        self.emit("}");
        self.emit("");

        // realloc: alloc new + copy (bump allocator — no real free)
        self.emit("define i8* @realloc(i8* %ptr, i64 %size) {");
        self.emit("  %new = call i8* @malloc(i64 %size)");
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

        self.emit("define void @free(i8* %ptr) {");
        self.emit("  ret void");
        self.emit("}");
        self.emit("");

        self.emit_shared_string_ops();

        // puts via SYS_write(1, buf, len) + newline
        self.emit("define i32 @puts(i8* %s) {");
        self.emit("  %pt_len = call i64 @strlen(i8* %s)");
        self.emit("  call i64 (i64, ...) @syscall(i64 1, i64 1, i8* %s, i64 %pt_len)");
        self.emit("  %pt_nl = alloca i8");
        self.emit("  store i8 10, i8* %pt_nl");
        self.emit("  call i64 (i64, ...) @syscall(i64 1, i64 1, i8* %pt_nl, i64 1)");
        self.emit("  ret i32 0");
        self.emit("}");
        self.emit("");

        self.emit_linux_file_ops();

        // brn_print_int via SYS_write
        self.emit("define void @brn_print_int(i64 %n) {");
        self.emit("  %bpi_str = call i8* @int_to_string_impl(i64 %n)");
        self.emit("  %bpi_len = call i64 @strlen(i8* %bpi_str)");
        self.emit("  call i64 (i64, ...) @syscall(i64 1, i64 1, i8* %bpi_str, i64 %bpi_len)");
        self.emit("  %bpi_nl = alloca i8");
        self.emit("  store i8 10, i8* %bpi_nl");
        self.emit("  call i64 (i64, ...) @syscall(i64 1, i64 1, i8* %bpi_nl, i64 1)");
        self.emit("  ret void");
        self.emit("}");
        self.emit("");

        // read_input via SYS_read(0, buf, 254)
        self.emit("define i8* @read_input_impl() {");
        self.emit("  %ri_buf = call i8* @malloc(i64 256)");
        self.emit("  %ri_n = call i64 (i64, ...) @syscall(i64 0, i64 0, i8* %ri_buf, i64 254)");
        self.emit("  %ri_endp = getelementptr i8, i8* %ri_buf, i64 %ri_n");
        self.emit("  store i8 0, i8* %ri_endp");
        self.emit("  %ri_has = icmp sgt i64 %ri_n, 0");
        self.emit("  br i1 %ri_has, label %ri_chk_n, label %ri_done");
        self.emit("ri_chk_n:");
        self.emit("  %ri_n1 = sub i64 %ri_n, 1");
        self.emit("  %ri_p1 = getelementptr i8, i8* %ri_buf, i64 %ri_n1");
        self.emit("  %ri_c1 = load i8, i8* %ri_p1");
        self.emit("  %ri_is_n = icmp eq i8 %ri_c1, 10");
        self.emit("  br i1 %ri_is_n, label %ri_strip_n, label %ri_done");
        self.emit("ri_strip_n:");
        self.emit("  store i8 0, i8* %ri_p1");
        self.emit("  br label %ri_done");
        self.emit("ri_done:");
        self.emit("  ret i8* %ri_buf");
        self.emit("}");
        self.emit("");
    }

    fn emit_linux_file_ops(&mut self) {
        // fopen via SYS_open (syscall 2)
        self.emit("define i8* @fopen(i8* %filename, i8* %mode) {");
        self.emit("fo_entry:");
        self.emit("  %fo_mc = load i8, i8* %mode");
        self.emit("  %fo_isw = icmp eq i8 %fo_mc, 119");
        self.emit("  br i1 %fo_isw, label %fo_write, label %fo_read");
        // O_WRONLY|O_CREAT|O_TRUNC = 577, mode 0644 = 420
        self.emit("fo_write:");
        self.emit(
            "  %fo_wfd = call i64 (i64, ...) @syscall(i64 2, i8* %filename, i64 577, i64 420)",
        );
        self.emit("  %fo_wh = inttoptr i64 %fo_wfd to i8*");
        self.emit("  ret i8* %fo_wh");
        // O_RDONLY = 0
        self.emit("fo_read:");
        self.emit("  %fo_rfd = call i64 (i64, ...) @syscall(i64 2, i8* %filename, i64 0, i64 0)");
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
        self.emit(
            "  %fr_n = call i64 (i64, ...) @syscall(i64 0, i64 %fr_fd, i8* %buf, i64 %fr_total)",
        );
        self.emit("  ret i64 %fr_n");
        self.emit("}");
        self.emit("");

        // fwrite via SYS_write (syscall 1)
        self.emit("define i64 @fwrite(i8* %buf, i64 %sz, i64 %count, i8* %handle) {");
        self.emit("  %fw_fd = ptrtoint i8* %handle to i64");
        self.emit("  %fw_total = mul i64 %sz, %count");
        self.emit(
            "  %fw_n = call i64 (i64, ...) @syscall(i64 1, i64 %fw_fd, i8* %buf, i64 %fw_total)",
        );
        self.emit("  ret i64 %fw_n");
        self.emit("}");
        self.emit("");

        // fseek via SYS_lseek (syscall 8)
        self.emit("define i32 @fseek(i8* %handle, i64 %offset, i32 %whence) {");
        self.emit("  %fsk_fd = ptrtoint i8* %handle to i64");
        self.emit("  %fsk_wh = sext i32 %whence to i64");
        self.emit("  call i64 (i64, ...) @syscall(i64 8, i64 %fsk_fd, i64 %offset, i64 %fsk_wh)");
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

        // Mutex primitives (Linux: pure IR spinlock, no pthread/libc)
        // Windows uses CRITICAL_SECTION (40 bytes) with the value at offset 40.
        // We keep the same memory layout on Linux so the rest of codegen is
        // platform-unaware: 40 bytes of spinlock state, value at offset 40.
        // The lock word lives at offset 0 (i64). Spin on cmpxchg — fine for
        // Brain's current single-threaded examples; no futex needed.

        self.emit("define void @InitializeCriticalSection(i8* %cs) {");
        self.emit("  %lp = bitcast i8* %cs to i64*");
        self.emit("  store i64 0, i64* %lp");
        self.emit("  ret void");
        self.emit("}");
        self.emit("");

        self.emit("define void @EnterCriticalSection(i8* %cs) {");
        self.emit("  %lp = bitcast i8* %cs to i64*");
        self.emit("  br label %spin");
        self.emit("spin:");
        self.emit("  %res = cmpxchg i64* %lp, i64 0, i64 1 acq_rel acquire");
        self.emit("  %got = extractvalue { i64, i1 } %res, 1");
        self.emit("  br i1 %got, label %done, label %spin");
        self.emit("done:");
        self.emit("  ret void");
        self.emit("}");
        self.emit("");

        self.emit("define void @LeaveCriticalSection(i8* %cs) {");
        self.emit("  %lp = bitcast i8* %cs to i64*");
        self.emit("  store atomic i64 0, i64* %lp release, align 8");
        self.emit("  ret void");
        self.emit("}");
        self.emit("");
    }
}
