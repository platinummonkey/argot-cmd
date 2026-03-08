# MCP Setup Guide

This guide covers how to expose your argot command registry to AI agents using the Model Context Protocol (MCP).

---

## What MCP Is

[Model Context Protocol](https://modelcontextprotocol.io) is an open standard that lets AI agents (such as Claude) discover and invoke tools provided by external processes. The agent sends JSON-RPC 2.0 requests over stdio; the server responds with tool listings and execution results.

Argot's `McpServer` maps your `Registry` directly to MCP tools, so an AI agent gets the same structured command metadata — arguments, flags, examples, best practices, anti-patterns — that a human sees in help text.

---

## Adding the Feature

The MCP transport is behind a feature flag to keep the default dependency footprint small.

```toml
[dependencies]
argot = { version = "0.1", features = ["mcp"] }
```

All MCP types are gated on `#[cfg(feature = "mcp")]`.

---

## Basic Setup

Build a `Registry`, wrap it in `McpServer`, and call `serve_stdio()`. The server blocks until EOF on stdin.

```rust
#[cfg(feature = "mcp")]
fn main() {
    use argot::{Command, McpServer, Registry};

    let registry = Registry::new(vec![
        Command::builder("ping")
            .summary("Check server connectivity")
            .build()
            .unwrap(),
    ]);

    McpServer::new(registry)
        .server_name("my-tool")
        .server_version("1.0.0")
        .serve_stdio()
        .unwrap_or_else(|e| {
            eprintln!("MCP server error: {}", e);
            std::process::exit(1);
        });
}
```

`serve_stdio()` reads newline-delimited JSON from stdin and writes newline-delimited JSON to stdout. It returns when stdin reaches EOF.

---

## Annotating Commands for Agents

Agents benefit most when commands carry rich metadata. Use `best_practice()`, `anti_pattern()`, `summary()`, and `description()` so the MCP tool description communicates safe and unsafe usage patterns.

```rust
use std::sync::Arc;
use argot::{Argument, Command, Example, Flag};

fn build_migrate() -> Command {
    Command::builder("migrate")
        .summary("Run database migrations")
        .description("Applies pending schema migrations to the target database.")
        .argument(
            Argument::builder("database")
                .description("Database name or connection alias")
                .required()
                .build()
                .unwrap(),
        )
        .flag(
            Flag::builder("dry-run")
                .short('n')
                .description("Show migrations that would run without applying them")
                .build()
                .unwrap(),
        )
        .flag(
            Flag::builder("version")
                .short('v')
                .description("Target schema version (default: latest)")
                .takes_value()
                .build()
                .unwrap(),
        )
        .example(Example::new("migrate to latest", "migrate production"))
        .example(Example::new("dry-run on staging", "migrate staging --dry-run"))
        .best_practice("Always run with --dry-run on production before applying")
        .best_practice("Target staging first to validate migration correctness")
        .anti_pattern("Do not run migrations during peak traffic hours")
        .anti_pattern("Do not skip versions by specifying a target version out of sequence")
        .handler(Arc::new(|parsed| {
            let db      = parsed.arg("database").unwrap_or("unknown");
            let dry_run = parsed.flag_bool("dry-run");
            let version = parsed.flag("version").unwrap_or("latest");
            if dry_run {
                eprintln!("[DRY RUN] Would migrate {} to {}", db, version);
            } else {
                eprintln!("Migrating {} to {} ...", db, version);
            }
            Ok(())
        }))
        .build()
        .unwrap()
}
```

The `best_practices` and `anti_patterns` fields are included in the MCP tool's `description`, giving agents explicit guidance on when and how to invoke the tool.

---

## How Commands Map to MCP Tools

`McpServer` converts each command in the registry to one MCP tool:

| Command structure | Tool name |
|-------------------|-----------|
| Top-level command `deploy` | `"deploy"` |
| Subcommand `service rollback` | `"service-rollback"` |
| Three-level `service deployment blue-green` | `"service-deployment-blue-green"` |

The input schema is derived directly from the command definition:

- Required positional arguments → `"type": "string"`, added to `required`
- Optional positional arguments → `"type": "string"`, not in `required`
- Value-taking flags → `"type": "string"`
- Boolean flags (no value) → `"type": "boolean"`
- Required flags → added to `required`

Example `tools/list` response for the `migrate` command above (trimmed):

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "tools": [{
      "name": "migrate",
      "description": "Run database migrations",
      "inputSchema": {
        "type": "object",
        "properties": {
          "database": {"type": "string", "description": "Database name or connection alias"},
          "dry-run":  {"type": "boolean", "description": "Show migrations that would run without applying them"},
          "version":  {"type": "string",  "description": "Target schema version (default: latest)"}
        },
        "required": ["database"]
      }
    }]
  }
}
```

---

## Running as a Standalone MCP Server

The simplest deployment is a dedicated binary that does nothing but serve MCP:

```rust
// src/bin/mcp_server.rs
#[cfg(feature = "mcp")]
fn main() {
    use argot::{McpServer, Registry};
    // Import your command builders from the main crate.
    use my_tool::commands::build_registry;

    let registry: Registry = build_registry();

    McpServer::new(registry)
        .server_name("my-tool")
        .server_version(env!("CARGO_PKG_VERSION"))
        .serve_stdio()
        .unwrap_or_else(|e| {
            eprintln!("MCP server error: {}", e);
            std::process::exit(1);
        });
}

#[cfg(not(feature = "mcp"))]
fn main() {
    eprintln!("Rebuild with --features mcp to enable the MCP server.");
    std::process::exit(1);
}
```

Build with:

```sh
cargo build --release --features mcp --bin mcp_server
```

---

## Testing the Server

Use `McpServer::serve` with a `Cursor` reader and a `Vec<u8>` writer to test without spawning a process:

```rust
#[cfg(feature = "mcp")]
#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use argot::{Command, McpServer, Registry};
    use serde_json::Value;

    fn make_registry() -> Registry {
        Registry::new(vec![
            Command::builder("ping")
                .summary("Ping the server")
                .build()
                .unwrap(),
        ])
    }

    #[test]
    fn test_tools_list() {
        let server = McpServer::new(make_registry())
            .server_name("test-server")
            .server_version("0.0.1");

        let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\",\"params\":{}}\n";
        let reader = Cursor::new(input.as_bytes().to_vec());
        let mut output = Vec::new();

        server.serve(reader, &mut output).unwrap();

        let response: Value = serde_json::from_slice(&output).unwrap();
        let tools = response["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"ping"), "expected ping tool: {:?}", names);
    }

    #[test]
    fn test_initialize() {
        let server = McpServer::new(make_registry())
            .server_name("test-server")
            .server_version("0.0.1");

        let input = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"initialize\",",
            "\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},",
            "\"clientInfo\":{\"name\":\"test\",\"version\":\"1.0\"}}}\n"
        );
        let reader = Cursor::new(input.as_bytes().to_vec());
        let mut output = Vec::new();

        server.serve(reader, &mut output).unwrap();

        let response: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(response["result"]["serverInfo"]["name"], "test-server");
        assert_eq!(response["result"]["serverInfo"]["version"], "0.0.1");
    }
}
```

---

## Integration with Claude Desktop and Other MCP Clients

Claude Desktop reads server definitions from its config file. Add your server binary to the `mcpServers` map:

```json
{
  "mcpServers": {
    "my-tool": {
      "command": "/path/to/my-tool-mcp-server",
      "args": []
    }
  }
}
```

The config file location:
- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`

Other MCP clients follow the same convention: provide the path to the server binary and any required arguments. The client launches the process and communicates over its stdin/stdout.

---

## Combining CLI and MCP in One Binary

You can ship one binary that behaves as a normal CLI when invoked by humans and as an MCP server when invoked by an agent runtime. Trigger MCP mode with an environment variable or a dedicated sub-command:

```rust
use argot::{Cli, Command};
use std::sync::Arc;

fn build_commands() -> Vec<Command> {
    vec![
        Command::builder("deploy")
            .summary("Deploy the service")
            .handler(Arc::new(|_| { println!("deploying"); Ok(()) }))
            .build()
            .unwrap(),
    ]
}

fn main() {
    // If MCP_MODE is set, run as MCP server.
    #[cfg(feature = "mcp")]
    if std::env::var("MCP_MODE").is_ok() {
        use argot::{McpServer, Registry};
        McpServer::new(Registry::new(build_commands()))
            .server_name("my-tool")
            .server_version(env!("CARGO_PKG_VERSION"))
            .serve_stdio()
            .unwrap_or_else(|e| {
                eprintln!("MCP server error: {}", e);
                std::process::exit(1);
            });
        return;
    }

    // Otherwise, run as normal CLI.
    Cli::new(build_commands())
        .app_name("my-tool")
        .version(env!("CARGO_PKG_VERSION"))
        .run_env_args_and_exit();
}
```

In the Claude Desktop config, set the env var so the agent always gets MCP mode:

```json
{
  "mcpServers": {
    "my-tool": {
      "command": "/path/to/my-tool",
      "args": [],
      "env": {
        "MCP_MODE": "1"
      }
    }
  }
}
```

Human users invoke the binary normally without `MCP_MODE`, getting the standard CLI experience.
