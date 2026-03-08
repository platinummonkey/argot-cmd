# Compile-Fail Test Cases

These `.rs` files document `#[derive(ArgotCommand)]` inputs that are expected
to fail compilation with a helpful error message.

They are NOT wired into the test suite automatically. To verify them manually:

1. Install trybuild: it is NOT in dev-dependencies (add temporarily if needed)
2. Run: `TRYBUILD=overwrite cargo test --features derive compile_fail`

## Cases

| File | Expected error |
|------|---------------|
| `both_positional_and_flag.rs` | field must include either `positional` or `flag`, not both |
| `enum_not_supported.rs` | ArgotCommand can only be derived for structs |
| `missing_kind.rs` | argot field must include either `positional` or `flag` |
