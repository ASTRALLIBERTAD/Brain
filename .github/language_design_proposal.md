---
name: Language Design Proposal
about: Propose a change or addition to Brain's syntax, type system, or ownership model
title: "[PROPOSAL] "
labels: language-design
assignees: ASTRALLIBERTAD
---

## Proposal Summary

A one or two sentence summary of what you're proposing.

## Motivation

What gap or pain point does this address? Why does Brain need this?

## Detailed Design

Describe the proposed change in full. Consider covering:

- Syntax (what does it look like in `.brn` source?)
- Type system impact (does it introduce new types, change inference, or affect generics?)
- Ownership model impact (does it interact with borrows, moves, or `Mutex<T>`?)
- Compile-time vs runtime behavior

## Example Programs

Show what code would look like with this proposal in place:

```brain
// Before (current Brain)

```

```brain
// After (with this proposal)

```

## Interaction with Existing Features

Does this proposal interact with or affect any of the following?

- [ ] Ownership and borrow checker
- [ ] Structs or enums
- [ ] Match expressions
- [ ] Module system (`import` / `export`)
- [ ] LLVM IR codegen
- [ ] `unsafe` blocks or `Mutex<T>`

Describe any interactions.

## Drawbacks

Are there any downsides, edge cases, or complications introduced by this proposal?

## Prior Art

Does any other language (Rust, Zig, C, Swift, etc.) implement something similar? How does it compare?

## Open Questions

List anything that's unresolved or that you'd like feedback on.
