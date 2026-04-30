# Migrating from clap

This guide is for application authors who have been using [clap] and are
considering argot — or who want a mental model for how argot's API maps onto
clap concepts they already know.

[clap]: https://docs.rs/clap

## Why migrate?

argot is **agent-first**: the same `Command` definition that powers your CLI
also serializes to JSON for tool-call layers, renders to Markdown skill files
for `.claude/skills/` and friends, and (with the `mcp` feature) exposes itself
over the Model Context Protocol. clap is excellent at humans-only CLIs;
argot's pitch is "one definition, two audiences."

If your CLI never needs to be discoverable by an LLM agent, clap is fine and
you should probably stay. If you've been bolting JSON-export glue on top of
clap, argot makes that the load-bearing path.

## API mapping at a glance

### Struct-level attributes

| clap                               | argot                                                       | Notes                                                                                  |
| ---------------------------------- | ----------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `#[derive(Parser)]`                | `#[derive(ArgotCommand)]` (feature: `derive`)               | argot's derive is intentionally smaller; the builder API covers everything else.       |
| `#[command(name = "foo")]`         | `#[argot(name = "foo")]`                                    |                                                                                        |
| `#[command(about = "...")]`        | `#[argot(summary = "...")]`                                 | argot distinguishes one-line `summary` from multi-paragraph `description`.             |
| `#[command(long_about = "...")]`   | `#[argot(description = "...")]`                             |                                                                                        |
| `#[command(version = "1.0")]`      | Set on the `Cli`: `Cli::new(..).version("1.0")`             | argot keeps version on the runtime entry point, not the command.                       |
| `#[command(alias = "x")]`          | `#[argot(alias = "x")]`                                     | Repeatable.                                                                            |
| `#[command(visible_alias = "x")]`  | `#[argot(alias = "x")]`                                     | argot aliases are always visible in help.                                              |
| *No clap equivalent*               | `Command::builder(..).spelling("x")` (builder-only)         | Silent typo correction (matches but isn't shown in help). Not yet in the derive.       |
| *No clap equivalent*               | `Command::builder(..).semantic_alias("ship to prod")` (builder-only) | Natural-language phrase for intent matching. Not yet in the derive.            |
| *No clap equivalent*               | `#[argot(best_practice = "...")]`, `anti_pattern`           | Surfaced to LLM agents, included in JSON / skill-file output.                          |
| `#[command(subcommand)]`           | Use `CommandBuilder::subcommand` (no derive equivalent yet) | Subcommand routing in the derive is a known gap; use the builder.                      |

### Argument and flag attributes

| clap                                            | argot                                                | Notes                                                                                                                |
| ----------------------------------------------- | ---------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `#[arg(short, long)]`                           | `#[argot(flag, short = 'x')]`                        | argot derives the long name from the field name automatically.                                                       |
| `#[arg(value_name = "ENV")]`                    | Embedded in description / argument name              | argot uses the field/argument name as the value placeholder.                                                         |
| `#[arg(required = true)]`                       | `#[argot(required)]`                                 |                                                                                                                      |
| `#[arg(default_value = "1.0")]`                 | `#[argot(default = "1.0")]`                          |                                                                                                                      |
| `#[arg(default_value_t = 42)]`                  | `#[argot(default = "42")]`                           | argot stores defaults as strings; coerce at parse time via `ParsedCommand::flag_as`.                                 |
| `#[arg(env = "FOO")]`                           | `Flag::builder("foo").env("FOO")` (builder)          | Builder-only today; the derive doesn't surface `env` yet.                                                            |
| `#[arg(value_parser = clap::value_parser!())]`  | `ParsedCommand::flag_as::<T>` at handler time        | argot stores parsed values as strings and coerces via `FromStr` at access time. No central value-parser registry.    |
| `#[arg(value_enum)]`                            | `Flag::builder("x").choices(["a", "b"])` (builder)   | argot validates choices against a static list — same UX, different shape.                                            |
| `#[arg(action = ArgAction::Count)]`             | `Flag::builder("v").repeatable()`                    | Same semantic: counts occurrences for boolean flags, collects values for value flags.                                |
| `#[arg(num_args = 1..)]`, `last = true`         | `Argument::builder(..).variadic()`                   | Variadic args must be last (enforced at build time).                                                                 |
| `#[arg(conflicts_with_all = ["a", "b"])]`       | `Command::builder(..).exclusive(["a", "b", "c"])`    | argot uses mutually-exclusive groups directly on the command.                                                        |
| `#[arg(requires = "...")]`                      | Not yet supported                                    | Track in your handler if you need it. Open issue.                                                                    |
| `#[arg(hide = true)]`                           | Use `spelling` (matches but is not advertised)       | argot doesn't have a separate "hidden" axis.                                                                         |

### Negation flags

clap's `--no-foo` is supported by argot's parser at runtime regardless of how
you defined the flag — `Parser::parse(&["cmd", "--no-verbose"])` resolves to
`flag_count("verbose") == 0`. You don't need to declare both forms.

## Worked example: a simple command

### clap

```rust,ignore
use clap::Parser;

#[derive(Parser)]
#[command(name = "deploy", about = "Deploy the app")]
struct Deploy {
    /// Target environment
    #[arg(value_name = "ENV")]
    env: String,

    /// Simulate without changes
    #[arg(short = 'n', long)]
    dry_run: bool,

    /// Deployment strategy
    #[arg(short, long, default_value = "rolling")]
    strategy: String,
}

fn main() {
    let args = Deploy::parse();
    println!("deploying to {}", args.env);
}
```

### argot (derive)

```rust,ignore
use std::sync::Arc;
use argot_cmd::{ArgotCommand, Cli};

#[derive(ArgotCommand)]
#[argot(summary = "Deploy the app")]
pub struct Deploy {
    #[argot(positional, required, description = "Target environment")]
    env: String,

    #[argot(flag, short = 'n', description = "Simulate without changes")]
    dry_run: bool,

    #[argot(
        flag,
        short = 's',
        takes_value,
        default = "rolling",
        description = "Deployment strategy"
    )]
    strategy: String,
}

fn main() {
    let mut cmd = Deploy::command();
    cmd.handler = Some(Arc::new(|parsed| {
        println!("deploying to {}", parsed.args["env"]);
        Ok(())
    }));
    Cli::new(vec![cmd]).run_env_args_and_exit();
}
```

Differences worth noting:

- argot fields require an explicit `#[argot(positional)]` or `#[argot(flag)]`
  marker. Bare fields are not auto-classified — this is a deliberate
  "everything declared, nothing inferred" stance.
- Arguments are bound by **name** in `parsed.args["env"]` rather than typed
  struct fields. Coerce with `parsed.arg_as::<T>("env")` or
  `parsed.flag_as::<T>("strategy")`.
- The handler is attached after `Deploy::command()` rather than baked in —
  this is what lets the same struct produce a `Command` for JSON export
  *and* for runtime dispatch.

## Worked example: subcommands

argot's derive doesn't yet handle subcommand routing, so this is a builder
example. The clap derive form on the left, the argot builder form on the
right.

### clap

```rust,ignore
use clap::{Parser, Subcommand};

#[derive(Parser)]
struct App {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Deploy { env: String },
    Status,
}
```

### argot (builder)

```rust,ignore
use argot_cmd::{Argument, Cli, Command};

let deploy = Command::builder("deploy")
    .summary("Deploy the app")
    .argument(Argument::builder("env").required().build().unwrap())
    .build()
    .unwrap();

let status = Command::builder("status")
    .summary("Show status")
    .build()
    .unwrap();

Cli::new(vec![deploy, status]).run_env_args_and_exit();
```

If you have many subcommands, mix the two: build each leaf with the derive
macro, then assemble them into the tree with the builder.

## Worked example: enum-valued flag

### clap

```rust,ignore
#[derive(clap::ValueEnum, Clone)]
enum Format { Json, Yaml, Text }

#[derive(Parser)]
struct Args {
    #[arg(short, long, value_enum, default_value_t = Format::Text)]
    format: Format,
}
```

### argot

```rust,ignore
use argot_cmd::{Command, Flag, ParsedCommand};

let cmd = Command::builder("export")
    .flag(
        Flag::builder("format")
            .short('f')
            .takes_value()
            .choices(["json", "yaml", "text"])
            .default_value("text")
            .build()
            .unwrap(),
    )
    .build()
    .unwrap();

// At handler time:
fn handle(parsed: &ParsedCommand<'_>) {
    match parsed.flag("format").unwrap() {
        "json" => { /* ... */ }
        "yaml" => { /* ... */ }
        "text" => { /* ... */ }
        _ => unreachable!(), // choices() validates at parse time
    }
}
```

argot does not have a `ValueEnum` derive; the `choices(...)` builder gives
you the same parse-time validation, and you handle the `&str` directly.

## Worked example: env-var fallback

### clap

```rust,ignore
#[derive(Parser)]
struct Args {
    #[arg(long, env = "DATABASE_URL")]
    db: String,
}
```

### argot

```rust,ignore
use argot_cmd::{Command, Flag};

let cmd = Command::builder("connect")
    .flag(
        Flag::builder("db")
            .takes_value()
            .required()
            .env("DATABASE_URL")
            .build()
            .unwrap(),
    )
    .build()
    .unwrap();
```

Resolution order matches clap: explicit CLI arg → env var → default → error.

## What argot does that clap doesn't

These are reasons to migrate beyond just "feature parity":

1. **`Cli::with_query_support()`** injects a `query` meta-command that emits
   structured JSON for every command, suitable for an LLM tool-call layer.
2. **`render_skill_file_with_frontmatter`** produces agent-consumable Markdown
   for `.claude/skills/<name>/SKILL.md`, `.cursor/skills/`, etc.
3. **MCP transport** (feature `mcp`) exposes the registry over Model Context
   Protocol stdio.
4. **Layered command sources** (`source` module): assemble a `Cli` from
   compile-time commands plus on-disk Markdown files in
   `~/.config/<app>/commands/` or `<repo>/.<app>/commands/`. End users (and
   AI agents) can author new commands without touching Rust source.
5. **`Cli::warn_missing_dry_run(true)`** flags mutating commands that lack a
   `--dry-run` flag — designed for agent guard-rails.

## Things clap has that argot doesn't (yet)

Be honest about the gaps before you decide to switch:

- **Subcommand routing in the derive macro.** Use `CommandBuilder::subcommand`
  to compose. The derive will catch up.
- **`requires` cross-flag dependencies.** Validate in your handler.
- **Custom `value_parser` / typed value-enum derive.** Use `choices()` for
  fixed sets and `ParsedCommand::flag_as::<T>()` for typed coercion.
- **Auto-generated shell completions for arbitrary shells.** argot has
  `render_completion(Shell::Bash | Zsh | Fish)`; clap covers more shells.
- **Color/styling controls.** argot's renderer is plain by design (agents
  don't care about ANSI). Plug a custom `Renderer` if you need styled
  output.

## Migration recipe

If you have a clap codebase and want to spike a port:

1. **Start with the builder, not the derive.** It's a 1:1 mental model for
   your existing `clap::Arg::new(...)` chains and side-steps the derive's
   current subcommand gap.
2. **Convert one subcommand at a time.** argot's `Cli` happily holds a small
   set of commands while the rest of your app stays on clap behind a
   different binary entry point.
3. **Wire the JSON/skill-file outputs early.** `Cli::with_query_support()`
   plus a `render_skill_files` call gives you the agent-facing surface in
   <10 lines and is the main reason to switch — it's a forcing function
   for getting your command metadata accurate.
4. **Migrate handlers last.** They're the largest surface and the most
   mechanical conversion (clap typed structs → argot `ParsedCommand` access).
5. **Don't try to preserve clap's exact help formatting.** argot's renderer
   is structurally different. If you need clap-identical text, implement
   the `Renderer` trait; otherwise let argot's defaults do their thing.

## Worked end-to-end: skill-file export

This is the payoff. Once you've ported, you get this for free:

```rust,ignore
use argot_cmd::{render::{render_skill_files_with_frontmatter, SkillFrontmatter}, Registry};

let registry: Registry = /* your migrated commands */;
let output = render_skill_files_with_frontmatter(&registry, |cmd| {
    Some(SkillFrontmatter::new(format!("mytool-{}", cmd.canonical))
        .version(env!("CARGO_PKG_VERSION"))
        .requires_bin("mytool"))
});
std::fs::write(".claude/skills/mytool/SKILL.md", output).unwrap();
```

Replicating this with clap means writing a custom serializer that walks
`clap::Command` reflectively. With argot it's three lines, because `Command`
is the metadata, not a parser-generator artifact.
