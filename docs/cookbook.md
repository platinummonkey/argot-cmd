# Argot Cookbook

Ten self-contained recipes for common argot patterns. Each recipe can be read independently. Code examples use only the public API exactly as it exists in the source.

---

# Recipe 1: Git-like Nested Subcommands

**Problem:** You want a 3-level command tree — `tool remote add <name> <url>`, `tool remote remove <name>`, `tool remote list` — and need to parse and dispatch them correctly.

```rust
use std::sync::Arc;
use argot::{Argument, Command, Parser, Registry, render_help, render_subcommand_list};

fn build_registry() -> Registry {
    let add = Command::builder("add")
        .summary("Add a named remote")
        .argument(Argument::builder("name").required().build().unwrap())
        .argument(Argument::builder("url").required().build().unwrap())
        .handler(Arc::new(|parsed| {
            let name = parsed.arg("name").unwrap_or("");
            let url  = parsed.arg("url").unwrap_or("");
            println!("Added remote '{}' -> {}", name, url);
            Ok(())
        }))
        .build().unwrap();

    let remove = Command::builder("remove")
        .alias("rm")
        .summary("Remove a named remote")
        .argument(Argument::builder("name").required().build().unwrap())
        .handler(Arc::new(|parsed| {
            println!("Removed remote '{}'", parsed.arg("name").unwrap_or(""));
            Ok(())
        }))
        .build().unwrap();

    let list = Command::builder("list")
        .summary("List configured remotes")
        .handler(Arc::new(|_| { println!("origin  https://github.com/example/repo"); Ok(()) }))
        .build().unwrap();

    let remote = Command::builder("remote")
        .summary("Manage tracked repositories")
        .subcommand(add)
        .subcommand(remove)
        .subcommand(list)
        .build().unwrap();

    Registry::new(vec![remote])
}

fn main() {
    let registry = build_registry();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();

    if argv.is_empty() {
        print!("{}", render_subcommand_list(registry.commands()));
        return;
    }

    let parser = Parser::new(registry.commands());
    match parser.parse(&argv) {
        Ok(parsed) => {
            // Use parsed.command.canonical — there is no path_str() on ParsedCommand.
            println!("dispatching: {}", parsed.command.canonical);
            match &parsed.command.handler {
                Some(h) => h(&parsed).unwrap(),
                None    => print!("{}", render_help(parsed.command)),
            }
        }
        Err(e) => eprintln!("error: {}", e),
    }
}
```

**Key points:**
- Subcommands are attached via `.subcommand()` on the parent builder. The parser walks the tree automatically.
- `ParsedCommand` does not expose a full path string; use `parsed.command.canonical` to get the resolved leaf command name.
- Bare invocation of `remote` (no subcommand) dispatches to the `remote` command, which has no handler, so `render_help` is the right fallback.
- The `"rm"` alias on `remove` lets `tool remote rm origin` resolve identically to `tool remote remove origin`.

---

# Recipe 2: Typed Argument Access

**Problem:** The `serve --port 8080 --workers 4` command delivers all values as `&str`. You need `u16` and `u32` with clean error messages when the user passes a non-numeric value.

```rust
use std::sync::Arc;
use argot::{Argument, Command, Flag, Cli};

fn build_serve() -> Command {
    Command::builder("serve")
        .summary("Start the HTTP server")
        .flag(
            Flag::builder("port")
                .short('p')
                .description("TCP port to listen on (1–65535)")
                .takes_value()
                .default_value("8080")
                .build().unwrap(),
        )
        .flag(
            Flag::builder("workers")
                .short('w')
                .description("Number of worker threads")
                .takes_value()
                .default_value("4")
                .build().unwrap(),
        )
        .handler(Arc::new(|parsed| {
            // Parse flag values into typed numbers.
            let port: u16 = parsed
                .flag("port")
                .unwrap_or("8080")
                .parse()
                .map_err(|_| "port must be an integer between 1 and 65535")?;

            let workers: u32 = parsed
                .flag("workers")
                .unwrap_or("4")
                .parse()
                .map_err(|_| "workers must be a positive integer")?;

            if port == 0 {
                return Err("port 0 is not allowed".into());
            }

            println!("Listening on :{} with {} workers", port, workers);
            Ok(())
        }))
        .build().unwrap()
}

fn main() {
    let cli = Cli::new(vec![build_serve()])
        .app_name("myapp")
        .version("1.0.0");
    if let Err(e) = cli.run_env_args() {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}
```

**Key points:**
- `parsed.flag("name")` returns `Option<&str>`. Chain `.parse::<T>()` directly; `.map_err(|_| "message")?` converts the `ParseIntError` into the handler's `Box<dyn Error>`.
- Defaults set via `.default_value()` mean `parsed.flag("port")` always returns `Some(...)` once the parser has run, making the `.unwrap_or` fallback redundant but defensive.
- Range validation (port > 0) belongs in the handler because the parser has no notion of numeric ranges.

---

# Recipe 3: Configuration via Env Vars and CLI Override

**Problem:** A `deploy` command needs `--token` from `$DEPLOY_TOKEN`, `--env` restricted to `["prod", "staging", "dev"]` with a default of `"dev"`, and `--dry-run` as a bool. You want the parser to handle all of this before the handler runs.

```rust
use std::sync::Arc;
use argot::{Argument, Command, Flag, Cli};

fn build_deploy() -> Command {
    Command::builder("deploy")
        .summary("Deploy a service to an environment")
        .argument(
            Argument::builder("service")
                .description("Service name to deploy")
                .required()
                .build().unwrap(),
        )
        .flag(
            Flag::builder("token")
                .description("Deploy token (or set $DEPLOY_TOKEN)")
                .takes_value()
                .required()
                .env("DEPLOY_TOKEN")       // CLI → $DEPLOY_TOKEN → required error
                .build().unwrap(),
        )
        .flag(
            Flag::builder("env")
                .description("Target environment")
                .takes_value()
                .choices(["prod", "staging", "dev"])
                .default_value("dev")      // omitting --env defaults to "dev"
                .build().unwrap(),
        )
        .flag(
            Flag::builder("dry-run")
                .short('n')
                .description("Simulate without making changes")
                .build().unwrap(),         // boolean: presence → "true"
        )
        .best_practice("Always deploy to staging before prod")
        .anti_pattern("Do not pass --token on the command line in CI; use $DEPLOY_TOKEN")
        .handler(Arc::new(|parsed| {
            let service = parsed.arg("service").unwrap();
            let env     = parsed.flag("env").unwrap_or("dev");
            let token   = parsed.flag("token").unwrap();
            let dry_run = parsed.flag_bool("dry-run");

            if dry_run {
                println!("[DRY RUN] Would deploy {} to {} (token: {}...)", service, env, &token[..4.min(token.len())]);
            } else {
                println!("Deploying {} to {} ...", service, env);
            }
            Ok(())
        }))
        .build().unwrap()
}

fn main() {
    let cli = Cli::new(vec![build_deploy()]).app_name("deployer").version("1.0.0");
    if let Err(e) = cli.run_env_args() {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}
```

**Key points:**
- `.env("DEPLOY_TOKEN")` makes the parser check `std::env::var("DEPLOY_TOKEN")` when `--token` is absent. Combined with `.required()`, the flag is satisfied by either source or the parser returns `ParseError::MissingFlag`.
- `.choices(["prod", "staging", "dev"])` causes the parser to return `ParseError::InvalidChoice` if the user passes an unknown environment — no handler validation needed.
- `.default_value("dev")` means `parsed.flag("env")` is always `Some(...)` after parsing.
- `parsed.flag_bool("dry-run")` is the idiomatic way to test a boolean flag.

---

# Recipe 4: Repeatable Flags

**Problem:** A `build` command accepts `--tag alpha --tag beta --tag latest` (collect all values) and `-v -v -v` to set verbosity level (count occurrences).

```rust
use std::sync::Arc;
use argot::{Argument, Command, Flag, Parser, Registry};

fn main() {
    let build = Command::builder("build")
        .summary("Build and tag an image")
        .argument(Argument::builder("image").required().build().unwrap())
        .flag(
            Flag::builder("tag")
                .short('t')
                .description("Tag to apply (may be repeated)")
                .takes_value()
                .repeatable()              // --tag a --tag b → ["a","b"] JSON array
                .build().unwrap(),
        )
        .flag(
            Flag::builder("verbose")
                .short('v')
                .description("Increase verbosity (repeat for more)")
                .repeatable()              // -v -v -v → "3"
                .build().unwrap(),
        )
        .handler(Arc::new(|parsed| {
            let image = parsed.arg("image").unwrap_or("unknown");

            // flag_values returns Vec<String>; works for both single and repeated values.
            let tags = parsed.flag_values("tag");
            let verbosity = parsed.flag_count("verbose");

            if verbosity >= 2 {
                println!("[verbose] image={} tags={:?}", image, tags);
            }

            if tags.is_empty() {
                println!("Building {} with no tags", image);
            } else {
                println!("Building {} with tags: {}", image, tags.join(", "));
            }
            Ok(())
        }))
        .build().unwrap();

    let registry = Registry::new(vec![build]);
    let parser = Parser::new(registry.commands());

    // Simulate: build myapp --tag alpha --tag beta -v -v
    let parsed = parser.parse(&["build", "myapp", "--tag", "alpha", "--tag", "beta", "-v", "-v"]).unwrap();
    if let Some(h) = &parsed.command.handler { h(&parsed).unwrap(); }
}
```

**Key points:**
- `.repeatable()` on a value-taking flag collects repeated occurrences into a JSON array string stored in the flag map. `parsed.flag_values("tag")` deserializes it back to `Vec<String>` transparently.
- `.repeatable()` on a boolean flag counts occurrences; `parsed.flag_count("verbose")` parses the numeric string back to `u64`.
- `flag_values` also works for non-repeatable flags — it returns a single-element `Vec` — so you can use it uniformly when you are unsure whether a flag is repeatable.

---

# Recipe 5: Variadic Positional Arguments

**Problem:** An `archive` command accepts one or more file paths: `archive file1.txt file2.txt dir/file3.txt`. You want all paths collected into one argument.

```rust
use std::sync::Arc;
use argot::{Argument, Command, Flag, Parser, Registry};

fn main() {
    let archive = Command::builder("archive")
        .summary("Create a tar archive from files")
        .argument(
            Argument::builder("output")
                .description("Output archive name")
                .required()
                .build().unwrap(),
        )
        .argument(
            Argument::builder("files")
                .description("One or more files to include")
                .required()
                .variadic()                // consumes all remaining positional tokens
                .build().unwrap(),
        )
        .flag(
            Flag::builder("compress")
                .short('z')
                .description("Compress the archive with gzip")
                .build().unwrap(),
        )
        .handler(Arc::new(|parsed| {
            let output = parsed.arg("output").unwrap();
            // Variadic args are stored as a JSON array string; flag_values decodes it.
            let files = parsed.flag_values("files");

            // Alternatively, decode directly:
            // let files: Vec<String> = serde_json::from_str(
            //     parsed.args.get("files").map(String::as_str).unwrap_or("[]")
            // ).unwrap_or_default();

            let compress = parsed.flag_bool("compress");
            println!(
                "Archiving {} files into '{}' (compressed={})",
                files.len(), output, compress
            );
            for f in &files {
                println!("  + {}", f);
            }
            Ok(())
        }))
        .build().unwrap();

    let registry = Registry::new(vec![archive]);
    let parser = Parser::new(registry.commands());

    let parsed = parser
        .parse(&["archive", "out.tar", "file1.txt", "file2.txt", "dir/file3.txt"])
        .unwrap();
    if let Some(h) = &parsed.command.handler { h(&parsed).unwrap(); }
}
```

**Key points:**
- A variadic argument must be the last positional argument declared. `CommandBuilder::build` returns `BuildError::VariadicNotLast` if this is violated.
- The parser stores the collected values as a JSON array string in `parsed.args["files"]`. `parsed.flag_values("files")` works because `flag_values` first tries JSON array deserialization; pass the value from `parsed.args` directly to `serde_json::from_str` if you prefer.
- Arguments before the variadic (here `output`) are bound normally; only the remaining tokens go into the variadic slot.

---

# Recipe 6: Mutually Exclusive Flags

**Problem:** An `export` command accepts `--json`, `--yaml`, or `--csv` but only one at a time. The parser should reject combinations like `export --json --yaml` before the handler runs.

```rust
use std::sync::Arc;
use argot::{Command, Flag, Parser, ParseError, Registry};

fn build_export() -> Command {
    Command::builder("export")
        .summary("Export data in a structured format")
        .flag(Flag::builder("json").description("Export as JSON").build().unwrap())
        .flag(Flag::builder("yaml").description("Export as YAML").build().unwrap())
        .flag(Flag::builder("csv").description("Export as CSV").build().unwrap())
        .exclusive(["json", "yaml", "csv"])  // at most one may be present
        .handler(Arc::new(|parsed| {
            let fmt = if parsed.flag_bool("json") { "json" }
                      else if parsed.flag_bool("yaml") { "yaml" }
                      else if parsed.flag_bool("csv")  { "csv" }
                      else { "json" }; // default when none supplied
            println!("Exporting as {}", fmt);
            Ok(())
        }))
        .build().unwrap()
}

fn main() {
    let registry = Registry::new(vec![build_export()]);
    let parser = Parser::new(registry.commands());

    // Success: single format flag.
    let ok = parser.parse(&["export", "--yaml"]).unwrap();
    if let Some(h) = &ok.command.handler { h(&ok).unwrap(); }

    // Error: two format flags at once.
    match parser.parse(&["export", "--json", "--yaml"]) {
        Err(ParseError::MutuallyExclusive { flags }) => {
            eprintln!("error: these flags are mutually exclusive: {}", flags.join(", "));
        }
        Err(e) => eprintln!("unexpected error: {}", e),
        Ok(_)  => unreachable!(),
    }
}
```

**Key points:**
- `.exclusive(["json", "yaml", "csv"])` declares the group on the builder. `CommandBuilder::build` validates that all names exist as defined flags; passing an unknown name returns `BuildError::ExclusiveGroupUnknownFlag`.
- The parser enforces the constraint and returns `ParseError::MutuallyExclusive { flags }` where `flags` contains the conflicting flag names (with `--` prefix).
- Multiple exclusive groups can be declared by calling `.exclusive(...)` more than once on the same builder.
- A group requires at least two members; a single-member group returns `BuildError::ExclusiveGroupTooSmall`.

---

# Recipe 7: Custom Rendering with ANSI Color

**Problem:** You want help output with colored section headers. Implement the `Renderer` trait and register it with `Cli::with_renderer`.

```rust
use std::sync::Arc;
use argot::{Cli, Command, Flag, Argument, Example, render_help, render_markdown,
            render_subcommand_list, render_ambiguity, render_resolve_error};
use argot::render::Renderer;

// No color crate needed — use ANSI escape codes directly.
const BOLD: &str  = "\x1b[1m";
const CYAN: &str  = "\x1b[36m";
const RESET: &str = "\x1b[0m";

struct ColorRenderer;

impl Renderer for ColorRenderer {
    fn render_help(&self, cmd: &argot::Command) -> String {
        let plain = render_help(cmd);
        // Color non-indented all-uppercase section headers.
        plain
            .lines()
            .map(|line| {
                let trimmed = line.trim_start();
                let is_header = !line.starts_with(' ')
                    && !trimmed.is_empty()
                    && trimmed.chars().all(|c| c.is_uppercase() || c.is_whitespace() || c == ':');
                if is_header {
                    format!("{}{}{}{}", BOLD, CYAN, line, RESET)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }
    fn render_markdown(&self, cmd: &argot::Command) -> String { render_markdown(cmd) }
    fn render_subcommand_list(&self, cmds: &[argot::Command]) -> String { render_subcommand_list(cmds) }
    fn render_ambiguity(&self, input: &str, candidates: &[String]) -> String { render_ambiguity(input, candidates) }
}

fn main() {
    let cmd = Command::builder("serve")
        .summary("Start the HTTP server")
        .argument(Argument::builder("addr").description("Bind address").build().unwrap())
        .flag(Flag::builder("port").short('p').takes_value().description("TCP port")
              .default_value("8080").build().unwrap())
        .example(Example::new("default bind", "serve 0.0.0.0 --port 8080"))
        .handler(Arc::new(|parsed| {
            let port: u16 = parsed.flag_as_or("port", 8080u16);
            println!("Serving on port {}", port);
            Ok(())
        }))
        .build().unwrap();

    // All --help output now goes through ColorRenderer.
    Cli::new(vec![cmd])
        .with_renderer(ColorRenderer)
        .run_env_args()
        .unwrap_or_else(|e| eprintln!("{}", e));
}
```

**Key points:**
- Override only the methods you care about; delegate the rest to the free functions.
- `Cli::with_renderer` wires the renderer for help, subcommand listing, and ambiguity messages.
- Keep ANSI codes out of `render_help` return values so piped output stays clean — detect a TTY before using a `ColorRenderer`.

---

# Recipe 8: Middleware for Logging and Audit

**Problem:** You want every command invocation logged with a timestamp, and every parse failure written to an audit log. Use the `Middleware` trait and `Cli::with_middleware`.

```rust
use std::sync::Arc;
use std::time::SystemTime;
use argot::{Cli, Command, Argument, ParsedCommand, Middleware};
use argot::parser::ParseError;

// ── Logger middleware ────────────────────────────────────────────────────────

struct Logger;

impl Middleware for Logger {
    fn before_dispatch(
        &self,
        parsed: &ParsedCommand<'_>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        eprintln!("[LOG] {} dispatching: {}", ts, parsed.command.canonical);
        Ok(())
    }

    fn on_parse_error(&self, err: &ParseError) {
        eprintln!("[LOG] parse error: {}", err);
    }
}

// ── Audit middleware ─────────────────────────────────────────────────────────

struct Audit;

impl Middleware for Audit {
    fn before_dispatch(
        &self,
        parsed: &ParsedCommand<'_>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        eprintln!("[AUDIT] command invoked: {}", parsed.command.canonical);
        Ok(())
    }

    fn after_dispatch(
        &self,
        parsed: &ParsedCommand<'_>,
        result: &Result<(), Box<dyn std::error::Error + Send + Sync>>,
    ) {
        if result.is_err() {
            eprintln!("[AUDIT] command failed: {}", parsed.command.canonical);
        }
    }

    fn on_parse_error(&self, err: &ParseError) {
        eprintln!("[AUDIT] failed invocation: {}", err);
    }
}

fn main() {
    let deploy = Command::builder("deploy")
        .summary("Deploy the service")
        .argument(Argument::builder("env").required().build().unwrap())
        .handler(Arc::new(|parsed| {
            println!("Deploying to {}", parsed.arg("env").unwrap_or("unknown"));
            Ok(())
        }))
        .build().unwrap();

    // Both middlewares fire in registration order.
    Cli::new(vec![deploy])
        .with_middleware(Logger)
        .with_middleware(Audit)
        .run_env_args()
        .unwrap_or_else(|e| eprintln!("{}", e));
}
```

**Key points:**
- All `Middleware` methods have default no-op implementations; override only the hooks you need.
- Middlewares fire in registration order. `before_dispatch` can abort by returning `Err(...)`.
- `on_parse_error` receives `&ParseError` so you can branch on variant (e.g., log `MissingArgument` differently from `Resolve`).
- Add as many middlewares as needed with repeated `.with_middleware()` calls.

---

# Recipe 9: Exposing Commands via MCP for AI Agents

**Problem:** You want to expose your command registry as an MCP (Model Context Protocol) server so that AI agents can discover and invoke your commands as structured tools over stdio.

```rust
// requires --features mcp
#[cfg(feature = "mcp")]
fn main() {
    use std::sync::Arc;
    use argot::{Argument, Command, Example, Flag, McpServer, Registry};

    let migrate = Command::builder("migrate")
        .summary("Run database migrations")
        .description("Applies pending schema migrations to the target database.")
        .argument(
            Argument::builder("database")
                .description("Database name or connection alias")
                .required()
                .build().unwrap(),
        )
        .flag(
            Flag::builder("dry-run")
                .short('n')
                .description("Show migrations that would run without applying them")
                .build().unwrap(),
        )
        .flag(
            Flag::builder("version")
                .short('v')
                .description("Target schema version (default: latest)")
                .takes_value()
                .build().unwrap(),
        )
        .example(Example::new("migrate to latest", "migrate production"))
        .example(Example::new("dry-run on staging", "migrate staging --dry-run"))
        // best_practice and anti_pattern are surfaced in the MCP tool description,
        // helping agents make better decisions about when and how to call this tool.
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
        .build().unwrap();

    let registry = Registry::new(vec![migrate]);

    // McpServer reads JSON-RPC 2.0 requests from stdin and writes responses to stdout.
    // tools/list response example (one tool, trimmed):
    // {
    //   "jsonrpc": "2.0", "id": 1,
    //   "result": {
    //     "tools": [{
    //       "name": "migrate",
    //       "description": "Run database migrations",
    //       "inputSchema": {
    //         "type": "object",
    //         "properties": {
    //           "database": {"type": "string", "description": "Database name or connection alias"},
    //           "dry-run":  {"type": "boolean", "description": "..."},
    //           "version":  {"type": "string",  "description": "..."}
    //         },
    //         "required": ["database"]
    //       }
    //     }]
    //   }
    // }
    McpServer::new(registry)
        .server_name("db-tool")
        .server_version("1.0.0")
        .serve_stdio()
        .unwrap_or_else(|e| {
            eprintln!("MCP server error: {}", e);
            std::process::exit(1);
        });
}

#[cfg(not(feature = "mcp"))]
fn main() {
    eprintln!("This example requires --features mcp");
    std::process::exit(1);
}
```

**Key points:**
- `McpServer::new(registry)` wraps the entire `Registry`; all top-level commands become MCP tools named by their canonical name, subcommands become `"parent-child"` (joined with `-`).
- `best_practices` and `anti_patterns` are surfaced in the tool's `description` field, giving AI agents explicit guidance on safe and unsafe usage patterns.
- The MCP input schema is derived directly from the command's `Argument` and `Flag` definitions: required arguments and flags become JSON Schema `required` entries.
- `.serve_stdio()` blocks until EOF on stdin, making it suitable as the body of a binary launched by an agent runtime.

---

# Recipe 10: Walking the Full Command Tree

**Problem:** You need a flat list of every command in the registry — including all nested subcommands — with their full paths, to generate documentation, build shell completion scripts, or feed a search index.

```rust
use argot::{Command, Example, Registry};

fn build_registry() -> Registry {
    let add = Command::builder("add")
        .summary("Add a named remote")
        .build().unwrap();
    let remove = Command::builder("remove")
        .summary("Remove a named remote")
        .build().unwrap();
    let list = Command::builder("list")
        .summary("List configured remotes")
        .build().unwrap();

    let remote = Command::builder("remote")
        .summary("Manage tracked repositories")
        .subcommand(add)
        .subcommand(remove)
        .subcommand(list)
        .build().unwrap();

    let status = Command::builder("status")
        .summary("Show working tree status")
        .build().unwrap();

    Registry::new(vec![remote, status])
}

fn main() {
    let registry = build_registry();

    // iter_all_recursive returns Vec<CommandEntry<'_>> in depth-first order.
    let entries = registry.iter_all_recursive();

    println!("All commands ({} total):", entries.len());
    for entry in &entries {
        // path_str() joins canonical names with '.', e.g. "remote.add"
        // name()     returns the leaf canonical name, e.g. "add"
        println!("  {:20}  {}", entry.path_str(), entry.command.summary);
    }

    // Build a completion-script-friendly list of space-separated paths.
    let paths: Vec<String> = entries.iter().map(|e| e.path.join(" ")).collect();
    println!("\nCompletion paths:");
    for p in &paths {
        println!("  {}", p);
    }

    // Serialize the full tree to JSON for a search index.
    let json = registry.to_json().expect("serialization failed");
    let _ = json; // would be written to a file or sent to a search service
}
```

**Output:**
```
All commands (5 total):
  remote                Manage tracked repositories
  remote.add            Add a named remote
  remote.remove         Remove a named remote
  remote.list           List configured remotes
  status                Show working tree status

Completion paths:
  remote
  remote add
  remote remove
  remote list
  status
```

**Key points:**
- `Registry::iter_all_recursive()` returns `Vec<CommandEntry<'_>>`. Each `CommandEntry` holds `path: Vec<String>` (canonical names root → leaf) and `command: &Command`.
- `entry.path_str()` produces a dotted path string (`"remote.add"`); `entry.name()` returns only the leaf name (`"add"`); `entry.path.join(" ")` gives the space-separated form suitable for completion scripts.
- The traversal is depth-first: a parent is yielded before its children, and children appear in registration order.
- `Registry::to_json()` serializes the entire tree (handlers excluded) for use with search indexes, documentation generators, or agent tool registries.
