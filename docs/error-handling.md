# Error Handling in Argot

Argot uses typed error enums at every layer of the pipeline. This guide covers each error type, the conditions that produce it, and recommended patterns for handling errors in both binary crates and library code.

---

## Error Type Overview

| Error type      | Layer        | When it is raised |
|-----------------|--------------|-------------------|
| `BuildError`    | model        | A builder's `build()` call fails validation |
| `ParseError`    | parser       | `Parser::parse` cannot process the argv slice |
| `ResolveError`  | resolver     | A string token does not map to exactly one command |
| `CliError`      | cli          | `Cli::run` or `Cli::run_env_args` encounters any runtime error |
| `QueryError`    | query        | A `Registry` method fails (currently only JSON serialization) |

All error types implement `std::error::Error`, `Debug`, and `Display`. `BuildError`, `ParseError`, and `ResolveError` also implement `PartialEq`, making them straightforward to test with `assert_eq!`.

---

## BuildError

`BuildError` is returned by the `build()` method on `CommandBuilder`, `ArgumentBuilder`, and `FlagBuilder`. It signals a structural problem with the command definition itself — something that can and should be caught before the program handles any user input.

### Variants

| Variant | Trigger |
|---------|---------|
| `EmptyCanonical` | The canonical name (or argument/flag name) is empty or all whitespace |
| `DuplicateAlias(String)` | Two aliases on the same command share the same string |
| `AliasEqualsCanonical(String)` | An alias is identical to the command's canonical name |
| `DuplicateFlagName(String)` | Two flags on the same command have the same long name |
| `DuplicateShortFlag(char)` | Two flags on the same command share the same short character |
| `DuplicateArgumentName(String)` | Two positional arguments on the same command share the same name |
| `DuplicateSubcommandName(String)` | Two subcommands at the same level share the same canonical name |
| `VariadicNotLast(String)` | A variadic argument is not the last argument in the declaration order |
| `EmptyChoices(String)` | A flag's `choices` list is empty, which would reject every value |

### Validate at startup, not at runtime

`BuildError` reflects a programming mistake in the command definition, not a user error. Because of this, the best practice is to call `build().unwrap()` during initialization in binary crates. Panicking at startup is the correct response — it surfaces the bug immediately during development rather than silently skipping a command at runtime.

```rust
// Binary crate: panic at startup if the definition is wrong.
let deploy = Command::builder("deploy")
    .summary("Deploy the application")
    .argument(
        Argument::builder("environment")
            .required()
            .build()
            .unwrap(),   // panics early if "environment" is invalid
    )
    .build()
    .unwrap();           // panics early if the command definition is invalid
```

In library code that programmatically constructs commands (for example, based on runtime configuration), propagate the error with `?` instead:

```rust
// Library code: propagate BuildError to the caller.
fn make_command(name: &str) -> Result<Command, BuildError> {
    Command::builder(name)
        .argument(Argument::builder("target").required().build()?)
        .build()
}
```

### Matching specific variants

```rust
use argot::BuildError;

match Command::builder("").build() {
    Err(BuildError::EmptyCanonical) => eprintln!("command name must not be empty"),
    Err(BuildError::DuplicateFlagName(name)) => {
        eprintln!("flag '{}' is defined more than once", name)
    }
    Err(e) => eprintln!("build error: {}", e),
    Ok(_) => {}
}
```

---

## ParseError

`ParseError` is returned by `Parser::parse`. It represents a problem with the user-supplied argument slice — the command definition is valid, but what the user typed does not conform to it.

### Variants

| Variant | Explanation |
|---------|-------------|
| `NoCommand` | The argv slice was empty; no command name was provided |
| `Resolve(ResolveError)` | The command token (or a subcommand token) could not be resolved; wraps `ResolveError` transparently |
| `MissingArgument(String)` | A required positional argument was absent; the inner string is the argument name |
| `UnexpectedArgument(String)` | More positional arguments were supplied than the command declares; the inner string is the first unexpected token |
| `MissingFlag(String)` | A required flag was absent and no env-var fallback provided a value; the inner string is the flag name |
| `FlagMissingValue { name }` | A value-taking flag was provided without a following value |
| `UnknownFlag(String)` | A flag token was not recognized by the resolved command; includes leading dashes, e.g. `"--foo"` |
| `UnknownSubcommand { parent, got }` | A word token did not match any declared subcommand on a parent with no positional arguments |
| `InvalidChoice { flag, value, choices }` | A flag value was not in the flag's declared `choices` list |

Note: `Resolve` wraps `ResolveError` with `#[error(transparent)]`, so `parse_error.to_string()` produces the same message as the wrapped `ResolveError`.

### Matching specific variants

```rust
use argot::{ParseError, ResolveError};

match parser.parse(&argv) {
    Ok(parsed) => {
        if let Some(handler) = &parsed.command.handler {
            handler(&parsed)?;
        }
    }
    Err(ParseError::MissingArgument(name)) => {
        eprintln!("Missing required argument: <{}>", name);
        eprintln!("{}", render_help(registry.get_command("deploy").unwrap()));
    }
    Err(ParseError::MissingFlag(name)) => {
        eprintln!("Missing required flag: --{}", name);
    }
    Err(ParseError::Resolve(ResolveError::Unknown { ref input, ref suggestions, .. })) => {
        eprintln!("Unknown command: `{}`", input);
        if !suggestions.is_empty() {
            eprintln!("Did you mean: {}", suggestions.join(", "));
        }
    }
    Err(ParseError::InvalidChoice { ref flag, ref value, ref choices }) => {
        eprintln!("--{} received invalid value `{}`", flag, value);
        eprintln!("Valid choices: {}", choices.join(", "));
    }
    Err(e) => eprintln!("Parse error: {}", e),
}
```

### Custom error rendering

When you want consistent help output alongside parse errors, render help for the partially-resolved command:

```rust
use argot::{ParseError, Registry, Parser, render_help};

fn dispatch(registry: &Registry, argv: &[&str]) {
    let parser = Parser::new(registry.commands());
    match parser.parse(argv) {
        Ok(parsed) => {
            if let Some(handler) = &parsed.command.handler {
                if let Err(e) = handler(&parsed) {
                    eprintln!("error: {}", e);
                    std::process::exit(1);
                }
            } else {
                // No handler: print help instead of silently succeeding.
                print!("{}", render_help(parsed.command));
            }
        }
        Err(ParseError::MissingArgument(name)) => {
            eprintln!("Missing required argument: <{}>", name);
            if let Some(cmd) = registry.get_command(argv[0]) {
                eprint!("{}", render_help(cmd));
            }
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }
}
```

---

## ResolveError

`ResolveError` is returned by `Resolver::resolve` and is also embedded in `ParseError::Resolve`. It has two variants.

### Variants

**`Unknown { input, suggestions }`** — the input did not match any registered command exactly or as a prefix. `suggestions` contains up to three canonical names whose Levenshtein edit distance from the input is ≤ 2, or which contain the input as a substring. `suggestions` may be empty.

**`Ambiguous { input, candidates }`** — the input is a prefix of more than one command. `candidates` lists the canonical names of all matching commands.

### How suggestions work

The resolver normalizes input by trimming whitespace and lowercasing before comparison. Suggestions are computed only for `Unknown`, not for `Ambiguous`. They are ranked by edit distance and capped at three entries.

```rust
use argot::{Resolver, ResolveError};

let resolver = Resolver::new(registry.commands());
match resolver.resolve("dploy") {
    Ok(cmd) => println!("resolved: {}", cmd.canonical),
    Err(ResolveError::Unknown { input, suggestions }) => {
        eprintln!("Unknown command: `{}`", input);
        if !suggestions.is_empty() {
            eprintln!("Did you mean: {}", suggestions.join(", "));
        }
    }
    Err(ResolveError::Ambiguous { input, candidates }) => {
        eprintln!("Ambiguous command `{}`: could match {}", input, candidates.join(", "));
    }
}
```

### Using `render_resolve_error` for user-friendly output

The render layer provides `render_resolve_error` and `render_ambiguity` as convenience formatters. They produce the same styled text that `Cli` prints automatically:

```rust
use argot::{ResolveError, render_resolve_error};

let err = ResolveError::Unknown {
    input: "dploy".to_string(),
    suggestions: vec!["deploy".to_string()],
};
eprintln!("{}", render_resolve_error(&err));
// Unknown command: `dploy`
// Did you mean: deploy
```

```rust
use argot::{ResolveError, render_resolve_error};

let err = ResolveError::Ambiguous {
    input: "dep".to_string(),
    candidates: vec!["deploy".to_string(), "describe".to_string()],
};
eprintln!("{}", render_resolve_error(&err));
// Ambiguous command "dep":
//   deploy
//   describe
```

---

## CliError

`CliError` is returned by `Cli::run` and `Cli::run_env_args`. It wraps the three failure modes that `Cli` can encounter after the built-in `--help`, `--version`, and empty-input behaviors have been handled.

### Variants

| Variant | Explanation |
|---------|-------------|
| `Parse(ParseError)` | Wraps `ParseError` transparently. `Cli::run` also prints the error and best-effort help to stderr before returning this variant. |
| `NoHandler(String)` | The resolved command has no handler registered. The inner string is the command's canonical name. |
| `Handler(Box<dyn Error + Send + Sync>)` | The registered handler returned an error. The handler's error message is captured as a string and re-boxed. |

Note: handler errors from `HandlerFn` (which returns `Box<dyn Error>`) are converted to `Box<dyn Error + Send + Sync>` via their `Display` output, so the original error type is not preserved across this boundary. If you need to inspect handler errors programmatically, handle them before they reach `Cli` by dispatching manually (see the deploy_tool example in `examples/deploy_tool.rs`).

### Recommended `main` pattern

```rust
fn main() {
    let cli = Cli::new(build_commands())
        .app_name("mytool")
        .version(env!("CARGO_PKG_VERSION"));

    if let Err(e) = cli.run_env_args() {
        // Cli::run already printed parse errors and help to stderr.
        // For NoHandler and Handler errors, print here.
        match &e {
            argot::CliError::Parse(_) => {
                // Already printed by Cli::run. Just exit.
            }
            _ => eprintln!("error: {}", e),
        }
        std::process::exit(1);
    }
}
```

The simplest acceptable form when you don't need to distinguish variants:

```rust
fn main() {
    let cli = Cli::new(build_commands())
        .app_name("mytool")
        .version(env!("CARGO_PKG_VERSION"));

    if let Err(e) = cli.run_env_args() {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}
```

---

## QueryError

`QueryError` is returned by `Registry::to_json`. It has one variant:

**`Serialization(serde_json::Error)`** — JSON serialization of the command tree failed. In practice this should never occur for well-formed command definitions, since all model types derive `Serialize`.

```rust
match registry.to_json() {
    Ok(json) => println!("{}", json),
    Err(e) => eprintln!("failed to serialize registry: {}", e),
}
```

---

## Custom Error Types in Handlers

Handler closures return `Result<(), Box<dyn std::error::Error>>`. Any type implementing `std::error::Error` can be returned by boxing it. Using `thiserror` to define a dedicated error enum gives you structured variants and avoids string formatting at the error site.

```rust
use std::sync::Arc;
use argot::{Command, Argument, Flag};

#[derive(Debug, thiserror::Error)]
enum DeployError {
    #[error("environment `{0}` is not recognized")]
    UnknownEnvironment(String),
    #[error("deployment failed: {0}")]
    Io(#[from] std::io::Error),
}

let deploy = Command::builder("deploy")
    .argument(Argument::builder("env").required().build().unwrap())
    .handler(Arc::new(|parsed| {
        let env = parsed.arg("env").unwrap_or("dev");
        if !["prod", "staging", "dev"].contains(&env) {
            return Err(Box::new(DeployError::UnknownEnvironment(env.to_string())));
        }
        // Any std::io::Error is automatically converted via the #[from] impl:
        // std::fs::write("deploy.log", env)?;
        Ok(())
    }))
    .build()
    .unwrap();
```

The `#[from]` attribute on `Io` allows `?` to convert `std::io::Error` into `DeployError::Io` automatically within the closure body.

---

## Error Propagation Patterns

### Using `?` inside handlers

Because handlers return `Box<dyn std::error::Error>`, any error type that implements `std::error::Error` can be propagated with `?`:

```rust
.handler(Arc::new(|parsed| {
    let count: u32 = parsed
        .arg("count")
        .unwrap_or("1")
        .parse::<u32>()
        .map_err(|e| format!("invalid count: {}", e))?;
    // ...
    Ok(())
}))
```

String literals and `String` values are also valid because `Box<dyn Error>` can be constructed from them:

```rust
.handler(Arc::new(|parsed| {
    let path = parsed.arg("file").ok_or("missing required argument: file")?;
    Ok(())
}))
```

### Converting ParseError to application-level errors

If you are building a library wrapper around argot, you may want to define your own error type that encompasses `ParseError`:

```rust
#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error(transparent)]
    Parse(#[from] argot::ParseError),
    #[error("command not found: {0}")]
    NotFound(String),
}

fn run_command(registry: &argot::Registry, argv: &[&str]) -> Result<(), AppError> {
    let parser = argot::Parser::new(registry.commands());
    let parsed = parser.parse(argv)?;  // ParseError converts via #[from]
    if parsed.command.handler.is_none() {
        return Err(AppError::NotFound(parsed.command.canonical.to_string()));
    }
    Ok(())
}
```

### Logging vs displaying errors

- Display errors to the user with `eprintln!("{}", e)` — the `Display` impl on every error type produces a readable message.
- For structured logging, use `{:?}` to get the debug representation including variant names and field values.
- `ParseError::Resolve` is transparent, so `format!("{}", parse_err)` produces the underlying `ResolveError` message directly.
