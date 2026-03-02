## Summary

<!-- A clear and concise description of what this PR does. -->

Closes #<!-- issue number -->

---

## Type of Change

<!-- Check all that apply -->

- [ ] 🐛 Bug fix
- [ ] ✨ New feature
- [ ] 🧠 Language design change (syntax, type system, ownership model)
- [ ] ♻️ Refactor (no behavior change)
- [ ] 📝 Documentation or example update
- [ ] 🔧 Tooling or build change

---

## Changes Made

<!-- List the files changed and briefly describe what was done in each. -->

- `src/lexer.rs` —
- `src/parser.rs` —
- `src/semantic.rs` —
- `src/codegen.rs` —
- `src/module.rs` —
- `examples/` —
- `README.md` —

<!-- Remove lines that don't apply -->

---

## Testing

<!-- Describe how you tested this change. -->

- [ ] All existing examples in `examples/` compile and produce correct output
- [ ] A new or updated `.brn` example demonstrates this change
- [ ] Malformed input produces a clean error message, not a panic
- [ ] Tested on: <!-- e.g. Windows 11, Ubuntu 22.04 -->

---

## If This Changes Language Behavior

<!-- Fill this out if your PR affects syntax, types, the ownership model, or codegen. Otherwise delete this section. -->

**Before:**
```brain
// What Brain currently does
```

**After:**
```brain
// What Brain does with this PR
```

---

## Checklist

- [ ] I opened an issue and discussed this change before writing code (for non-trivial changes)
- [ ] I ran `rustfmt` on all modified Rust files
- [ ] I updated `README.md` if this adds or changes a language feature
- [ ] My commit messages use the imperative mood and reference the related issue
- [ ] This PR has a single focused purpose — one fix or feature, not several

---

## Additional Notes

<!-- Anything else the maintainer should know — edge cases, follow-up work, open questions. -->
