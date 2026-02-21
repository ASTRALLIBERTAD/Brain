use std::env;
use std::fs;
use std::process;

mod codegen;
mod lexer;
mod parser;
mod semantic;

use codegen::CodeGenerator;
use lexer::Lexer;
use parser::Parser;
use semantic::SemanticAnalyzer;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <input.brn> [output]", args[0]);
        eprintln!("Example: {} main.brn", args[0]);
        process::exit(1);
    }

    let input_file = &args[1];
    let output_file = if args.len() > 2 {
        args[2].clone()
    } else {
        input_file.trim_end_matches(".brn").to_string()
    };

    compile_file(input_file, &output_file);
}

fn get_output_filename(base: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{}.exe", base)
    } else {
        base.to_string()
    }
}

fn compile_file(input_file: &str, output_file: &str) {
    println!("Compiling {}...", input_file);

    // Read source file
    let source = match fs::read_to_string(input_file) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error: Could not read file '{}': {}", input_file, e);
            process::exit(1);
        }
    };

    // Step 1: Lexical Analysis
    println!("  [1/4] Lexical analysis...");
    let mut lexer = Lexer::new(&source, input_file);
    let tokens = match lexer.tokenize() {
        Ok(tokens) => tokens,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    // Step 2: Parsing
    println!("  [2/4] Parsing...");
    let mut parser = Parser::new(tokens, input_file);
    let ast = match parser.parse() {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    // Step 3: Semantic Analysis (Ownership & Memory Safety)
    println!("  [3/4] Semantic analysis (ownership checking)...");
    let mut analyzer = SemanticAnalyzer::new(input_file);
    if let Err(e) = analyzer.analyze(&ast) {
        eprintln!("{}", e);
        process::exit(1);
    }

    // Step 4: Code Generation
    println!("  [4/4] Code generation...");
    let mut codegen = CodeGenerator::new();
    let llvm_ir = codegen.generate(&ast);

    // Write LLVM IR to file
    let ll_file = format!("{}.ll", output_file);

    let output_exe = get_output_filename(output_file);

    let mut cmd = process::Command::new("clang");
    cmd.arg(&ll_file)
        .arg("-o")
        .arg(&output_exe)
        .arg("-Wno-override-module");

    if cfg!(target_os = "windows") {
        cmd.arg("-lmsvcrt");
    } else if cfg!(target_os = "macos") {
        cmd.arg("-lSystem");
    } else {
        cmd.arg("-lc");
    }

    if let Err(e) = fs::write(&ll_file, llvm_ir) {
        eprintln!("Error writing LLVM IR: {}", e);
        process::exit(1);
    }

    println!("  Generated LLVM IR: {}", ll_file);

    // Compile LLVM IR to executable using clang
    println!("  Linking to executable: {}", output_file);

    let output = cmd.output();

    match output {
        Ok(result) => {
            if result.status.success() {
                println!("✓ Successfully compiled to: {}", output_file);
            } else {
                eprintln!("Error during linking:");
                eprintln!("{}", String::from_utf8_lossy(&result.stderr));
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error: clang not found. {}", e);
            println!("LLVM IR saved to: {}", ll_file);
            println!(
                "You can compile manually with: clang {} -o {}",
                ll_file, output_file
            );
        }
    }
}

