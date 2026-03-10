# ARCHITECTURE.md

# Argot Architecture

Argot is an **agent-first command interface framework for Rust**.

Unlike traditional CLI frameworks that focus on parsing command-line arguments, Argot models a CLI as a **structured command language**. This allows the same command interface to support:

* humans using a CLI
* AI agents discovering commands
* automation systems invoking commands
* optional tool exposure through protocols such as MCP

The CLI is treated as **one interface** to the command model rather than the source of truth.

---

# Design Goals

Argot aims to provide:

### Agent Discoverability

Agents should be able to programmatically determine:

* what commands exist
* what each command does
* what arguments and flags are available
* safe usage examples
* recommended practices

Agents should **never need to scrape help text**.

---

### Stable Command Identity

Each command has a canonical identity.

Aliases and alternate spellings resolve to the same canonical command.

Example:

```
canonical: deploy
aliases: [release, ship, push]
```

Internally the system always operates on the canonical name.

---

### Structured Command Metadata

Commands include structured metadata for both humans and agents.

Typical metadata includes:

* summary
* description
* arguments
* flags
* examples
* best practices
* anti-patterns

This metadata becomes the single source of truth used to generate:

* CLI help output
* machine-readable command schemas
* documentation
* tool definitions

---

### Human-Friendly CLI

Although Argot prioritizes agent UX, the CLI should still feel familiar and ergonomic.

Design inspiration comes from:

* `clap` — the dominant Rust CLI argument parser

Argot should follow similar conventions for:

* command hierarchy
* flags
* help output
* examples

However, these outputs should be generated from the structured command model rather than written by hand.

---

# System Architecture

Argot is organized into several logical layers.

```
model
resolver
parser
query
render
cli
input_validation
transport
```

Each layer should remain loosely coupled.

---

# Model Layer

The **model** layer defines the core command structures.

These structures represent the canonical command surface.

Example:

```rust
#[derive(Debug, Clone)]
pub struct Command {
    pub canonical: String,
    pub aliases: Vec<String>,
    pub spellings: Vec<String>,

    pub summary: String,
    pub description: String,

    pub arguments: Vec<Argument>,
    pub flags: Vec<Flag>,

    pub examples: Vec<Example>,

    pub best_practices: Vec<String>,
    pub anti_patterns: Vec<String>,

    /// When `true`, help output, Markdown docs, and JSON schema all surface a
    /// warning that this command modifies state. Set via `.mutating()` on the
    /// builder. `Cli::warn_missing_dry_run(true)` will emit an advisory warning
    /// at dispatch time when this field is `true` and no `--dry-run` flag is
    /// defined.
    pub mutating: bool,
}

#[derive(Debug, Clone)]
pub struct Example {
    pub command: String,
    pub description: String,
}
```

The model layer should contain **no parsing logic or CLI behavior**.

---

# Resolver Layer

The resolver maps user input to canonical commands.

Responsibilities include:

* alias resolution
* spelling normalization
* ambiguity detection

Example:

```
release → deploy
ship → deploy
```

If multiple commands match an input, the resolver should surface the ambiguity rather than guessing.

---

# Parser Layer

The parser converts argv input into structured command execution.

Responsibilities include:

* tokenizing CLI input
* matching commands
* parsing arguments
* validating flags

The parser should rely on the resolver to determine canonical commands.

---

# Query Layer

The query layer provides programmatic access to command metadata.

Example queries:

```
list commands
get command deploy
get examples deploy
```

This layer exists primarily for agents and automation.

The CLI may expose these queries through commands such as:

```
tool query commands
tool query deploy
```

---

# Render Layer

The render layer produces human-readable outputs.

Examples include:

* CLI help text
* Markdown documentation
* terminal summaries
* skill files — structured Markdown for AI agent contexts (e.g. `.claude/commands/`),
  with optional YAML frontmatter via `SkillFrontmatter`
* JSON Schema suitable for LLM tool-call definitions (OpenAI function calling, MCP)
* shell completion scripts (bash, zsh, fish)

All rendering is generated from the command model.

Manual help text should be avoided whenever possible.

---

# CLI Layer

The CLI layer provides a human interface for executing commands.

Responsibilities include:

* reading argv
* invoking the parser
* formatting output
* presenting help

The CLI layer should remain thin.

Most logic should exist in earlier layers.

---

# Input Validation Layer

The `input_validation` module (`src/input_validation.rs`) provides opt-in runtime validation of argument and flag values supplied to a parsed command.

`InputValidator` implements `Middleware` and is wired in via `Cli::with_middleware`. It validates every `ParsedCommand` field before the handler is invoked, aborting dispatch and returning an error on the first violation.

Available checks (each opt-in; all enabled by `InputValidator::strict()`):

| Check | Enabled by | What it detects |
|-------|-----------|-----------------|
| Path traversal | `.check_path_traversal()` | `../`, `..\`, values starting with `/` or `~` |
| Control characters | `.check_control_chars()` | ASCII 0x00–0x1F / 0x7F (tab and newline are permitted) |
| Query injection | `.check_query_injection()` | `?` or `&key=val` patterns |
| URL encoding | `.check_url_encoding()` | `%XX` percent-encoded sequences |

`InputValidator` validates values supplied at invocation. It does not sanitize the command metadata strings (summary, description, etc.) that are defined at build time.

---

# Transport Layer

The transport layer exposes commands through machine interfaces.

Examples include:

* JSON APIs
* tool discovery endpoints
* MCP servers

MCP support is optional and should not introduce dependencies into the core packages.

It should be gated behind a Cargo feature flag:

```toml
[features]
mcp = ["dep:some-mcp-crate"]
```

---

# Command Example

Below is an example command definition using the builder pattern.

```rust
use argot::{Command, Example};

let deploy = Command::builder("deploy")
    .aliases(&["release", "ship"])
    .summary("Deploy a service to an environment")
    .description("Deploy builds and releases a service artifact to the specified environment.")
    .example(Example::new(
        "deploy api --env staging",
        "Deploy API service to staging",
    ))
    .example(Example::new(
        "deploy api --env prod",
        "Deploy API service to production",
    ))
    .best_practice("Deploy to staging before production")
    .anti_pattern("Deploy directly to production without validation")
    .build();
```

---

# CLI Example

Human usage:

```
tool deploy api --env prod
```

Help output:

```
deploy

Deploy a service to an environment.

Examples:
  deploy api --env staging
  deploy api --env prod
```

---

# Agent Query Example

Agents should be able to request structured data.

Example:

```
tool query deploy --json
```

Example output:

```json
{
  "canonical": "deploy",
  "summary": "Deploy a service",
  "arguments": ["service"],
  "flags": ["env"],
  "examples": [
    "deploy api --env prod"
  ]
}
```

---

# Ambiguity Handling

Argot should prefer **explicit resolution over guessing**.

Example ambiguous input:

```
dep
```

If this could match:

```
deploy
describe
delete
```

Argot should return:

```
Ambiguous command "dep".

Did you mean:
  deploy
  describe
  delete
```

Agents should receive a structured version of this response.

---

# Error Handling

Argot uses structured error types via `thiserror`.

```rust
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("unknown command: {0}")]
    Unknown(String),

    #[error("ambiguous command \"{input}\": could match {candidates:?}")]
    Ambiguous {
        input: String,
        candidates: Vec<String>,
    },
}
```

Library code should never `unwrap()`. All fallible operations return `Result<T, E>`.

---

# Design Principles

Argot follows several guiding principles.

### Single Source of Truth

The command model drives all outputs.

### Deterministic Behavior

Parsing and resolution should be predictable.

### Explicit Over Magical

Avoid hidden behavior or implicit command guessing.

### Discoverability

Commands should be easy to explore programmatically.

### Idiomatic Rust

Follow Rust conventions: ownership, `Result`-based error handling, `derive` macros, and feature flags for optional integrations.

---

# Future Extensions

Potential future capabilities include:

* interactive command exploration
* IDE integrations (via LSP or rust-analyzer plugins)
* richer schema output (OpenAI function-calling format, Anthropic tool-use format)

These should build on the existing command model rather than replace it.
