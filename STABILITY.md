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

### Proc-Macro (`argot-cmd-derive`) Attribute Stability

The `#[derive(ArgotCommand)]` macro and all documented `#[argot(...)]` attributes are **stable from v0.2 onwards**.

| Status | Items |
|--------|-------|
| **Stable** | All struct-level keys: `canonical`, `summary`, `description`, `alias`, `best_practice`, `anti_pattern` |
| **Stable** | All field-level keys: `positional`, `flag`, `required`, `short`, `takes_value`, `description`, `default` |
| **Not guaranteed** | Compiler error message text; internal proc-macro helper identifiers |
| **Breaking (major bump)** | Removing or renaming any documented attribute key |
| **Non-breaking** | Adding new optional attribute keys with backward-compatible defaults |

### Additional Stable Public APIs

The following items are stable from v0.1.0 onwards:

| Item | Type | Notes |
|------|------|-------|
| `AsyncHandlerFn` | Type alias | Feature: `async`. Stable signature. |
| `Middleware` trait | Trait | All three methods (`before_dispatch`, `after_dispatch`, `on_parse_error`) are stable. Adding new default-impl methods in future minor versions is non-breaking. |
| `Renderer` trait | Trait | All four methods are stable. Implementing the trait is stable. |
| `DefaultRenderer` | Struct | Stable, implements `Renderer`. |
| `Registry::iter_all_recursive` | Method | Returns `Vec<CommandEntry<'_>>`. Stable. |
| `Registry::fuzzy_search` | Method | Feature: `fuzzy`. Returns `Vec<(&Command, i64)>`. Stable. |
| `CommandEntry` | Struct | Fields `path: Vec<String>` and `command: &Command` are stable. `name()` and `path_str()` are stable. |
| `render_completion` | Function | `Shell` enum variants and function signature are stable. |
| `render_json_schema` | Function | Returns `Result<String, serde_json::Error>`. Stable. |
| `Shell` | Enum | `Bash`, `Zsh`, `Fish` variants are stable. |
| `Cli::with_renderer` | Method | Stable. |
| `Cli::with_middleware` | Method | Stable. |
| `Cli::with_query_support` | Method | Stable. |
| `Cli::run_and_exit` | Method | Returns `!`. Stable. |
| `Cli::run_env_args_and_exit` | Method | Returns `!`. Stable. |
| `Cli::run_async` | Method | Feature: `async`. Stable. |
| `Cli::run_async_and_exit` | Method | Feature: `async`. Stable. |

### Source layer (added in 0.2.x)

The `source` module is **stable from v0.2 onwards**. Its public surface:

| Item | Type | Notes |
|------|------|-------|
| `CommandSource` | Trait | Object-safe (`Box<dyn CommandSource>` works). Method signatures stable. |
| `EmbeddedSource` | Struct | Constructor + `with_layer` / `with_priority` are stable. |
| `Layer` | Enum | `Embedded`, `User`, `Project`, `Local`, `Custom(i32)` variants stable. `rank()` and `label()` are stable. No `Ord` impl (rank is non-injective on `Custom`). |
| `LayeredBuilder` | Struct | `new`, `add`, `add_boxed`, `build` are stable. `build()` is `#[must_use]`. |
| `LoadedCommand` | Struct | Fields `command`, `priority`, `overrides`, `origin` are stable. Builder methods stable. |
| `LoadDiagnostic` | Enum | Variants `Shadowed`, `OverrideTargetMissing`, `SourceError`, `SchemaWarning` are stable, including their named fields. `is_error()` and `Display` are stable. |
| `SourceLoad`, `SourceOrigin` | Structs | Field shapes stable. |
| `Cli::from_layered` | Method | Returns `(Cli, Vec<LoadDiagnostic>)`, marked `#[must_use]`. Stable. |
| `Cli::commands` | Method | Returns `&[Command]`. Stable. |

### `markdown-source` feature (added in 0.2.x)

| Item | Type | Notes |
|------|------|-------|
| `source::markdown::MarkdownDirSource` | Struct | Constructors `new`, `optional`, `user_config`, `project_root` are stable. |
| `source::markdown::user_config_dir` | Function | Returns `Option<PathBuf>`. Resolution order (XDG → HOME → APPDATA) is stable. |
| `source::markdown::find_project_dir` | Function | Returns `Option<PathBuf>`. Walk-up semantics stable; non-directory candidates silently skipped (documented). |
| Frontmatter schema | YAML subset | Recognised keys: `name` (required), `summary`, `aliases`, `spellings`, `semantic_aliases`, `best_practices`, `anti_patterns`, `priority`, `overrides`, `layer`, `mutating`, `extra`. Adding new optional keys is non-breaking. Removing or repurposing existing keys is breaking. |
| Markdown body sections | `## Arguments`, `## Flags`, `## Examples` | Bullet grammar (see `src/source/markdown.rs` module docs) is stable from 0.2. Heading matching is case-insensitive. Adding new recognised modifiers (`required`, `default:`, etc.) is non-breaking; renaming or removing them is breaking. |

### Trait Object Safety

Both `Middleware` and `Renderer` are object-safe (`Box<dyn Middleware>`, `Box<dyn Renderer>` work). This is a stability guarantee — we will not make breaking changes that remove object safety.

### Middleware Trait Evolution Policy

New methods may be added to `Middleware` in minor versions **only if they have default no-op implementations**. This allows existing implementations to continue compiling without changes. Methods added this way will be announced in CHANGELOG.

### Renderer Trait Evolution Policy

New methods may be added to `Renderer` in minor versions **only if they have default implementations** (e.g., delegating to `render_help`). Existing implementations continue to compile.

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

The minimum supported Rust version is **1.94.0**. MSRV increases are treated as breaking changes for `1.x` and minor-version changes for `0.x`.
