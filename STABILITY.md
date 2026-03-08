# API Stability

## Versioning Policy

argot follows [Semantic Versioning](https://semver.org/):

| Version range | Meaning |
|---|---|
| `0.x.y` | Pre-release. Minor versions (`0.x`) may contain breaking changes. Patch versions (`0.x.y`) are backward-compatible. |
| `1.0.0+` | Stable. Breaking changes require a new major version. |

## What Is Stable (0.x)

The following are considered **public API** and will not change within a minor version:

- All `pub` items in `src/lib.rs` re-exports
- `Command`, `Argument`, `Flag`, `Example` struct fields
- `BuildError`, `ParseError`, `ResolveError`, `QueryError`, `CliError` variants
- `Registry`, `Parser`, `Resolver`, `Cli` method signatures
- `ArgotCommand` trait
- `McpServer` method signatures (feature: `mcp`)

### Proc-Macro (`argot-derive`) Attribute Stability

The `#[derive(ArgotCommand)]` macro and all documented `#[argot(...)]` attributes are **stable from v0.2 onwards**.

| Status | Items |
|--------|-------|
| **Stable** | All struct-level keys: `canonical`, `summary`, `description`, `alias`, `best_practice`, `anti_pattern` |
| **Stable** | All field-level keys: `positional`, `flag`, `required`, `short`, `takes_value`, `description`, `default` |
| **Not guaranteed** | Compiler error message text; internal proc-macro helper identifiers |
| **Breaking (major bump)** | Removing or renaming any documented attribute key |
| **Non-breaking** | Adding new optional attribute keys with backward-compatible defaults |

## What May Change (0.x)

- Items marked `#[doc(hidden)]`
- Internal module structure (`src/model/command.rs` vs `src/model/mod.rs`)
- Private fields and helpers
- Proc-macro attribute syntax (best-effort stability)

## Breaking Changes

Breaking changes are noted in `CHANGELOG.md` under a `### Breaking Changes` heading. Before 1.0, they may occur in minor version bumps (`0.2 → 0.3`).

## Deprecation

Deprecated items will be annotated with `#[deprecated(since = "x.y.z", note = "...")]` for at least one minor version before removal.

## MSRV

The minimum supported Rust version is **1.75.0**. MSRV increases are treated as breaking changes for `1.x` and minor-version changes for `0.x`.
