# Brain Programming Language

Brain is a compiled programming language designed to be **type-safe**, **high-performance**, and **built for parallel programming**. It combines low-level control with strong compile-time guarantees — no garbage collector, no runtime overhead, no surprises.

> ⚠️ Brain is experimental. The language design, syntax, and compiler are still evolving.

---

## Goals

* 🔒 **Type Safety** – Strong static typing and ownership checking at compile time
* ⚡ **High Performance** – Compiled to native code via LLVM with full O3 optimization
* 🧵 **Parallel Programming** – Safe concurrency with Mutex primitives built into the type system
* 🛠️ **Low-level Control** – Manual memory semantics without a garbage collector

---

## What Works Today

The compiler fully implements the following features end-to-end — lexing, parsing, semantic analysis, ownership checking, LLVM IR generation, and linking to a native executable:

- Primitive types: `int`, `bool`, `char`, `string`
- Arithmetic, comparison, and logical operators
- `if` / `else if` / `else`, `while`, `for` loops
- Functions with typed parameters and return values, including recursion
- String concatenation, `.len()`, `.char_at()`, `int_to_string()`
- Fixed-size arrays and dynamic `Vec`
- Structs with named fields and member access
- Enums with optional associated values and `match` expressions
- Ownership and borrow checking — move semantics, `&` borrows, `&mut` mutable borrows
- File I/O — `read_file`, `write_file`
- `Mutex<T>` with `.lock()` and `unsafe` escape hatch
- Module system — `export` and `import` across files and folders
- LLVM O3 optimization pipeline via `build.ps1`

---

## Ownership Model

Brain uses a compile-time ownership system — no garbage collector, no reference counting.

```brain
fn consume_string(s: string) {
    print(s);
}

fn borrow_string(s: &string) {
    print(s);
}

fn ownership_example() {
    let s = "hello world";
    borrow_string(&s);
    borrow_string(&s);
    consume_string(s);
}
```

- `&s` borrows — the caller keeps ownership, can borrow multiple times
- Passing `s` directly moves ownership — `s` cannot be used again after that point
- Violations are caught at compile time, not at runtime

---

## Module System

```brain
export struct Point {
    x: int,
    y: int,
}

export fn make_point(x: int, y: int) -> Point {
    return Point { x: x, y: y };
}
```

```brain
import { Point, make_point } from "structs/structs.brn"

fn main() {
    let p = make_point(3, 4);
    print(p.x);
}
```

`export` works on `fn`, `struct`, `enum`, and `let`. Imports are resolved relative to the importing file.

---

## Example Programs

```
examples/
├── main.brn
├── types/types.brn
├── operators/operators.brn
├── control_flow/control_flow.brn
├── functions/functions.brn
├── strings/strings.brn
├── arrays/arrays.brn
├── vectors/vectors.brn
├── structs/structs.brn
├── enums/enums.brn
├── ownership/ownership.brn
├── files/files.brn
└── mutex/mutex.brn
```

---

## Building

### Requirements

- [Rust](https://rustup.rs/) (for building the Brain compiler)
- [LLVM / Clang](https://llvm.org/) (for optimization and linking)

### Quick Start

```powershell
.\build.ps1
```

`build.ps1` handles everything: compiles the Brain compiler with `cargo`, runs it on your chosen source file to produce LLVM IR, then runs the LLVM O3 optimization pipeline and links a native `.exe`.

### Manual

```powershell
cargo build --release
target\release\brain.exe examples\main.brn
clang -O3 examples\main.ll -o main.exe -lkernel32 -luser32
```

---

## Syntax Overview

```brain
struct Person {
    name: string,
    age: int,
}

enum Direction {
    North,
    South,
    East,
    West,
}

fn match_direction(d: Direction) -> int {
    match d {
        Direction::North => 0,
        Direction::South => 1,
        Direction::East  => 2,
        Direction::West  => 3,
    }
}

fn fib(n: int) -> int {
    if n < 2 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

fn main() {
    let p = Person { name: "Alice", age: 30 };
    print(p.age);

    let d = Direction::North;
    print(match_direction(d));

    print(fib(10));
}
```

---

## Use Cases

* Game engines
* Operating systems and low-level tooling
* Performance-critical applications
* Language and compiler design research

---

## Project Status

The compiler pipeline is complete and produces working native executables. Current focus is preparing for self-hosting — rewriting the Brain compiler in Brain itself.

---

## License

This project is open-source. See the `LICENSE` file for more details.
