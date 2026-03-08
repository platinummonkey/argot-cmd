# Argot

<!-- Badges -->
![CI](https://github.com/platinummonkey/argot/actions/workflows/ci.yml/badge.svg)
![Coverage](https://codecov.io/gh/platinummonkey/argot/branch/main/graph/badge.svg)

Argot is an **agent-first command interface framework for Rust**.

Argot models command-line interfaces as **structured languages**, not just argument parsers.
The command model becomes a source of truth that can serve:

- humans using a CLI
- AI agents discovering commands
- automation systems invoking tools
- documentation generation
- optional tool exposure (such as MCP)

Argot prioritizes **agent usability and discoverability**, while still providing a clear and familiar CLI experience for humans.

---

# Why Argot Exists

Traditional CLI frameworks focus on parsing `argv`.

That works well for humans typing commands, but it creates friction for automation and AI systems because:

- commands are only described in help text
- examples are not machine-readable
- safe usage patterns are undocumented or informal
- agents must scrape or guess command capabilities

Argot solves this by modeling the CLI as **structured command metadata**.

The CLI becomes **a projection of the command model**, not the source of truth.

---

# Key Features

## Canonical Command Identity

Each command has a stable canonical identity.

Aliases and alternative spellings resolve to that identity.

Example:

```rust
Command {
    canonical: "deploy",
    aliases: &["release", "ship", "push"],
    ..Default::default()
}
```

## Structured Metadata

Commands carry rich metadata for both humans and agents:

```rust
Command {
    canonical: "deploy",
    summary: "Deploy a service to an environment",
    description: "Deploy builds and releases a service artifact to the specified environment.",
    examples: vec![
        Example {
            command: "deploy api --env staging",
            description: "Deploy API service to staging",
        },
    ],
    best_practices: vec!["Deploy to staging before production"],
    anti_patterns: vec!["Deploy directly to production without validation"],
    ..Default::default()
}
```

## Agent Discoverability

Agents can query command metadata programmatically without parsing help text:

```
tool query deploy --json
```

```json
{
  "canonical": "deploy",
  "summary": "Deploy a service to an environment",
  "arguments": ["service"],
  "flags": ["env"],
  "examples": ["deploy api --env prod"]
}
```

## Ambiguity Handling

Argot surfaces ambiguity rather than guessing:

```
Ambiguous command "dep".

Did you mean:
  deploy
  describe
  delete
```

Agents receive a structured version of this response.

---

# Getting Started

Add Argot to your `Cargo.toml`:

```toml
[dependencies]
argot = "0.1"
```

Define a command and run it:

```rust
use std::sync::Arc;
use argot::{Argument, Cli, Command, Flag};

let deploy = Command::builder("deploy")
    .summary("Deploy a service to an environment")
    .argument(Argument::builder("env").required().description("Target environment").build().unwrap())
    .flag(Flag::builder("dry-run").description("Simulate without applying changes").build().unwrap())
    .handler(Arc::new(|parsed| {
        let env = parsed.arg("env").unwrap_or("dev");
        let dry = parsed.flag_bool("dry-run");
        println!("Deploying to {} (dry_run={})", env, dry);
        Ok(())
    }))
    .build()
    .unwrap();

fn main() {
    Cli::new(vec![deploy])
        .app_name("mytool")
        .version("1.0.0")
        .run_env_args_and_exit();
}
```

---

## Agent Discovery

Argot is designed so AI agents can discover your tool's commands without scraping help text.

Enable the built-in `query` command with `with_query_support()`:

```rust
Cli::new(commands)
    .with_query_support()
    .run_env_args_and_exit();
```

Agents can then call:

```
# List all commands as JSON
mytool query commands

# Get structured metadata for one command
mytool query deploy
mytool query deploy --json
```

The output is the full serialized `Command` struct including arguments, flags, examples, best practices, and anti-patterns — everything an agent needs to use a command correctly.

---

### Mutually exclusive flags

```rust
Command::builder("export")
    .flag(Flag::builder("json").build().unwrap())
    .flag(Flag::builder("yaml").build().unwrap())
    .flag(Flag::builder("csv").build().unwrap())
    .exclusive(["json", "yaml", "csv"])   // at most one may be set
    .build()
    .unwrap()
```

Providing more than one flag from the group returns `ParseError::MutuallyExclusive { flags }`.

---

# Feature Flags

| Feature  | Description | Default |
|----------|-------------|---------|
| `derive` | `#[derive(ArgotCommand)]` proc-macro | no |
| `fuzzy`  | `Registry::fuzzy_search()` via `fuzzy-matcher` | no |
| `mcp`    | `McpServer` stdio transport (Model Context Protocol) | no |

# MSRV

Minimum Supported Rust Version: **1.75.0**

---

# Optional: MCP Transport

Argot can optionally expose commands via MCP without affecting core packages:

```toml
[dependencies]
argot = { version = "0.1", features = ["mcp"] }
```

---

# Guides

- [Cookbook](docs/cookbook.md) — 10 recipes for common patterns
- [Error Handling](docs/error-handling.md) — how to handle `ParseError`, `ResolveError`, and `CliError`
- [Validation Patterns](docs/validation-patterns.md) — built-in and handler-level validation
- [Middleware Guide](docs/middleware-guide.md) — hooks for logging, auditing, and request interception
- [MCP Setup](docs/mcp-setup.md) — exposing commands to AI agents via Model Context Protocol

---

# Design Principles

- **Single Source of Truth** — the command model drives all outputs
- **Deterministic Behavior** — parsing and resolution are predictable
- **Explicit Over Magical** — no hidden behavior or implicit guessing
- **Discoverability** — commands are easy to explore programmatically

---

# License

MIT
