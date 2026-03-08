# Middleware Guide

Argot's middleware system lets you intercept the parse-and-dispatch lifecycle without modifying command handlers. Use it for logging, auditing, authentication checks, metrics, or any cross-cutting concern.

---

## What Middleware Is

When `Cli::run` receives a command, it:

1. Parses `argv` with `Parser`
2. Calls each registered middleware's `before_dispatch` hook
3. Invokes the command handler
4. Calls each registered middleware's `after_dispatch` hook

If parsing fails, `on_parse_error` is called instead of the dispatch hooks.

Middleware fits cleanly into this lifecycle without touching individual command handlers.

---

## The Three Hooks

### `before_dispatch`

```rust
fn before_dispatch(
    &self,
    parsed: &ParsedCommand<'_>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
```

- Fires after a successful parse, before the handler runs.
- Receives the fully-resolved `ParsedCommand` (command metadata + parsed args/flags).
- Return `Ok(())` to proceed with dispatch.
- Return `Err(...)` to abort dispatch; the error surfaces as `CliError::Handler`.

### `after_dispatch`

```rust
fn after_dispatch(
    &self,
    parsed: &ParsedCommand<'_>,
    result: &Result<(), Box<dyn std::error::Error + Send + Sync>>,
)
```

- Fires after the handler returns, whether it succeeded or failed.
- Receives the same `ParsedCommand` and the handler's result.
- Always fires — even when `before_dispatch` aborted or the handler errored.
- Return value is `()` — this hook cannot affect the outcome.

### `on_parse_error`

```rust
fn on_parse_error(&self, error: &ParseError)
```

- Fires when `Parser::parse` returns an error (unknown command, missing argument, etc.).
- The error is also printed to stderr and returned from `Cli::run` as `CliError::Parse`.
- Use this hook to route parse failures to an audit log or error tracker.

---

## Registering Middleware

Use `Cli::with_middleware` to register one or more middlewares. Calls chain fluently:

```rust
use argot::{Cli, Command};

Cli::new(commands)
    .with_middleware(Logger)
    .with_middleware(Audit)
    .run_env_args_and_exit();
```

Middlewares fire in registration order. `Logger`'s `before_dispatch` runs before `Audit`'s.

---

## Example: Request Logging

Log the command name and a Unix timestamp every time a command is dispatched.

```rust
use std::sync::Arc;
use std::time::SystemTime;
use argot::{Argument, Cli, Command, ParsedCommand};
use argot::middleware::Middleware;

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
        eprintln!("[LOG {}] dispatching: {}", ts, parsed.command.canonical);
        Ok(())
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
        .build()
        .unwrap();

    Cli::new(vec![deploy])
        .with_middleware(Logger)
        .run_env_args_and_exit();
}
```

Sample stderr output for `mytool deploy staging`:

```
[LOG 1714000000] dispatching: deploy
Deploying to staging
```

---

## Example: Aborting Dispatch

A `before_dispatch` hook that returns `Err` prevents the handler from running. Use this for authentication checks, rate limiting, or dry-run guards.

```rust
use argot::{Cli, Command, ParsedCommand};
use argot::middleware::Middleware;
use std::sync::Arc;

struct AuthGuard;

impl Middleware for AuthGuard {
    fn before_dispatch(
        &self,
        parsed: &ParsedCommand<'_>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Read a token from the environment.
        let token = std::env::var("API_TOKEN").unwrap_or_default();
        if token.is_empty() {
            return Err(format!(
                "command '{}' requires API_TOKEN to be set",
                parsed.command.canonical
            ).into());
        }
        Ok(())
    }
}

fn main() {
    let deploy = Command::builder("deploy")
        .summary("Deploy the service")
        .handler(Arc::new(|_| {
            println!("Deploying...");
            Ok(())
        }))
        .build()
        .unwrap();

    Cli::new(vec![deploy])
        .with_middleware(AuthGuard)
        .run_env_args_and_exit();
}
```

When `API_TOKEN` is not set:

```
error: handler error: command 'deploy' requires API_TOKEN to be set
```

The handler never runs.

---

## Example: Error Tracking

Use `on_parse_error` and `after_dispatch` together to capture both parse failures and handler failures in one place.

```rust
use argot::{Cli, Command, ParsedCommand};
use argot::middleware::Middleware;
use argot::parser::ParseError;
use std::sync::Arc;

struct ErrorTracker;

impl Middleware for ErrorTracker {
    fn on_parse_error(&self, err: &ParseError) {
        // Send to your error tracker, e.g. Sentry, Datadog, etc.
        eprintln!("[TRACKER] parse failure: {}", err);
    }

    fn after_dispatch(
        &self,
        parsed: &ParsedCommand<'_>,
        result: &Result<(), Box<dyn std::error::Error + Send + Sync>>,
    ) {
        if let Err(e) = result {
            eprintln!(
                "[TRACKER] handler failure: command={} error={}",
                parsed.command.canonical, e
            );
        }
    }
}

fn main() {
    let deploy = Command::builder("deploy")
        .summary("Deploy the service")
        .handler(Arc::new(|_| {
            Err("connection refused".into())
        }))
        .build()
        .unwrap();

    Cli::new(vec![deploy])
        .with_middleware(ErrorTracker)
        .run_env_args_and_exit();
}
```

Running `mytool deploy` with this middleware:

```
[TRACKER] handler failure: command=deploy error=connection refused
error: handler error: connection refused
```

---

## Multiple Middlewares

All middlewares registered with `with_middleware` fire in the order they were registered.

```rust
Cli::new(commands)
    .with_middleware(Logger)    // fires first
    .with_middleware(Audit)     // fires second
    .with_middleware(Metrics)   // fires third
    .run_env_args_and_exit();
```

For `before_dispatch`: if `Logger` returns `Err`, `Audit` and `Metrics` do not fire, and the handler does not run.

For `after_dispatch` and `on_parse_error`: all registered middlewares always fire, regardless of what earlier ones returned.

---

## Key Points

- All three hooks have **default no-op implementations**. Implement only the hooks you need.
- `before_dispatch` is the **only hook that can abort dispatch**. Returning `Err` prevents the handler from running and returns `CliError::Handler` from `Cli::run`.
- `after_dispatch` **always fires** — even when `before_dispatch` aborted or the handler errored. The `result` parameter tells you what happened.
- `on_parse_error` fires for **parser-level failures only** (before any dispatch occurs).
- Middleware must implement `Send + Sync` to be safely shared across threads.
- Add as many middlewares as needed; there is no limit on registrations.
