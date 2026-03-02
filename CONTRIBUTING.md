# Contributing to Brain

Brain is a compiled programming language designed for type safety, high performance, and parallel programming. It was started and is currently led by **Prince Gabrielle Jhon M. Libertad** ([@ASTRALLIBERTAD](https://github.com/ASTRALLIBERTAD)) — but the goal is to grow this into a language built with a community.

If you're excited about language design, compilers, LLVM, or systems programming, you're in the right place. Contributions of all kinds are welcome.

---

## Before You Contribute

Brain is still in its early experimental phase. The language design, syntax, and compiler internals are all evolving. Before writing any significant code, please open an issue first — this avoids wasted effort and gives the author a chance to provide direction and context early.

For small fixes (typos, documentation, example programs), feel free to open a PR directly.

---

## Ways to Contribute

| Type | How |
|---|---|
| 🐛 Bug report | Open an issue using the Bug Report template |
| 💡 Feature request | Open an issue using the Feature Request template |
| 🧠 Language design proposal | Open an issue using the Language Design Proposal template |
| 📝 Docs / typo fix | Open a PR directly |
| 🔧 Compiler fix | Open an issue first, then a PR |
| 🧪 Example programs | Open a PR directly |
| 🏗️ Self-hosting work | Open an issue to coordinate |

---

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (stable toolchain)
- [LLVM / Clang](https://llvm.org/) (for linking and optimization)

### Building Locally

```bash
git clone https://github.com/ASTRALLIBERTAD/Brain.git
cd Brain
cargo build --release
```

Compile and run an example:

```powershell
# Windows (recommended — handles the full pipeline)
.\build.ps1

# Manual
cargo build --release
target\release\brain.exe examples\main.brn
clang -O3 examples\main.ll -o main.exe -lkernel32 -luser32
```

---

## Project Structure

```
src/
├── main.rs          # CLI entry point and compile pipeline
├── lexer.rs         # Tokenizer
├── parser.rs        # AST construction
├── semantic.rs      # Type checking and ownership analysis
├── codegen.rs       # LLVM IR generation
└── module.rs        # Import resolution and module cache

examples/
├── main.brn
├── types/
├── operators/
├── control_flow/
├── functions/
├── strings/
├── arrays/
├── vectors/
├── structs/
├── enums/
├── ownership/
├── files/
└── mutex/
```

The compiler pipeline runs in order: **Lexer → Parser → Module resolver → Semantic analyzer → Code generator → Clang linker**.

---

## Making a Pull Request

### 1. Fork and branch

```bash
git checkout -b fix/borrow-checker-false-positive
# or
git checkout -b feature/float-type
# or
git checkout -b docs/readme-clarification
```

Use a descriptive branch name prefixed with `fix/`, `feature/`, `docs/`, or `refactor/`.

### 2. Make your changes

- Keep changes focused — one fix or feature per PR.
- Run `rustfmt` on any Rust files you touch.
- Match the existing error message format: file, line, column, source line, `^` pointer, and a hint where possible.
- Add or update a `.brn` example in `examples/` when changing language behavior.
- Update the README if you add or change a language feature.
- Do not add external Rust crate dependencies without prior discussion.

### 3. Test your changes

There is no automated test suite yet — manually verify:

- All existing examples in `examples/` still compile and produce correct output.
- Your change is exercised by at least one `.brn` example program.
- Malformed input always produces a clean error, never a panic.

```bash
cargo build --release
target\release\brain.exe examples\main.brn
target\release\brain.exe examples\ownership\ownership.brn
# ... and any examples relevant to your change
```

### 4. Commit messages

Use the imperative mood, keep the subject under 72 characters, and reference the related issue:

```
Fix false positive in borrow checker for re-used identifiers  (#12)
Add float type with arithmetic and comparison operators  (#7)
Improve error messages in semantic analyzer
```

### 5. Open the PR

- Describe *what* changed and *why*.
- Link to the related issue.
- Mark as Draft if it's a work in progress.

---

## Adding a Keyword or Type

Changes to language features require updates across all of these:

1. `lexer.rs` — add the `TokenType` variant and keyword mapping
2. `parser.rs` — add parsing logic and AST node(s)
3. `semantic.rs` — add type-checking rules
4. `codegen.rs` — add IR generation
5. `examples/` — add a `.brn` example demonstrating the feature
6. `README.md` — update the syntax overview and feature list

---

## Error Message Standards

Brain prioritizes developer experience. All compiler errors must:

- Include file name, line, and column.
- Show the offending source line with a `^` pointer.
- Include a brief, actionable hint where possible.
- Never surface a panic or internal message to the user.

---

## Licensing

By submitting a contribution, you agree that your work will be licensed under the same license as this project. See the `LICENSE` file for details.

---

*Brain is an experimental language being built in the open. Whether you're here to fix a bug, propose a feature, or just explore — welcome.*
