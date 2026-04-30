# Changelog

All notable changes to this project will be documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
Argot adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `source` module — layered command sources with priority-ordered merging.
  - `CommandSource` trait (object-safe) for plugging in custom command producers.
  - `Layer` enum (`Embedded`, `User`, `Project`, `Local`, `Custom(i32)`) with
    `rank()` for precedence comparisons. Higher rank wins.
  - `LoadedCommand` carrying `priority` and optional `overrides` hints, plus
    a `SourceOrigin` for provenance.
  - `LayeredBuilder` — assembles a `Registry` from multiple sources with
    deterministic merging by (layer rank, priority, insertion order).
  - `LoadDiagnostic` (Shadowed / OverrideTargetMissing / SourceError /
    SchemaWarning) with `Display` impls that surface source, layer, and path.
  - `EmbeddedSource` — wraps a `Vec<Command>` as a `CommandSource`.
- `markdown-source` feature flag — enables `MarkdownDirSource`, which loads
  commands from `*.md` files with YAML-style frontmatter (`name`, `summary`,
  `aliases`, `priority`, `overrides`, `layer`, `mutating`, `extra`, etc.).
- `examples/layered_commands.rs` demonstrating the layered-source pattern.
- `Cli::from_layered(LayeredBuilder) -> (Cli, Vec<LoadDiagnostic>)` —
  one-call construction of a `Cli` from layered command sources, returning
  diagnostics so the caller can surface them.
- `Cli::commands() -> &[Command]` — borrow the registered top-level commands.
- `MarkdownDirSource` now parses `## Arguments`, `## Flags`, and `## Examples`
  sections in the markdown body into typed `Argument`, `Flag`, and `Example`
  values. Bullet grammar supports modifiers (`required`, `variadic`,
  `default:`, `env:`, `choices:`, `repeatable`) and per-bullet parse failures
  emit `SchemaWarning` diagnostics rather than aborting the file.
- `docs/migrating-from-clap.md` — translation table + worked examples for
  applications coming from clap.
- Discovery helpers in `source::markdown`:
  - `MarkdownDirSource::user_config(app_name)` — `Layer::User` source rooted at
    the platform's user-config directory (`$XDG_CONFIG_HOME` / `~/.config` /
    `%APPDATA%`), marked optional.
  - `MarkdownDirSource::project_root(app_name)` — `Layer::Project` source found
    by walking up from the current directory looking for
    `.<app_name>/commands/`, marked optional.
  - `user_config_dir(app_name)` and `find_project_dir(app_name, start)` free
    functions for callers that need to inspect the resolved path before
    constructing a source.

---

## [0.1.0] - 2026-03-08

Initial release of Argot — an agent-first command interface framework for Rust.

### Model

- `Command`, `Argument`, `Flag`, `Example` with consuming builder pattern
- `Command::builder(canonical)` → `CommandBuilder` with full validation at `build()`
- `Flag::choices([...])` — parse-time enforcement with `ParseError::InvalidChoice`
- `Flag::repeatable()` — boolean flags count occurrences; value flags collect JSON arrays
- `Flag::env("VAR_NAME")` — environment variable fallback (CLI → env → default → error)
- `Argument::variadic()` — final positional consumes all remaining tokens as JSON array
- `Command::exclusive([...])` — mutually exclusive flag groups (`ParseError::MutuallyExclusive`)
- `Command.extra: HashMap<String, serde_json::Value>` — arbitrary metadata
- `HandlerFn` / `AsyncHandlerFn` — sync and async handler type aliases
- `ParsedCommand` typed accessors: `arg`, `flag`, `flag_bool`, `flag_count`, `flag_values`, `has_flag`
- `ParsedCommand` coercion: `arg_as::<T>`, `flag_as::<T>`, `arg_as_or`, `flag_as_or`
- `#[derive(ArgotCommand)]` proc-macro (`--features derive`) — struct → `Command` builder
- `Hash`, `Ord`, `Eq`, `serde::Serialize`/`Deserialize` on all model types
- `#![forbid(unsafe_code)]` in both `argot-cmd` and `argot-cmd-derive`

### Resolver

- Exact → prefix → ambiguous resolution pipeline
- Alias and spelling resolution (spellings are silent; not shown in help or ambiguity output)
- "Did you mean?" suggestions via Levenshtein edit distance in `ResolveError::Unknown`
- `render_resolve_error(&ResolveError) -> String` for human-readable output

### Parser

- Full argv tokenizer: `--flag=value`, `-f value`, `-abc` short expansion, `--` separator
- `ParseError` with 10+ typed variants (all recoverable, all carry context)
- `ParseError::MutuallyExclusive`, `InvalidChoice`, `UnknownSubcommand`
- Environment variable fallback applied before required/default validation
- Mutual exclusivity enforced after env-var resolution

### Query

- `Registry` — command store with `list_commands`, `get_command`, `get_subcommand`, `get_examples`, `search`, `to_json`
- `Registry::fuzzy_search` (`--features fuzzy`) — SkimMatcherV2 ranked results
- `Registry::iter_all_recursive` — depth-first `CommandEntry` iterator over full tree
- `CommandEntry { path, command }` with `path_str()` and `name()` helpers

### Render

- `render_help(command)` — plain-text help with arguments, flags, examples, best practices
- `render_markdown(command)` — Markdown documentation page
- `render_subcommand_list(commands)` — compact command listing
- `render_ambiguity(input, candidates)` — "Did you mean?" prose
- `render_resolve_error(&ResolveError)` — dispatches to ambiguity or unknown renderer
- `render_completion(Shell, program, registry)` — shell completion scripts (Bash / Zsh / Fish)
- `render_json_schema(command)` — JSON Schema draft-07 for agent tool definitions
- `Renderer` trait + `DefaultRenderer` — pluggable rendering via `Cli::with_renderer`

### CLI

- `Cli::new(commands)` with `app_name`, `version`, `with_renderer`, `with_middleware`, `with_query_support`
- `Cli::run(args)`, `run_env_args()` — sync dispatch
- `Cli::run_and_exit(args)`, `run_env_args_and_exit()` — dispatch and `process::exit`
- `Cli::run_async(args)`, `run_env_args_async()` — async dispatch (`--features async`)
- Built-ins: `--help` / `-h`, `--version` / `-V`, empty-args listing
- Query built-in: `tool query commands`, `tool query <name>`, `tool query <name> --json`
- `Middleware` trait — `before_dispatch`, `after_dispatch`, `on_parse_error` hooks

### Transport

- `McpServer` — JSON-RPC 2.0 stdio MCP server (`--features mcp`)
- Commands exposed as MCP tools; `tools/list` and `tools/call` supported
- `best_practice` and `anti_pattern` annotations surfaced in tool descriptions

### Derive macro (`--features derive`)

- `#[derive(ArgotCommand)]` on structs — generates `Command` via `ArgotCommand::command()`
- Struct attrs: `canonical`, `summary`, `description`, `alias`, `best_practice`, `anti_pattern`
- Field attrs: `positional`, `flag`, `required`, `short`, `takes_value`, `description`, `default`
- `CamelCase` → `kebab-case` struct names; `snake_case` → `kebab-case` field names
- Clear compile errors: conflict detection, valid-key hints, field-naming in messages

### Documentation

- [Error Handling guide](docs/error-handling.md)
- [Validation Patterns guide](docs/validation-patterns.md)
- [Cookbook](docs/cookbook.md) — 10 recipes
- `STABILITY.md` — semver guarantees including proc-macro attribute stability

### CI

- `cargo test` on stable/MSRV (1.94.0), `cargo clippy -D warnings`, `cargo doc`
- `cargo audit` for security advisories
- `cargo tarpaulin` coverage with 80% threshold enforcement
