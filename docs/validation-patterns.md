# Validation Patterns in Argot

Argot validates command input at two distinct points: the **parser layer** enforces structural constraints declared in the command definition, and the **handler** performs semantic validation that requires business logic or external resources. This guide covers both levels with practical examples.

---

## Built-in Validation

The parser enforces built-in constraints automatically before the handler is ever called. Prefer built-in validation over handler validation whenever the constraint can be expressed declaratively — it produces consistent error messages, is reflected in generated help text, and keeps handler code focused on business logic.

### Required Arguments

Mark a positional argument as required with `.required()`. The parser returns `ParseError::MissingArgument` if the argument is absent.

```rust
use argot::Argument;

Argument::builder("env")
    .description("Target environment (e.g. staging, prod)")
    .required()
    .build()
    .unwrap()
```

The parser enforces this after all tokens have been processed, so a missing required argument always produces an error — there is no way for the handler to receive a `ParsedCommand` with the argument absent.

### Required Flags

Flags are made required the same way. The parser returns `ParseError::MissingFlag` if the flag is absent from the command line and no environment variable fallback provides a value.

```rust
use argot::Flag;

Flag::builder("token")
    .takes_value()
    .required()
    .env("API_TOKEN")
    .build()
    .unwrap()
```

With `.env("API_TOKEN")`, the lookup order is:

1. `--token <value>` on the command line
2. The `API_TOKEN` environment variable
3. A `.default_value(...)` if one was set
4. `ParseError::MissingFlag` if none of the above provided a value

### Enum/Choice Constraints

Restrict a value-taking flag to a fixed set of choices with `.choices(...)`. The parser returns `ParseError::InvalidChoice` for any value not in the list.

```rust
use argot::Flag;

Flag::builder("strategy")
    .takes_value()
    .choices(["rolling", "blue-green", "canary"])
    .default_value("rolling")
    .build()
    .unwrap()
```

The choice constraint is applied to both command-line values and environment variable fallbacks, so the validation is consistent regardless of how the flag value arrives.

If you call `.choices(...)` with an empty iterator, `build()` returns `BuildError::EmptyChoices`. This catches the mistake at definition time rather than silently accepting all values.

### Environment Variable Fallback

Any value-taking flag can be backed by an environment variable using `.env(var_name)`. This is useful for secrets and configuration that operators prefer not to pass on the command line.

```rust
use argot::Flag;

Flag::builder("database-url")
    .takes_value()
    .required()
    .env("DATABASE_URL")
    .build()
    .unwrap()
```

The env var name is stored on the `Flag` struct's `env` field and is reflected in generated help output, so users know which variable to set.

### Repeatable Flags

Flags that may be specified more than once are declared with `.repeatable()`.

For boolean flags, repeated occurrences are counted and stored as a numeric string:

```rust
// -v -v -v → flags["verbose"] == "3"
Flag::builder("verbose")
    .short('v')
    .repeatable()
    .build()
    .unwrap()
```

For value-taking flags, values are collected into a JSON array string:

```rust
// --tag foo --tag bar → flags["tag"] == r#"["foo","bar"]"#
Flag::builder("tag")
    .takes_value()
    .repeatable()
    .build()
    .unwrap()
```

### Boolean Flag Negation

Boolean flags support `--no-{name}` negation syntax automatically. Passing `--no-dry-run` sets `flags["dry-run"]` to `"false"`. No extra declaration is needed.

### Variadic Arguments

When a command accepts a variable number of positional arguments, declare the last argument as variadic. The parser collects all remaining positional tokens into a JSON array string stored under that argument's name.

```rust
use argot::Argument;

Argument::builder("files")
    .description("One or more files to process")
    .required()
    .variadic()
    .build()
    .unwrap()
// argv: process a.txt b.txt c.txt
// args["files"] == r#"["a.txt","b.txt","c.txt"]"#
```

`CommandBuilder::build()` enforces that the variadic argument is the last argument defined, returning `BuildError::VariadicNotLast` if that constraint is violated.

---

## Handler-Level Validation

After the parser succeeds, the handler receives a `ParsedCommand` with all arguments and flags bound. Use handler validation for:

- Numeric range or format checks
- File/path existence checks
- Cross-field constraint checks (combinations of flags that are mutually invalid)
- Business rule validation (valid environment names, non-empty strings, network reachability)

Return descriptive errors from handlers — they surface directly to the user via `CliError::Handler`.

### Numeric Range Checking

```rust
use std::sync::Arc;
use argot::{Command, Argument};

Command::builder("scale")
    .argument(Argument::builder("workers").required().build().unwrap())
    .handler(Arc::new(|parsed| {
        let workers: u32 = parsed
            .arg("workers")
            .unwrap_or("1")
            .parse()
            .map_err(|_| "workers must be a positive integer")?;

        if workers == 0 || workers > 64 {
            return Err("workers must be between 1 and 64".into());
        }

        println!("Scaling to {} workers", workers);
        Ok(())
    }))
    .build()
    .unwrap();
```

### Path Validation

```rust
use std::sync::Arc;
use argot::{Command, Argument};

Command::builder("load")
    .argument(Argument::builder("file").required().build().unwrap())
    .handler(Arc::new(|parsed| {
        let path = std::path::Path::new(parsed.arg("file").unwrap());

        if !path.exists() {
            return Err(format!("file not found: {}", path.display()).into());
        }
        if !path.is_file() {
            return Err(format!("not a regular file: {}", path.display()).into());
        }

        println!("Loading {}", path.display());
        Ok(())
    }))
    .build()
    .unwrap();
```

### Cross-Field Validation

When two or more flags are mutually exclusive in a way that depends on their values or combined semantics, validate in the handler:

```rust
use std::sync::Arc;
use argot::{Command, Flag};

Command::builder("deploy")
    .flag(Flag::builder("dry-run").short('n').build().unwrap())
    .flag(Flag::builder("force").build().unwrap())
    .handler(Arc::new(|parsed| {
        let dry_run = parsed.flag_bool("dry-run");
        let force = parsed.flag_bool("force");

        if dry_run && force {
            return Err("--dry-run and --force cannot be combined".into());
        }

        if force {
            println!("Deploying with --force");
        } else if dry_run {
            println!("[DRY RUN] No changes will be made");
        } else {
            println!("Deploying");
        }
        Ok(())
    }))
    .build()
    .unwrap();
```

### Validated Struct Pattern

For commands with several flags and arguments, extract a typed configuration struct from `ParsedCommand` before running any business logic. This separates input parsing from execution, makes the handler easier to test, and gives validation errors a single location.

```rust
use std::error::Error;
use argot::ParsedCommand;

struct DeployConfig {
    env: String,
    strategy: String,
    dry_run: bool,
    timeout_secs: Option<u64>,
}

impl DeployConfig {
    fn from_parsed(parsed: &ParsedCommand) -> Result<Self, Box<dyn Error>> {
        let env = parsed
            .arg("environment")
            .ok_or("missing required argument: environment")?
            .to_string();

        let strategy = parsed
            .flag("strategy")
            .unwrap_or("rolling")
            .to_string();

        let valid_strategies = ["rolling", "blue-green", "canary"];
        if !valid_strategies.contains(&strategy.as_str()) {
            return Err(format!(
                "unknown strategy `{}`: expected one of {}",
                strategy,
                valid_strategies.join(", ")
            ).into());
        }

        let timeout_secs = match parsed.flag("timeout") {
            Some(t) => Some(
                t.parse::<u64>()
                    .map_err(|_| format!("timeout must be a positive integer, got `{}`", t))?,
            ),
            None => None,
        };

        Ok(Self {
            env,
            strategy,
            dry_run: parsed.flag_bool("dry-run"),
            timeout_secs,
        })
    }
}

// In the handler:
use std::sync::Arc;
use argot::{Command, Argument, Flag};

Command::builder("deploy")
    .argument(Argument::builder("environment").required().build().unwrap())
    .flag(Flag::builder("strategy").takes_value().default_value("rolling").build().unwrap())
    .flag(Flag::builder("dry-run").short('n').build().unwrap())
    .flag(Flag::builder("timeout").short('t').takes_value().build().unwrap())
    .handler(Arc::new(|parsed| {
        let config = DeployConfig::from_parsed(parsed)?;

        if config.dry_run {
            println!("[DRY RUN] Would deploy to {} using {}", config.env, config.strategy);
            return Ok(());
        }

        println!("Deploying to {} using {} strategy", config.env, config.strategy);
        if let Some(t) = config.timeout_secs {
            println!("Timeout: {}s", t);
        }
        Ok(())
    }))
    .build()
    .unwrap();
```

### Structured Handler Errors with `thiserror`

For commands with several distinct failure modes, define a dedicated error enum so each case has a clear name and message:

```rust
use std::sync::Arc;
use argot::{Command, Argument};

#[derive(Debug, thiserror::Error)]
enum DeployError {
    #[error("environment `{0}` is not recognized; valid environments: prod, staging, dev")]
    UnknownEnvironment(String),
    #[error("environment `{0}` is locked for maintenance")]
    EnvironmentLocked(String),
    #[error("deployment failed: {0}")]
    Io(#[from] std::io::Error),
}

Command::builder("deploy")
    .argument(Argument::builder("env").required().build().unwrap())
    .handler(Arc::new(|parsed| {
        let env = parsed.arg("env").unwrap();

        if !["prod", "staging", "dev"].contains(&env) {
            return Err(Box::new(DeployError::UnknownEnvironment(env.to_string())));
        }

        // Hypothetical check against a lock file:
        let lock_path = format!("/var/locks/{}.lock", env);
        if std::path::Path::new(&lock_path).exists() {
            return Err(Box::new(DeployError::EnvironmentLocked(env.to_string())));
        }

        println!("Deploying to {}", env);
        Ok(())
    }))
    .build()
    .unwrap();
```

The `#[from] std::io::Error` attribute on `Io` allows `?` to convert `std::io::Error` directly into `DeployError::Io` within the handler.

---

## Validation Order

When a user invokes a command, argot applies validation in this order:

1. **Tokenization** — the argv slice is tokenized into words, long flags, short flags, and the `--` separator.
2. **Command resolution** — the first token is resolved to a top-level `Command` via exact match, alias match, or unambiguous prefix. Subcommand tokens are resolved recursively.
3. **Flag binding** — flag tokens are matched against the resolved command's flag definitions. `UnknownFlag`, `FlagMissingValue`, and `InvalidChoice` errors are raised here.
4. **Argument binding** — remaining word tokens are bound to positional argument definitions in declaration order. `UnexpectedArgument` is raised here.
5. **Required validation** — after all tokens are consumed, missing required arguments and flags are detected. `MissingArgument` and `MissingFlag` are raised here. Environment variable fallbacks and defaults are applied during this step.
6. **Handler** — if all of the above steps succeed, the handler is called with the fully-bound `ParsedCommand`. Semantic validation (business rules, cross-field logic, external resource checks) happens here.

If any step fails, the pipeline stops and the error is returned to the caller. The handler is only called when every parser-layer check passes.

---

## Best Practices

**Prefer built-in validation over handler validation when possible.** Using `.required()`, `.choices(...)`, and `.env(...)` is always preferable to repeating those checks in the handler. Built-in constraints are reflected in help output and produce consistent error messages across every command that uses them.

**Use `.choices(...)` for fixed enumerated values.** Rather than checking `if strategy != "rolling" && strategy != "blue-green" ...` in the handler, declare the choices on the flag. The parser enforces them and the help text lists the valid options automatically.

**Reserve handler validation for semantic checks.** File existence, network availability, business rules, numeric ranges, and cross-field constraints belong in the handler. These are things the parser cannot know about.

**Return descriptive errors from handlers.** Handler errors become `CliError::Handler` messages printed to the user. "environment `xyz` is not recognized" is far more useful than "invalid input".

**Use `thiserror` for structured handler errors.** A typed error enum with clear variants makes handler logic easier to read and test. The `#[from]` attribute eliminates manual error conversion boilerplate.

**Extract a config struct for complex commands.** When a handler needs more than two or three values from `ParsedCommand`, introduce a validated config struct. It localizes all input validation, makes the handler body read like business logic, and is easy to unit-test independently of the parser.
