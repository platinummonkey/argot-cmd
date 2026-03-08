# Changelog

All notable changes to this project will be documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
Argot adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `Flag::env("VAR_NAME")` — environment variable fallback for flags

### Breaking Changes
<!-- none yet -->

---

## [0.3.0] - 2026-03-08

### Added
- `ParsedCommand` typed accessors: `arg()`, `flag()`, `flag_bool()`, `flag_count()`, `flag_values()`, `has_flag()`
- `Command::meta()` / `Command.extra` — arbitrary `HashMap<String, serde_json::Value>` metadata
- `Flag::choices([...])` with `ParseError::InvalidChoice` enforcement at parse time
- `Flag::repeatable()` — boolean flags count occurrences; value flags collect JSON arrays
- `examples/mcp_server.rs` (feature: `mcp`), `examples/derive_example.rs` (feature: `derive`)
- SECURITY.md, CONTRIBUTING.md, STABILITY.md
- GitHub issue/PR templates and pull request template
- `argot-derive` workspace version/license/repository inheritance
- `#![forbid(unsafe_code)]` in both crates

### Breaking Changes
- `ResolveError::Unknown` is now a struct variant `{ input: String, suggestions: Vec<String> }` (was `Unknown(String)`)
- `ParseError::UnknownSubcommand { parent, got }` added — previously unknown subcommands were silently treated as positionals

---

## [0.2.0] - 2026-03-08

### Added
- Adjacent short flag expansion: `-abc` expands to `-a -b -c` via `VecDeque` token queue
- `--no-{flag}` negation for boolean flags
- `Argument::variadic()` — consumes all remaining positional tokens as a JSON array
- Build-time duplicate detection in `CommandBuilder::build()` (7 new `BuildError` variants: `DuplicateAlias`, `AliasEqualsCanonical`, `DuplicateFlagName`, `DuplicateShortFlag`, `DuplicateArgumentName`, `DuplicateSubcommandName`, `VariadicNotLast`)
- "Did you mean?" suggestions in `ResolveError::Unknown` via Levenshtein edit distance
- `rust-version = "1.75.0"` in `Cargo.toml`
- `[package.metadata.docs.rs]` with `all-features = true`
- `fuzzy-matcher` made optional behind `feature = "fuzzy"`
- CHANGELOG.md

### Breaking Changes
- `ResolveError::Unknown` restructured (see v0.3 for final form)

---

## [0.1.0] - 2026-03-08

### Added
- Five-layer architecture: model → resolver → parser → query → render
- `Command`, `Argument`, `Flag`, `Example` model types with consuming builders (`builder()` entry point)
- `CommandBuilder` with build-time validation (`BuildError::EmptyCanonical`)
- `Registry` with `list_commands()`, `get_command()`, `get_subcommand()`, `search()`, `to_json()`
- `Registry::fuzzy_search()` (feature: `fuzzy`) using `fuzzy-matcher` SkimMatcherV2
- `Parser` with subcommand tree walk, long/short flag binding, `--` separator, `ParsedCommand<'a>` borrow
- `Resolver` with exact → prefix → ambiguous matching; aliases and spellings supported
- `Cli` high-level entry point with `--help`/`-h`, `--version`/`-V`, empty-args listing
- `#[derive(ArgotCommand)]` proc-macro (feature: `derive`) — struct → `Command` builder
- `McpServer` JSON-RPC 2.0 stdio transport (feature: `mcp`)
- Serde serialization for the command tree (`handler` field skipped)
- `Hash`, `Ord`, `Eq` on all model types
- Comprehensive rustdoc with doctests (zero `missing_docs` warnings)
- MIT license, CI with MSRV (1.75.0), `cargo audit`, `cargo doc`, cross-platform tests
- `examples/git_like.rs`, `examples/deploy_tool.rs`
