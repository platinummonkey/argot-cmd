# Argot — an agent-first command interface framework for Rust

<!-- Badges -->
![CI](https://github.com/platinummonkey/argot/actions/workflows/ci.yml/badge.svg)
[![Coverage](https://codecov.io/gh/platinummonkey/argot/branch/main/graph/badge.svg)](https://codecov.io/gh/platinummonkey/argot)
[![Crates.io](https://img.shields.io/crates/v/argot-cmd.svg)](https://crates.io/crates/argot-cmd)
[![docs.rs](https://docs.rs/argot-cmd/badge.svg)](https://docs.rs/argot-cmd)

---

## Overview

Argot models command-line interfaces as **structured command languages**, not just argument parsers. The command model is the single source of truth: it drives CLI help output, machine-readable schemas, Markdown documentation, and optional MCP tool exposure — all from the same data.

Argot prioritizes **agent usability and discoverability**. AI agents can query commands programmatically and receive structured JSON rather than scraping help text. Humans get a familiar, ergonomic CLI with prefix resolution, typo correction, and contextual help.

The design philosophy is: the CLI is one interface to the command model, not the source of truth.

---

## Quick Start

Add argot-cmd to your `Cargo.toml`:

```toml
[dependencies]
argot-cmd = "0.1"
```

Define a command, build a `Cli`, and call `run_env_args_and_exit()`:

```rust
use std::sync::Arc;
use argot_cmd::{Argument, Cli, Command, Flag};

fn main() {
    let deploy = Command::builder("deploy")
        .summary("Deploy a service to an environment")
        .argument(
            Argument::builder("env")
                .description("Target environment")
                .required()
                .build()
                .unwrap(),
        )
        .flag(
            Flag::builder("dry-run")
                .short('n')
                .description("Simulate without applying changes")
                .build()
                .unwrap(),
        )
        .handler(Arc::new(|parsed| {
            let env = parsed.arg("env").unwrap_or("dev");
            let dry = parsed.flag_bool("dry-run");
            println!("Deploying to {} (dry_run={})", env, dry);
            Ok(())
        }))
        .build()
        .unwrap();

    Cli::new(vec![deploy])
        .app_name("mytool")
        .version("1.0.0")
        .run_env_args_and_exit();
}
```

---

## Core Concepts

### Canonical Identity

Every command has a stable canonical name. All resolution, help output, and serialization use this name. Aliases and spellings both resolve to the canonical name, but are semantically distinct.

### Aliases

Aliases are advertised alternatives — they appear in `--help` output and participate in prefix matching:

```rust
Command::builder("deploy")
    .alias("release")
    .alias("ship")
    // ...
```

### Spellings

Spellings are silent corrections — typo variants or alternate capitalizations that resolve to the canonical command without being advertised:

```rust
Command::builder("deploy")
    .spelling("deply")    // silent typo correction
    .spelling("delpoy")   // another silent correction
    // ...
```

Spellings participate in exact matching but not prefix matching. They are never shown in help output.

### Semantic Aliases (Intent Discovery)

Semantic aliases are natural-language phrases that describe what a command does. They are used for intent-based discovery via `Registry::match_intent()` but are **not** shown in help output and do not participate in normal resolution:

```rust
Command::builder("deploy")
    .semantic_alias("release to production")
    .semantic_alias("push to environment")
    // ...
```

Use `.semantic_aliases(["...", "..."])` to set multiple at once.

---

## Building Commands

All builder methods consume and return `self` for chaining. Call `.build()` at the end; it returns `Result<Command, BuildError>`.

```rust
use std::sync::Arc;
use argot_cmd::{Argument, Command, Example, Flag};

let cmd = Command::builder("deploy")
    .alias("release")                          // shown in help, participates in prefix matching
    .alias("ship")
    .spelling("deply")                         // silent typo correction
    .summary("Deploy the application")         // one-line description
    .description("Deploys to the given env.")  // prose description
    .argument(
        Argument::builder("env")
            .description("Target environment")
            .required()
            .build()
            .unwrap(),
    )
    .flag(
        Flag::builder("dry-run")
            .short('n')
            .description("Simulate without changes")
            .build()
            .unwrap(),
    )
    .example(Example::new("deploy to staging", "mytool deploy staging"))
    .best_practice("Always deploy to staging before production")
    .anti_pattern("Do not deploy directly to production without validation")
    .subcommand(
        Command::builder("rollback")
            .summary("Roll back the last deployment")
            .build()
            .unwrap(),
    )
    .handler(Arc::new(|parsed| {
        println!("deploying to {}", parsed.args["env"]);
        Ok(())
    }))
    .build()
    .unwrap();
```

### Arbitrary Metadata

Attach structured application-specific metadata with `.meta()`. Values are `serde_json::Value` and are included in JSON serialization:

```rust
use serde_json::json;

let cmd = Command::builder("deploy")
    .meta("category", json!("infrastructure"))
    .meta("min_role", json!("ops"))
    .build()
    .unwrap();
```

---

## Arguments and Flags

### Arguments

Positional arguments are bound in declaration order. Use `Argument::builder(name)`:

```rust
use argot_cmd::Argument;

// Required positional argument
let env = Argument::builder("env")
    .description("Target environment")
    .required()
    .build()
    .unwrap();

// Optional with a default
let format = Argument::builder("format")
    .description("Output format")
    .default_value("text")
    .build()
    .unwrap();

// Variadic: consumes all remaining tokens (must be last)
let files = Argument::builder("files")
    .description("Files to process")
    .variadic()
    .build()
    .unwrap();
```

### Flags

Named flags can be boolean or value-taking. Use `Flag::builder(name)`:

```rust
use argot_cmd::Flag;

// Boolean flag
let verbose = Flag::builder("verbose")
    .short('v')
    .description("Enable verbose output")
    .build()
    .unwrap();

// Value-taking flag with a default
let output = Flag::builder("output")
    .short('o')
    .description("Output format")
    .takes_value()
    .default_value("text")
    .build()
    .unwrap();

// Value-taking flag with an allowed choices list
let format = Flag::builder("format")
    .takes_value()
    .choices(["json", "yaml", "text"])
    .description("Output format")
    .build()
    .unwrap();

// Required value-taking flag
let token = Flag::builder("token")
    .takes_value()
    .required()
    .description("API token")
    .build()
    .unwrap();

// Repeatable boolean flag: -v -v -v stores "3"
let debug = Flag::builder("verbose")
    .short('v')
    .repeatable()
    .build()
    .unwrap();

// Repeatable value-taking flag: --tag a --tag b stores ["a","b"]
let tag = Flag::builder("tag")
    .takes_value()
    .repeatable()
    .build()
    .unwrap();

// Environment variable fallback
let api_key = Flag::builder("api-key")
    .takes_value()
    .env("DEPLOY_API_KEY")
    .build()
    .unwrap();
```

Lookup order for a flag: CLI argv → environment variable (if `.env()` is set) → default value → required error.

### Mutually Exclusive Flags

Declare groups of flags where at most one may be provided per invocation:

```rust
use argot_cmd::{Command, Flag};

let cmd = Command::builder("export")
    .flag(Flag::builder("json").build().unwrap())
    .flag(Flag::builder("yaml").build().unwrap())
    .flag(Flag::builder("csv").build().unwrap())
    .exclusive(["json", "yaml", "csv"])
    .build()
    .unwrap();
```

Providing more than one flag from the group returns `ParseError::MutuallyExclusive { flags }`.

---

## The Cli Layer

`Cli` wires together `Registry`, `Parser`, and the render layer. It handles built-in behaviors so application code only needs to build commands and register handlers.

```rust
use argot_cmd::{Cli, Command};
use std::sync::Arc;

fn main() {
    Cli::new(vec![/* commands */])
        .app_name("mytool")
        .version("1.0.0")
        .run_env_args_and_exit();
}
```

### Built-in Behaviors

| Input | Behavior |
|-------|----------|
| `--help` / `-h` | Print help for the most-specific resolved command; return `Ok(())`. |
| `--version` / `-V` | Print `"<app_name> <version>"`; return `Ok(())`. |
| Empty argument list | Print the top-level command listing; return `Ok(())`. |
| Unrecognized command | Print error and best-effort help to stderr; return `Err(CliError::Parse(...))`. |

### Run Methods

```rust
// Read from std::env::args().skip(1)
cli.run_env_args()?;

// Read from an explicit iterator (useful in tests)
cli.run(["deploy", "--env", "prod"])?;

// Read from env args and exit the process (recommended for main())
cli.run_env_args_and_exit();

// Explicit args with process exit
cli.run_and_exit(["deploy", "prod"]);
```

---

## Agent Discovery

Enable a built-in `query` command that gives agents structured access to command metadata:

```rust
Cli::new(commands)
    .with_query_support()
    .run_env_args_and_exit();
```

Agents can then call:

```
# List all commands as JSON
mytool query commands

# Get structured metadata for a single command
mytool query deploy

# List examples for a command
mytool query examples deploy

# Prefix matching and aliases work too
mytool query dep
```

When a query is ambiguous (e.g. `mytool query dep` matches both `deploy` and `describe`), agents receive structured JSON rather than an error:

```json
{
  "error": "ambiguous",
  "input": "dep",
  "candidates": ["deploy", "describe"]
}
```

The `--json` flag is accepted for compatibility but has no effect; all query output is already JSON.

---

## Registry and Search

`Registry` owns the command tree and provides all lookup operations. Pass `registry.commands()` to `Parser::new` to wire it into the parse pipeline.

```rust
use argot_cmd::{Command, Registry};

let registry = Registry::new(vec![
    Command::builder("deploy").summary("Deploy the app").build().unwrap(),
    Command::builder("status").summary("Show status").build().unwrap(),
]);

// Exact canonical lookup
let cmd = registry.get_command("deploy").unwrap();

// Walk into subcommands by canonical path
let sub = registry.get_subcommand(&["remote", "add"]).unwrap();

// Examples for a command
let examples = registry.get_examples("deploy").unwrap();

// Case-insensitive substring search across name, summary, description
let results = registry.search("deploy");

// Depth-first iteration over all commands (including subcommands)
for entry in registry.iter_all_recursive() {
    println!("{} — {}", entry.path_str(), entry.command.summary);
}

// Serialize the entire tree to pretty-printed JSON (handlers excluded)
let json = registry.to_json().unwrap();

// Intent matching: score commands by how many phrase words appear in their
// combined text (canonical, aliases, semantic_aliases, summary, description)
let hits = registry.match_intent("release to production");
// Returns Vec<(&Command, u32)> sorted by score descending
```

`CommandEntry` carries the full path from the registry root:

```rust
entry.name()       // last segment: "add"
entry.path_str()   // dotted path: "remote.add"
entry.path         // Vec<String>: ["remote", "add"]
```

---

## Rendering

All render functions return `String`. None of them print directly; callers write the output wherever appropriate.

### Plain-text Help

```rust
use argot_cmd::render_help;

let help = render_help(&cmd);
// Sections: NAME, SUMMARY, DESCRIPTION, USAGE, ARGUMENTS, FLAGS,
//           SUBCOMMANDS, EXAMPLES, BEST PRACTICES, ANTI-PATTERNS
// Empty sections are omitted.
```

### Compact Command Listing

```rust
use argot_cmd::render_subcommand_list;

let listing = render_subcommand_list(registry.commands());
// Two-column output: "  canonical  summary"
```

### Markdown Documentation

```rust
use argot_cmd::render_markdown;

let md = render_markdown(&cmd);
// GitHub-flavored Markdown with ## headings, tables for arguments/flags,
// and fenced code blocks for examples.
```

### Full Registry Docs

```rust
use argot_cmd::render_docs;

let docs = render_docs(&registry);
// "# Commands" heading, table of contents with depth-based indentation,
// and per-command Markdown sections separated by "---".
```

### Disambiguation

```rust
use argot_cmd::render_ambiguity;

let msg = render_ambiguity("dep", &["deploy".to_string(), "describe".to_string()]);
```

### JSON Schema

Generate a JSON Schema (draft-07) suitable for agent tool definitions (OpenAI function calling, Anthropic tool use, MCP):

```rust
use argot_cmd::render::render_json_schema;

let schema = render_json_schema(&cmd).unwrap();
// Arguments become string properties; boolean flags become boolean properties;
// flags with choices get an "enum" constraint.
```

### Shell Completions

Generate tab-completion scripts for bash, zsh, or fish:

```rust
use argot_cmd::render::{render_completion, Shell};

let bash_script  = render_completion(Shell::Bash, "mytool", &registry);
let zsh_script   = render_completion(Shell::Zsh,  "mytool", &registry);
let fish_script  = render_completion(Shell::Fish, "mytool", &registry);
```

Source the script in your shell profile to enable tab-completion.

### Custom Renderer

Implement the `Renderer` trait and inject it with `Cli::with_renderer`:

```rust
use argot_cmd::{Cli, Command, render::Renderer};

struct UppercaseRenderer;

impl Renderer for UppercaseRenderer {
    fn render_help(&self, command: &Command) -> String {
        argot_cmd::render_help(command).to_uppercase()
    }
    fn render_markdown(&self, command: &Command) -> String {
        argot_cmd::render_markdown(command)
    }
    fn render_subcommand_list(&self, commands: &[Command]) -> String {
        argot_cmd::render_subcommand_list(commands)
    }
    fn render_ambiguity(&self, input: &str, candidates: &[String]) -> String {
        argot_cmd::render_ambiguity(input, candidates)
    }
}

let cli = Cli::new(vec![/* commands */])
    .with_renderer(UppercaseRenderer);
```

---

## Middleware

Implement `Middleware` to hook into the parse-and-dispatch lifecycle. Register it with `Cli::with_middleware`. Multiple middlewares are invoked in registration order.

All methods have default no-op implementations; override only what you need.

```rust
use argot_cmd::{Cli, Command, middleware::Middleware, ParsedCommand, parser::ParseError};

struct AuditLogger;

impl Middleware for AuditLogger {
    fn before_dispatch(
        &self,
        parsed: &ParsedCommand<'_>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        eprintln!("[audit] dispatching: {}", parsed.command.canonical);
        Ok(()) // return Err(...) to abort dispatch
    }

    fn after_dispatch(
        &self,
        parsed: &ParsedCommand<'_>,
        result: &Result<(), Box<dyn std::error::Error + Send + Sync>>,
    ) {
        match result {
            Ok(()) => eprintln!("[audit] ok: {}", parsed.command.canonical),
            Err(e)  => eprintln!("[audit] err: {}: {}", parsed.command.canonical, e),
        }
    }

    fn on_parse_error(&self, err: &ParseError) {
        eprintln!("[audit] parse error: {}", err);
    }
}

let cli = Cli::new(vec![/* commands */])
    .with_middleware(AuditLogger);
```

---

## Async Support

Enable the `async` feature to register async handlers and use `run_async`:

```toml
[dependencies]
argot-cmd = { version = "0.1", features = ["async"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

```rust
#[cfg(feature = "async")]
mod example {
    use std::sync::Arc;
    use argot_cmd::{Cli, Command};

    #[tokio::main]
    async fn main() {
        let cmd = Command::builder("deploy")
            .async_handler(Arc::new(|parsed| Box::pin(async move {
                println!("async deploy to {}", parsed.args.get("env").map(|s| s.as_str()).unwrap_or("?"));
                Ok(())
            })))
            .build()
            .unwrap();

        Cli::new(vec![cmd])
            .run_env_args_async_and_exit()
            .await;
    }
}
```

Dispatch priority when both handlers are registered: async handler takes precedence over the sync handler.

Async convenience methods mirror their sync equivalents:

| Method | Description |
|--------|-------------|
| `run_async(args)` | Parse and dispatch asynchronously. |
| `run_env_args_async()` | Same, reading from `std::env::args().skip(1)`. |
| `run_async_and_exit(args)` | Dispatch and exit the process. |
| `run_env_args_async_and_exit()` | Same, reading from env args. |

---

## MCP Transport

Enable the `mcp` feature to expose commands as MCP tools over a stdio JSON-RPC 2.0 transport:

```toml
[dependencies]
argot-cmd = { version = "0.1", features = ["mcp"] }
```

```rust
#[cfg(feature = "mcp")]
mod example {
    use argot_cmd::{Command, McpServer, Registry};

    fn main() {
        let registry = Registry::new(vec![
            Command::builder("ping")
                .summary("Ping the server")
                .build()
                .unwrap(),
        ]);

        McpServer::new(registry)
            .server_name("my-tool")
            .server_version("1.0.0")
            .serve_stdio()
            .unwrap();
    }
}
```

### Tool Naming Convention

Commands are exposed as MCP tools using a dash-joined path:

- Top-level `deploy` → tool name `deploy`
- Subcommand `service rollback` → tool name `service-rollback`
- Three levels `service deployment blue-green` → tool name `service-deployment-blue-green`

### Supported MCP Methods

| Method | Description |
|--------|-------------|
| `initialize` | Returns server name, version, and capabilities. |
| `tools/list` | Lists all commands as MCP tool definitions with JSON Schema input schemas. |
| `tools/call` | Dispatches a command by tool name, building a `ParsedCommand` from the JSON arguments. |

Notifications (requests without an `id`) receive no response per the JSON-RPC 2.0 specification.

---

## Derive Macro

Enable the `derive` feature to auto-generate `Command` definitions from struct attributes:

```toml
[dependencies]
argot-cmd = { version = "0.1", features = ["derive"] }
```

```rust
#[cfg(feature = "derive")]
mod example {
    use argot_cmd::ArgotCommand;

    #[derive(ArgotCommand)]
    #[argot(
        summary = "Deploy the application",
        alias = "d",
        best_practice = "Always dry-run first",
        anti_pattern = "Do not deploy to production without staging"
    )]
    struct Deploy {
        #[argot(positional, required, description = "Target environment")]
        env: String,

        #[argot(flag, short = 'n', description = "Simulate without changes")]
        dry_run: bool,

        #[argot(flag, takes_value, description = "Output format", default = "text")]
        output: String,
    }

    fn main() {
        let cmd = Deploy::command();
        assert_eq!(cmd.canonical, "deploy");
        assert_eq!(cmd.aliases, vec!["d"]);
    }
}
```

### Struct-Level Attributes

| Key | Type | Description |
|-----|------|-------------|
| `canonical = "name"` | string | Override the command name. Default: struct name in kebab-case (`DeployApp` → `deploy-app`). |
| `summary = "text"` | string | One-line summary. |
| `description = "text"` | string | Long prose description. |
| `alias = "a"` | string | Add an alias. Repeat the attribute to add more. |
| `best_practice = "text"` | string | Add a best-practice tip. Repeatable. |
| `anti_pattern = "text"` | string | Add an anti-pattern warning. Repeatable. |

### Field-Level Attributes

Fields without `#[argot(...)]` are skipped. Annotated fields must include either `positional` or `flag`.

| Key | Description |
|-----|-------------|
| `positional` | Treat as a positional `Argument`. |
| `flag` | Treat as a named `Flag`. |
| `required` | Mark as required. |
| `short = 'c'` | Short character for a flag. |
| `takes_value` | Flag consumes the next token as its value. |
| `description = "text"` | Human-readable description. |
| `default = "value"` | Default value string. |

Name conventions: struct names use `CamelCase` → `kebab-case`; field names use `snake_case` → `kebab-case` (e.g., `dry_run` → `dry-run`).

---

## Fuzzy Search

Enable the `fuzzy` feature to search commands with the skim fuzzy-matching algorithm:

```toml
[dependencies]
argot-cmd = { version = "0.1", features = ["fuzzy"] }
```

```rust
#[cfg(feature = "fuzzy")]
mod example {
    use argot_cmd::{Command, Registry};

    fn main() {
        let registry = Registry::new(vec![
            Command::builder("deploy").summary("Deploy a service").build().unwrap(),
            Command::builder("describe").summary("Describe a resource").build().unwrap(),
        ]);

        // Returns Vec<(&Command, i64)> sorted by score descending (best match first)
        let results = registry.fuzzy_search("dep");
        if let Some((cmd, score)) = results.first() {
            println!("best match: {} (score {})", cmd.canonical, score);
        }
    }
}
```

Fuzzy search covers the canonical name, summary, and description fields. Commands with no match are excluded from the results.

---

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `async` | `AsyncHandlerFn`, `Cli::run_async()`, and async-family entry points | no |
| `derive` | `#[derive(ArgotCommand)]` proc-macro from `argot-cmd-derive` | no |
| `fuzzy` | `Registry::fuzzy_search()` via `fuzzy-matcher` (skim algorithm) | no |
| `mcp` | `McpServer` stdio transport (Model Context Protocol) | no |

---

## Error Handling

Argot uses structured error types via `thiserror`. All fallible operations return `Result<T, E>`.

### `BuildError`

Returned by `CommandBuilder::build()`, `ArgumentBuilder::build()`, and `FlagBuilder::build()`.

Common variants: `EmptyCanonical`, `AliasEqualsCanonical`, `DuplicateAlias`, `DuplicateFlagName`, `DuplicateShortFlag`, `DuplicateArgumentName`, `DuplicateSubcommandName`, `VariadicNotLast`, `EmptyChoices`, `ExclusiveGroupTooSmall`, `ExclusiveGroupUnknownFlag`.

### `ResolveError`

Returned by `Resolver::resolve()`.

```rust
use argot_cmd::ResolveError;

match resolver.resolve("dep") {
    Ok(cmd) => { /* unique match */ }
    Err(ResolveError::Ambiguous { input, candidates }) => {
        // "dep" matched multiple commands
    }
    Err(ResolveError::Unknown { input, suggestions }) => {
        // no match; suggestions contains up to 3 near-miss canonical names
    }
}
```

### `ParseError`

Returned by `Parser::parse()`.

Common variants: `NoCommand`, `Resolve(ResolveError)`, `MissingArgument`, `UnexpectedArgument`, `MissingFlag`, `FlagMissingValue`, `UnknownFlag`, `UnknownSubcommand`, `InvalidChoice`, `MutuallyExclusive`.

### `CliError`

Returned by `Cli::run()`.

| Variant | Description |
|---------|-------------|
| `Parse(ParseError)` | Parse failed; error and best-effort help have already been printed to stderr. |
| `NoHandler(String)` | The resolved command has no handler registered. |
| `Handler(Box<dyn Error>)` | The registered handler returned an error. |

### `QueryError`

Returned by `Registry::to_json()`.

| Variant | Description |
|---------|-------------|
| `Serialization(serde_json::Error)` | JSON serialization failed. |

---

## Design Principles

**Single Source of Truth** — the command model drives all outputs: help text, Markdown docs, JSON schemas, and MCP tool definitions. Manual help strings are not needed.

**Deterministic Behavior** — parsing and resolution are predictable. Prefix matching is unambiguous; ambiguity is surfaced as an explicit error or structured JSON rather than a guess.

**Explicit Over Magical** — no hidden behavior. Aliases are declared, spellings are declared, exclusivity groups are declared. Nothing is inferred from field types or naming conventions unless you opt in via `#[derive(ArgotCommand)]`.

**Discoverability** — commands are easy to explore programmatically. Agents never need to scrape help text; the `query` command and `Registry` API provide structured access to all metadata.

**Idiomatic Rust** — ownership and lifetimes are explicit; all fallible operations return `Result`; optional integrations are gated behind feature flags.

---

## Security: Prompt Injection Defense

Command metadata fields — `summary`, `description`, `examples`, `best_practices`, `anti_patterns` — flow directly into JSON query output (`registry.to_json()`), skill files, and MCP tool definitions consumed by AI agents. This is standard behavior for any system that builds prompts from structured data, and it carries the same prompt injection risk.

### The Risk

If those strings come from user-controlled or external sources, an attacker who can influence the data can embed instructions like `"Ignore previous instructions and..."` into a command description. That string is then passed verbatim to any LLM that consumes the tool's output.

### When You Are at Risk

- Loading command definitions from a config file, database, or API response
- Using `Command::builder("...").description(user_provided_string)` where `user_provided_string` comes from external input
- Calling `registry.to_json()` or rendering skill files when metadata was sourced from external input
- Passing command help text or JSON query output directly into an LLM context without review

### Mitigation Strategies

**Keep metadata static.** The safest pattern is defining all command metadata as Rust string literals compiled into the binary. Static metadata cannot be tampered with at runtime.

**Treat metadata as untrusted at system boundaries.** Validate and sanitize strings before passing them to `Command::builder`. Do not pass raw external strings directly into metadata fields.

**Strip control characters and suspicious sequences.** A minimal sanitization pass before building a command:

```rust
fn sanitize_metadata(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect()
}
```

This removes null bytes, escape sequences, and other non-printable characters while preserving newlines for multi-line descriptions.

**Use `InputValidator` middleware to reject adversarial flag values at dispatch time.** Note that `InputValidator` validates flag and argument values supplied at invocation, not the command metadata strings themselves. It is a complementary defense, not a replacement for sanitizing metadata at build time.

**Audit metadata sources.** If external data must flow into command metadata, log it and review it. Treat it the same as any other user-controlled input that reaches a security boundary.

### What Argot Does Not Do

Argot does not automatically sanitize metadata strings. It stores and serializes whatever strings are provided. Sanitizing externally-sourced strings before building commands is the application author's responsibility.

---

## MSRV

Minimum Supported Rust Version: **1.94.0**

---

## License

MIT
