//! High-level CLI entry point that wires together the argot pipeline.
//!
//! [`Cli`] is a batteries-included struct that combines [`Registry`],
//! [`Parser`] and the render layer into a single `run` method.
//! It handles the common built-in behaviors (help, version, empty input) so
//! that application code only needs to build commands and register handlers.
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use argot_cmd::{Cli, Command};
//!
//! let cmd = Command::builder("greet")
//!     .summary("Say hello")
//!     .handler(Arc::new(|_| {
//!         println!("Hello, world!");
//!         Ok(())
//!     }))
//!     .build()
//!     .unwrap();
//!
//! let cli = Cli::new(vec![cmd])
//!     .app_name("myapp")
//!     .version("1.0.0");
//!
//! // In a real application:
//! // cli.run_env_args().unwrap();
//! ```

use crate::parser::{ParseError, Parser};
use crate::query::Registry;
use crate::render::{DefaultRenderer, Renderer};
use crate::resolver::Resolver;

/// Errors produced by [`Cli::run`].
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// A parse error occurred (unknown command, missing argument, etc.).
    ///
    /// When this variant is returned, `Cli::run` also prints the error and
    /// best-effort help to stderr before returning.
    #[error(transparent)]
    Parse(#[from] ParseError),
    /// The matched command has no handler registered.
    ///
    /// The inner `String` is the canonical name of the command.
    #[error("command `{0}` has no handler registered")]
    NoHandler(String),
    /// The registered handler returned an error.
    ///
    /// The inner boxed error carries the handler's error message.
    #[error("handler error: {0}")]
    Handler(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// A batteries-included entry point that wires together [`Registry`], [`Parser`],
/// and the render layer so callers do not have to do it themselves.
///
/// Build a `Cli` with [`Cli::new`], optionally configure it with
/// [`Cli::app_name`] and [`Cli::version`], then call [`Cli::run`] (or
/// [`Cli::run_env_args`] for the common case of reading from
/// [`std::env::args`]).
///
/// ## Built-in behaviors
///
/// | Input | Behavior |
/// |-------|----------|
/// | `--help` / `-h` anywhere | Print help for the most-specific resolved command; return `Ok(())`. |
/// | `--version` / `-V` | Print `"<app_name> <version>"` (or just the version); return `Ok(())`. |
/// | Empty argument list | Print the top-level command listing; return `Ok(())`. |
/// | Unrecognized command | Print error + help to stderr; return `Err(CliError::Parse(...))`. |
///
/// # Examples
///
/// ```
/// # use std::sync::Arc;
/// # use argot_cmd::{Cli, Command};
/// let cli = Cli::new(vec![
///     Command::builder("ping")
///         .summary("Check connectivity")
///         .handler(Arc::new(|_| { println!("pong"); Ok(()) }))
///         .build()
///         .unwrap(),
/// ])
/// .app_name("myapp")
/// .version("0.1.0");
///
/// // Invoking with no args prints the command list (does not error).
/// assert!(cli.run(std::iter::empty::<&str>()).is_ok());
/// ```
pub struct Cli {
    registry: Registry,
    app_name: String,
    version: Option<String>,
    middlewares: Vec<Box<dyn crate::middleware::Middleware>>,
    renderer: Box<dyn Renderer>,
    query_support: bool,
    /// When `true`, a warning is emitted to stderr at dispatch time for mutating
    /// commands that have no `--dry-run` flag defined.
    warn_missing_dry_run: bool,
}

impl Cli {
    /// Create a new `Cli` from a list of top-level commands.
    ///
    /// # Arguments
    ///
    /// - `commands` — The fully-built top-level command list. Ownership is
    ///   transferred to an internal [`Registry`].
    pub fn new(commands: Vec<crate::model::Command>) -> Self {
        Self {
            registry: Registry::new(commands),
            app_name: String::new(),
            version: None,
            middlewares: vec![],
            renderer: Box::new(DefaultRenderer),
            query_support: false,
            warn_missing_dry_run: false,
        }
    }

    /// Set the application name (shown in version output and top-level help).
    ///
    /// If not set, the version string is printed without a prefix.
    pub fn app_name(mut self, name: impl Into<String>) -> Self {
        self.app_name = name.into();
        self
    }

    /// Set the application version (shown by `--version` / `-V`).
    ///
    /// If not set, `"(no version set)"` is printed.
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Register a middleware that hooks into the parse-and-dispatch lifecycle.
    ///
    /// Middlewares are invoked in registration order. Multiple middlewares can
    /// be added by calling `with_middleware` repeatedly.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use argot_cmd::{Cli, Command, middleware::Middleware};
    ///
    /// struct Audit;
    /// impl Middleware for Audit {
    ///     fn before_dispatch(&self, parsed: &argot_cmd::ParsedCommand<'_>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///         eprintln!("audit: {}", parsed.command.canonical);
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let cli = Cli::new(vec![Command::builder("run").build().unwrap()])
    ///     .with_middleware(Audit);
    /// ```
    pub fn with_middleware<M: crate::middleware::Middleware + 'static>(mut self, m: M) -> Self {
        self.middlewares.push(Box::new(m));
        self
    }

    /// Replace the default renderer with a custom implementation.
    ///
    /// The renderer is used for all help text, Markdown, subcommand listings,
    /// and ambiguity messages produced by this `Cli` instance.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use argot_cmd::{Cli, Command, render::Renderer};
    /// struct MyRenderer;
    /// impl Renderer for MyRenderer {
    ///     fn render_help(&self, cmd: &argot_cmd::Command) -> String { format!("HELP: {}", cmd.canonical) }
    ///     fn render_markdown(&self, cmd: &argot_cmd::Command) -> String { String::new() }
    ///     fn render_subcommand_list(&self, cmds: &[argot_cmd::Command]) -> String { String::new() }
    ///     fn render_ambiguity(&self, input: &str, _: &[String]) -> String { format!("bad: {}", input) }
    /// }
    ///
    /// let cli = Cli::new(vec![Command::builder("run").build().unwrap()])
    ///     .with_renderer(MyRenderer);
    /// ```
    pub fn with_renderer<R: Renderer + 'static>(mut self, renderer: R) -> Self {
        self.renderer = Box::new(renderer);
        self
    }

    /// Enable agent-discovery query support.
    ///
    /// When enabled, the CLI recognises a built-in `query` command:
    ///
    /// ```text
    /// tool query commands                          # list all commands as JSON
    /// tool query commands --stream                 # NDJSON: one object per line
    /// tool query commands --fields canonical,summary
    /// tool query commands --stream --fields canonical,summary
    /// tool query <name>                            # get structured JSON for one command
    /// tool query <name> --stream                   # single compact JSON line
    /// ```
    ///
    /// The `query` command is also injected into the registry so that it
    /// appears in `--help` output and in [`Registry::iter_all_recursive`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use argot_cmd::{Cli, Command};
    ///
    /// let cli = Cli::new(vec![Command::builder("deploy").build().unwrap()])
    ///     .with_query_support();
    /// // Now: `tool query commands` and `tool query deploy` work.
    /// // Also: `tool query commands --stream` and `tool query deploy --stream`.
    /// ```
    pub fn with_query_support(mut self) -> Self {
        self.query_support = true;
        // Inject a meta `query` command so it shows up in --help and iter_all_recursive.
        let query_cmd = crate::model::Command::builder("query")
            .summary("Query command metadata (agent discovery)")
            .description(
                "Structured JSON output for agent discovery. \
                 `query commands` lists all commands; `query <name>` returns metadata for one. \
                 Use `--fields <csv>` to request only specific top-level fields, reducing output \
                 size for agents that only need a subset of command metadata.",
            )
            .flag(
                crate::model::Flag::builder("fields")
                    .description(
                        "Comma-separated list of top-level fields to include in JSON output \
                         (e.g. `canonical,summary,examples`). When omitted all fields are returned.",
                    )
                    .takes_value()
                    .build()
                    .expect("built-in fields flag should always build"),
            )
            .example(crate::model::Example::new(
                "query commands",
                "List all commands as JSON",
            ))
            .example(crate::model::Example::new(
                "query deploy",
                "Get metadata for the deploy command",
            ))
            .example(crate::model::Example::new(
                "query deploy --fields canonical,summary,examples",
                "Get only canonical name, summary, and examples for the deploy command",
            ))
            .example(crate::model::Example::new(
                "query commands --fields canonical,summary",
                "List all commands showing only canonical name and summary",
            ))
            .build()
            .expect("built-in query command should always build");
        self.registry.push(query_cmd);
        self
    }

    /// Enable advisory warnings for mutating commands that have no `--dry-run` flag.
    ///
    /// When enabled (default: off), `Cli::run` (and `Cli::run_async`) will emit a
    /// warning to stderr before dispatching a mutating command that has no `--dry-run`
    /// flag defined on it:
    ///
    /// ```text
    /// warning: mutating command 'delete' has no --dry-run flag defined
    /// ```
    ///
    /// This is an advisory lint, not a hard error. It helps developers notice
    /// missing safety flags while building CLIs with argot.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::sync::Arc;
    /// use argot_cmd::{Cli, Command};
    ///
    /// let cli = Cli::new(vec![
    ///     Command::builder("delete")
    ///         .summary("Delete a resource")
    ///         .mutating()
    ///         .handler(Arc::new(|_| Ok(())))
    ///         .build()
    ///         .unwrap(),
    /// ])
    /// .warn_missing_dry_run(true);
    /// // Running `delete` will now emit a warning to stderr.
    /// ```
    pub fn warn_missing_dry_run(mut self, enabled: bool) -> Self {
        self.warn_missing_dry_run = enabled;
        self
    }

    /// Parse and dispatch a command from an iterator of string arguments.
    ///
    /// The iterator should **not** include the program name (`argv[0]`).
    ///
    /// Built-in behaviors:
    /// - `--help` or `-h` anywhere → print help for the most-specific matched
    ///   command and return `Ok(())`.
    /// - `--version` or `-V` → print version string and return `Ok(())`.
    /// - Empty input → print top-level command list and return `Ok(())`.
    /// - Parse error → print the error to stderr, then help if possible; return
    ///   `Err(CliError::Parse(...))`.
    /// - No handler registered → return `Err(CliError::NoHandler(...))`.
    ///
    /// # Arguments
    ///
    /// - `args` — Iterator of argument strings, not including the program name.
    ///
    /// # Errors
    ///
    /// - [`CliError::Parse`] — the argument list could not be parsed.
    /// - [`CliError::NoHandler`] — the resolved command has no handler.
    /// - [`CliError::Handler`] — the handler returned an error.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::sync::Arc;
    /// # use argot_cmd::{Cli, Command, CliError};
    /// let cli = Cli::new(vec![
    ///     Command::builder("hello")
    ///         .handler(Arc::new(|_| Ok(())))
    ///         .build()
    ///         .unwrap(),
    /// ]);
    ///
    /// assert!(cli.run(["hello"]).is_ok());
    /// assert!(matches!(cli.run(["--help"]), Ok(())));
    /// ```
    pub fn run(&self, args: impl IntoIterator<Item = impl AsRef<str>>) -> Result<(), CliError> {
        let argv: Vec<String> = args.into_iter().map(|a| a.as_ref().to_owned()).collect();
        let argv_refs: Vec<&str> = argv.iter().map(String::as_str).collect();

        // ── Built-in: query support ───────────────────────────────────────────
        if self.query_support && argv_refs.first().copied() == Some("query") {
            return self.handle_query(&argv_refs[1..]);
        }

        // ── Built-in: --help / -h ──────────────────────────────────────────
        if argv_refs.iter().any(|a| *a == "--help" || *a == "-h") {
            // Strip the help flag(s) and try to identify the target command.
            let remaining: Vec<&str> = argv_refs
                .iter()
                .copied()
                .filter(|a| *a != "--help" && *a != "-h")
                .collect();

            let help_text = self.resolve_help_text(&remaining);
            print!("{}", help_text);
            return Ok(());
        }

        // ── Built-in: --version / -V ──────────────────────────────────────
        if argv_refs.iter().any(|a| *a == "--version" || *a == "-V") {
            match &self.version {
                Some(v) if !self.app_name.is_empty() => println!("{} {}", self.app_name, v),
                Some(v) => println!("{}", v),
                None => println!("(no version set)"),
            }
            return Ok(());
        }

        // ── Built-in: empty args → list top-level commands ────────────────
        if argv_refs.is_empty() {
            print!(
                "{}",
                self.renderer
                    .render_subcommand_list(self.registry.commands())
            );
            return Ok(());
        }

        // ── Normal parse ──────────────────────────────────────────────────
        let parser = Parser::new(self.registry.commands());
        match parser.parse(&argv_refs) {
            Ok(parsed) => {
                // Advisory warning: mutating command without --dry-run flag
                if self.warn_missing_dry_run
                    && parsed.command.mutating
                    && !parsed.command.flags.iter().any(|f| f.name == "dry-run")
                {
                    eprintln!(
                        "warning: mutating command '{}' has no --dry-run flag defined",
                        parsed.command.canonical
                    );
                }

                // Before dispatch: run middleware hooks
                for mw in &self.middlewares {
                    mw.before_dispatch(&parsed).map_err(CliError::Handler)?;
                }

                // Call handler
                let handler_result = match &parsed.command.handler {
                    Some(handler) => {
                        // HandlerFn returns Box<dyn Error> (no Send+Sync bound).
                        // We convert manually to match CliError::Handler.
                        handler(&parsed).map_err(|e| {
                            // Wrap in a Send+Sync-compatible error by capturing
                            // the display string.
                            let msg = e.to_string();
                            let boxed: Box<dyn std::error::Error + Send + Sync> = msg.into();
                            CliError::Handler(boxed)
                        })
                    }
                    None => Err(CliError::NoHandler(parsed.command.canonical.to_string())),
                };

                // After dispatch: run middleware hooks (even on error)
                let handler_result_for_mw: Result<(), Box<dyn std::error::Error + Send + Sync>> =
                    match &handler_result {
                        Ok(()) => Ok(()),
                        Err(e) => Err(Box::<dyn std::error::Error + Send + Sync>::from(
                            e.to_string(),
                        )),
                    };
                for mw in &self.middlewares {
                    mw.after_dispatch(&parsed, &handler_result_for_mw);
                }

                handler_result
            }
            Err(parse_err) => {
                // Fire on_parse_error middleware hooks
                for mw in &self.middlewares {
                    mw.on_parse_error(&parse_err);
                }

                eprintln!("error: {}", parse_err);
                if let crate::parser::ParseError::Resolve(
                    crate::resolver::ResolveError::Unknown {
                        ref suggestions, ..
                    },
                ) = parse_err
                {
                    if !suggestions.is_empty() {
                        eprintln!("Did you mean one of: {}", suggestions.join(", "));
                    }
                }
                // Best-effort: render help for whatever partial command we can resolve.
                let help_text = self.resolve_help_text(&argv_refs);
                eprint!("{}", help_text);
                Err(CliError::Parse(parse_err))
            }
        }
    }

    /// Convenience: run with `std::env::args().skip(1)`.
    ///
    /// Equivalent to `self.run(std::env::args().skip(1))`. Skipping element 0
    /// is required because `std::env::args` includes the program name.
    ///
    /// # Errors
    ///
    /// Same as [`Cli::run`].
    pub fn run_env_args(&self) -> Result<(), CliError> {
        self.run(std::env::args().skip(1))
    }

    /// Parse, dispatch, and exit the process with an appropriate exit code.
    ///
    /// On success exits with code `0`. On any error, prints the error to `stderr`
    /// and exits with code `1`.
    ///
    /// This is the recommended entry point for binary crates that want `main`
    /// to be a one-liner:
    ///
    /// ```no_run
    /// use argot_cmd::{Cli, Command};
    /// use std::sync::Arc;
    ///
    /// fn main() {
    ///     Cli::new(vec![
    ///         Command::builder("run")
    ///             .handler(Arc::new(|_| Ok(())))
    ///             .build()
    ///             .unwrap(),
    ///     ])
    ///     .run_env_args_and_exit();
    /// }
    /// ```
    ///
    /// # Panics
    ///
    /// Does not panic; all errors are handled by printing to stderr and exiting.
    pub fn run_and_exit(&self, args: impl IntoIterator<Item = impl AsRef<str>>) -> ! {
        match self.run(args) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("error: {}", e);
                std::process::exit(1);
            }
        }
    }

    /// Convenience: [`run_and_exit`][Self::run_and_exit] using `std::env::args().skip(1)`.
    pub fn run_env_args_and_exit(&self) -> ! {
        self.run_and_exit(std::env::args().skip(1))
    }

    /// Async version of [`run_and_exit`][Self::run_and_exit].
    ///
    /// Must be called from an async context (e.g., `#[tokio::main]`).
    #[cfg(feature = "async")]
    pub async fn run_async_and_exit(&self, args: impl IntoIterator<Item = impl AsRef<str>>) -> ! {
        match self.run_async(args).await {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("error: {}", e);
                std::process::exit(1);
            }
        }
    }

    /// Convenience: [`run_async_and_exit`][Self::run_async_and_exit] using `std::env::args().skip(1)`.
    #[cfg(feature = "async")]
    pub async fn run_env_args_async_and_exit(&self) -> ! {
        self.run_async_and_exit(std::env::args().skip(1)).await
    }

    /// Parse and dispatch a command asynchronously.
    ///
    /// Behaves identically to [`Cli::run`] but also invokes
    /// [`AsyncHandlerFn`][crate::model::AsyncHandlerFn] handlers
    /// registered with [`crate::CommandBuilder::async_handler`].
    ///
    /// Must be called from an async context (e.g., inside `#[tokio::main]`).
    ///
    /// Dispatch priority: async handler → sync handler → `CliError::NoHandler`.
    ///
    /// # Feature
    ///
    /// Requires the `async` feature flag.
    ///
    /// # Errors
    ///
    /// Same variants as [`Cli::run`].
    #[cfg(feature = "async")]
    pub async fn run_async(
        &self,
        args: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<(), CliError> {
        let args: Vec<String> = args.into_iter().map(|a| a.as_ref().to_string()).collect();
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();

        // ── Built-in: query support ───────────────────────────────────────────
        if self.query_support && argv.first().copied() == Some("query") {
            let refs: Vec<&str> = argv.to_vec();
            return self.handle_query(&refs[1..]);
        }

        // ── Built-in: --help / -h ──────────────────────────────────────────
        if argv.iter().any(|a| *a == "--help" || *a == "-h") {
            let remaining: Vec<&str> = argv
                .iter()
                .copied()
                .filter(|a| *a != "--help" && *a != "-h")
                .collect();
            let help_text = self.resolve_help_text(&remaining);
            print!("{}", help_text);
            return Ok(());
        }

        // ── Built-in: --version / -V ──────────────────────────────────────
        if argv.iter().any(|a| *a == "--version" || *a == "-V") {
            match &self.version {
                Some(v) if !self.app_name.is_empty() => println!("{} {}", self.app_name, v),
                Some(v) => println!("{}", v),
                None => println!("(no version set)"),
            }
            return Ok(());
        }

        // ── Built-in: empty args → list top-level commands ────────────────
        if argv.is_empty() {
            print!(
                "{}",
                self.renderer
                    .render_subcommand_list(self.registry.commands())
            );
            return Ok(());
        }

        // ── Normal parse ──────────────────────────────────────────────────
        let parser = Parser::new(self.registry.commands());
        match parser.parse(&argv) {
            Ok(parsed) => {
                // Advisory warning: mutating command without --dry-run flag
                if self.warn_missing_dry_run
                    && parsed.command.mutating
                    && !parsed.command.flags.iter().any(|f| f.name == "dry-run")
                {
                    eprintln!(
                        "warning: mutating command '{}' has no --dry-run flag defined",
                        parsed.command.canonical
                    );
                }

                // Before dispatch: run middleware hooks
                for mw in &self.middlewares {
                    mw.before_dispatch(&parsed).map_err(CliError::Handler)?;
                }

                // Prefer async handler over sync handler
                let handler_result = if let Some(ref async_handler) = parsed.command.async_handler {
                    async_handler(&parsed).await.map_err(|e| {
                        let msg = e.to_string();
                        let boxed: Box<dyn std::error::Error + Send + Sync> = msg.into();
                        CliError::Handler(boxed)
                    })
                } else if let Some(ref handler) = parsed.command.handler {
                    handler(&parsed).map_err(|e| {
                        let msg = e.to_string();
                        let boxed: Box<dyn std::error::Error + Send + Sync> = msg.into();
                        CliError::Handler(boxed)
                    })
                } else {
                    Err(CliError::NoHandler(parsed.command.canonical.clone()))
                };

                // After dispatch: run middleware hooks (even on error)
                let handler_result_for_mw: Result<(), Box<dyn std::error::Error + Send + Sync>> =
                    match &handler_result {
                        Ok(()) => Ok(()),
                        Err(e) => Err(Box::<dyn std::error::Error + Send + Sync>::from(
                            e.to_string(),
                        )),
                    };
                for mw in &self.middlewares {
                    mw.after_dispatch(&parsed, &handler_result_for_mw);
                }

                handler_result
            }
            Err(parse_err) => {
                // Fire on_parse_error middleware hooks
                for mw in &self.middlewares {
                    mw.on_parse_error(&parse_err);
                }

                eprintln!("error: {}", parse_err);
                if let crate::parser::ParseError::Resolve(
                    crate::resolver::ResolveError::Unknown {
                        ref suggestions, ..
                    },
                ) = parse_err
                {
                    if !suggestions.is_empty() {
                        eprintln!("Did you mean one of: {}", suggestions.join(", "));
                    }
                }
                let help_text = self.resolve_help_text(&argv);
                eprint!("{}", help_text);
                Err(CliError::Parse(parse_err))
            }
        }
    }

    /// Convenience: `run_async` using `std::env::args().skip(1)`.
    #[cfg(feature = "async")]
    pub async fn run_env_args_async(&self) -> Result<(), CliError> {
        self.run_async(std::env::args().skip(1)).await
    }

    // ── Private helpers ───────────────────────────────────────────────────

    fn handle_query(&self, args: &[&str]) -> Result<(), CliError> {
        // Strip --json flag (JSON is always the output format; --json accepted for compatibility).
        // Extract --stream and --fields flags; --json is a no-op for compat.
        let mut stream = false;
        let mut fields_opt: Option<String> = None;
        let mut positional: Vec<&str> = Vec::new();

        let mut iter = args.iter().copied().peekable();
        while let Some(arg) = iter.next() {
            if arg == "--json" {
                // accepted for compatibility, no-op
            } else if arg == "--stream" {
                stream = true;
            } else if arg == "--fields" {
                if let Some(val) = iter.next() {
                    fields_opt = Some(val.to_owned());
                }
            } else if let Some(val) = arg.strip_prefix("--fields=") {
                fields_opt = Some(val.to_owned());
            } else {
                positional.push(arg);
            }
        }
        let args = positional.as_slice();

        let field_strings: Vec<String> = fields_opt
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(|f| f.trim().to_owned())
            .filter(|f| !f.is_empty())
            .collect();
        let fields: Vec<&str> = field_strings.iter().map(String::as_str).collect();

        match args.first().copied() {
            // `query commands` → JSON array of all top-level commands (or NDJSON if --stream)
            None | Some("commands") => {
                if stream {
                    let ndjson = self
                        .registry
                        .to_ndjson_with_fields(&fields)
                        .map_err(|e| {
                            CliError::Handler(Box::<dyn std::error::Error + Send + Sync>::from(
                                e.to_string(),
                            ))
                        })?;
                    print!("{}", ndjson);
                } else {
                    let json = self
                        .registry
                        .to_json_with_fields(&fields)
                        .map_err(|e| {
                            CliError::Handler(Box::<dyn std::error::Error + Send + Sync>::from(
                                e.to_string(),
                            ))
                        })?;
                    println!("{}", json);
                }
                Ok(())
            }
            // `query examples <name>` → JSON array of examples for the named command
            Some("examples") => {
                let name = args.get(1).copied().ok_or_else(|| {
                    CliError::Handler(Box::<dyn std::error::Error + Send + Sync>::from(
                        "usage: query examples <command-name>",
                    ))
                })?;
                let cmd = self
                    .registry
                    .get_command(name)
                    .or_else(|| {
                        let resolver = crate::resolver::Resolver::new(self.registry.commands());
                        resolver.resolve(name).ok()
                    })
                    .ok_or_else(|| {
                        CliError::Handler(Box::<dyn std::error::Error + Send + Sync>::from(
                            format!("unknown command: `{}`", name),
                        ))
                    })?;
                let json = serde_json::to_string_pretty(&cmd.examples).map_err(|e| {
                    CliError::Handler(Box::<dyn std::error::Error + Send + Sync>::from(
                        e.to_string(),
                    ))
                })?;
                println!("{}", json);
                Ok(())
            }
            // `query <name>` → JSON (or NDJSON if --stream) for the named command
            Some(name) => {
                // First try exact match, then resolver (which handles prefix/alias).
                let cmd = self.registry.get_command(name);
                if let Some(cmd) = cmd {
                    if stream {
                        let line = crate::query::command_to_ndjson(cmd).map_err(|e| {
                            CliError::Handler(Box::<dyn std::error::Error + Send + Sync>::from(
                                e.to_string(),
                            ))
                        })?;
                        println!("{}", line);
                    } else {
                        let json =
                            crate::query::command_to_json_with_fields(cmd, &fields).map_err(|e| {
                                CliError::Handler(Box::<dyn std::error::Error + Send + Sync>::from(
                                    e.to_string(),
                                ))
                            })?;
                        println!("{}", json);
                    }
                    return Ok(());
                }

                // Try resolver for prefix/alias matching; handle ambiguity with structured JSON.
                let resolver = crate::resolver::Resolver::new(self.registry.commands());
                match resolver.resolve(name) {
                    Ok(cmd) => {
                        if stream {
                            let line = crate::query::command_to_ndjson(cmd).map_err(|e| {
                                CliError::Handler(Box::<dyn std::error::Error + Send + Sync>::from(
                                    e.to_string(),
                                ))
                            })?;
                            println!("{}", line);
                        } else {
                            let json = crate::query::command_to_json_with_fields(cmd, &fields)
                                .map_err(|e| {
                                    CliError::Handler(
                                        Box::<dyn std::error::Error + Send + Sync>::from(
                                            e.to_string(),
                                        ),
                                    )
                                })?;
                            println!("{}", json);
                        }
                        Ok(())
                    }
                    Err(crate::resolver::ResolveError::Ambiguous { input, candidates }) => {
                        // Agents should receive data, not errors — emit structured JSON.
                        let json = serde_json::json!({
                            "error": "ambiguous",
                            "input": input,
                            "candidates": candidates,
                        });
                        println!("{}", json);
                        Ok(())
                    }
                    Err(crate::resolver::ResolveError::Unknown { .. }) => Err(CliError::Handler(
                        Box::<dyn std::error::Error + Send + Sync>::from(format!(
                            "unknown command: `{}`",
                            name
                        )),
                    )),
                }
            }
        }
    }

    /// Walk the arg list and return the help text for the deepest command that
    /// can be resolved. Falls back to the top-level command list if nothing
    /// resolves.
    fn resolve_help_text(&self, argv: &[&str]) -> String {
        // Try to walk the command tree as far as possible.
        if argv.is_empty() {
            return self
                .renderer
                .render_subcommand_list(self.registry.commands());
        }

        // Skip any flag-looking tokens for the purpose of command resolution.
        let words: Vec<&str> = argv
            .iter()
            .copied()
            .filter(|a| !a.starts_with('-'))
            .collect();

        if words.is_empty() {
            return self
                .renderer
                .render_subcommand_list(self.registry.commands());
        }

        // Resolve the first word as a top-level command.
        let resolver = Resolver::new(self.registry.commands());
        let top_cmd = match resolver.resolve(words[0]) {
            Ok(cmd) => cmd,
            Err(_) => {
                return self
                    .renderer
                    .render_subcommand_list(self.registry.commands())
            }
        };

        // Walk into subcommands as far as possible.
        let mut current = top_cmd;
        for word in words.iter().skip(1) {
            if current.subcommands.is_empty() {
                break;
            }
            let sub_resolver = Resolver::new(&current.subcommands);
            match sub_resolver.resolve(word) {
                Ok(sub) => current = sub,
                Err(_) => break,
            }
        }

        self.renderer.render_help(current)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Command;
    use std::sync::{Arc, Mutex};

    fn make_cli_no_handler() -> Cli {
        let cmd = Command::builder("greet")
            .summary("Say hello")
            .build()
            .unwrap();
        Cli::new(vec![cmd]).app_name("testapp").version("1.2.3")
    }

    fn make_cli_with_handler(called: Arc<Mutex<bool>>) -> Cli {
        let cmd = Command::builder("greet")
            .summary("Say hello")
            .handler(Arc::new(move |_parsed| {
                *called.lock().unwrap() = true;
                Ok(())
            }))
            .build()
            .unwrap();
        Cli::new(vec![cmd]).app_name("testapp").version("1.2.3")
    }

    #[test]
    fn test_run_empty_args() {
        let cli = make_cli_no_handler();
        let result = cli.run(std::iter::empty::<&str>());
        assert!(result.is_ok(), "empty args should return Ok");
    }

    #[test]
    fn test_run_help_flag() {
        let cli = make_cli_no_handler();
        let result = cli.run(["--help"]);
        assert!(result.is_ok(), "--help should return Ok");
    }

    #[test]
    fn test_run_help_flag_short() {
        let cli = make_cli_no_handler();
        let result = cli.run(["-h"]);
        assert!(result.is_ok(), "-h should return Ok");
    }

    #[test]
    fn test_run_version_flag() {
        let cli = make_cli_no_handler();
        let result = cli.run(["--version"]);
        assert!(result.is_ok(), "--version should return Ok");
    }

    #[test]
    fn test_run_version_flag_short() {
        let cli = make_cli_no_handler();
        let result = cli.run(["-V"]);
        assert!(result.is_ok(), "-V should return Ok");
    }

    #[test]
    fn test_run_no_handler() {
        let cli = make_cli_no_handler();
        let result = cli.run(["greet"]);
        assert!(
            matches!(result, Err(CliError::NoHandler(ref name)) if name == "greet"),
            "expected NoHandler(\"greet\"), got {:?}",
            result
        );
    }

    #[test]
    fn test_run_with_handler() {
        let called = Arc::new(Mutex::new(false));
        let cli = make_cli_with_handler(called.clone());
        let result = cli.run(["greet"]);
        assert!(result.is_ok(), "handler should succeed, got {:?}", result);
        assert!(*called.lock().unwrap(), "handler should have been called");
    }

    #[test]
    fn test_run_unknown_command() {
        let cli = make_cli_no_handler();
        let result = cli.run(["unknowncmd"]);
        assert!(
            matches!(result, Err(CliError::Parse(_))),
            "unknown command should yield Parse error, got {:?}",
            result
        );
    }

    #[test]
    fn test_run_handler_error_wrapped() {
        use std::sync::Arc;
        let cmd = crate::model::Command::builder("fail")
            .handler(Arc::new(|_| {
                Err(Box::<dyn std::error::Error>::from("something went wrong"))
            }))
            .build()
            .unwrap();
        let cli = super::Cli::new(vec![cmd]);
        let result = cli.run(["fail"]);
        assert!(result.is_err());
        match result {
            Err(super::CliError::Handler(e)) => {
                assert!(e.to_string().contains("something went wrong"));
            }
            other => panic!("expected CliError::Handler, got {:?}", other),
        }
    }

    #[test]
    fn test_run_command_named_help_dispatches_correctly() {
        // A command named "help" passed through Cli should be dispatched,
        // not intercepted as a built-in --help flag.
        // This verifies Cli only intercepts "--help" flag, not the word "help".
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let called = Arc::new(AtomicBool::new(false));
        let called2 = called.clone();
        let cmd = crate::model::Command::builder("help")
            .handler(Arc::new(move |_| {
                called2.store(true, Ordering::SeqCst);
                Ok(())
            }))
            .build()
            .unwrap();
        let cli = super::Cli::new(vec![cmd]);
        cli.run(["help"]).unwrap();
        assert!(
            called.load(Ordering::SeqCst),
            "handler should have been called"
        );
    }

    #[test]
    fn test_middleware_before_dispatch_called() {
        use crate::middleware::Middleware;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        struct Flag(Arc<AtomicBool>);
        impl Middleware for Flag {
            fn before_dispatch(
                &self,
                _: &crate::model::ParsedCommand<'_>,
            ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                self.0.store(true, Ordering::SeqCst);
                Ok(())
            }
        }

        let called = Arc::new(AtomicBool::new(false));
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called2 = handler_called.clone();

        let cmd = crate::model::Command::builder("run")
            .handler(std::sync::Arc::new(move |_| {
                handler_called2.store(true, Ordering::SeqCst);
                Ok(())
            }))
            .build()
            .unwrap();

        let cli = super::Cli::new(vec![cmd]).with_middleware(Flag(called.clone()));
        cli.run(["run"]).unwrap();

        assert!(called.load(Ordering::SeqCst));
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_middleware_can_abort_dispatch() {
        use crate::middleware::Middleware;
        struct Aborter;
        impl Middleware for Aborter {
            fn before_dispatch(
                &self,
                _: &crate::model::ParsedCommand<'_>,
            ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                Err("aborted by middleware".into())
            }
        }

        let cmd = crate::model::Command::builder("run")
            .handler(std::sync::Arc::new(|_| panic!("should not be called")))
            .build()
            .unwrap();

        let cli = super::Cli::new(vec![cmd]).with_middleware(Aborter);
        assert!(cli.run(["run"]).is_err());
    }

    #[test]
    fn test_query_commands_outputs_json() {
        use crate::model::Command;
        let cli = super::Cli::new(vec![
            Command::builder("deploy")
                .summary("Deploy")
                .build()
                .unwrap(),
            Command::builder("status")
                .summary("Status")
                .build()
                .unwrap(),
        ])
        .with_query_support();

        // Should not error (we can't easily capture stdout in unit tests,
        // but we verify the dispatch path succeeds).
        assert!(cli.run(["query", "commands"]).is_ok());
    }

    #[test]
    fn test_query_named_command_outputs_json() {
        use crate::model::Command;
        let cli = super::Cli::new(vec![Command::builder("deploy")
            .summary("Deploy svc")
            .build()
            .unwrap()])
        .with_query_support();

        assert!(cli.run(["query", "deploy"]).is_ok());
    }

    #[test]
    fn test_query_unknown_command_errors() {
        use crate::model::Command;
        let cli =
            super::Cli::new(vec![Command::builder("deploy").build().unwrap()]).with_query_support();

        assert!(cli.run(["query", "nonexistent"]).is_err());
    }

    #[test]
    fn test_query_meta_command_appears_in_registry() {
        use crate::model::Command;
        let cli =
            super::Cli::new(vec![Command::builder("run").build().unwrap()]).with_query_support();

        // The injected `query` command should be discoverable.
        assert!(cli.registry.get_command("query").is_some());
    }

    #[test]
    fn test_query_with_json_flag() {
        use crate::model::Command;
        let cli = super::Cli::new(vec![Command::builder("deploy")
            .summary("Deploy")
            .build()
            .unwrap()])
        .with_query_support();
        // --json flag must not cause an error
        assert!(cli.run(["query", "deploy", "--json"]).is_ok());
        assert!(cli.run(["query", "commands", "--json"]).is_ok());
    }

    #[test]
    fn test_query_ambiguous_returns_structured_json() {
        use crate::model::Command;
        // Two commands sharing the prefix "dep" make resolution ambiguous.
        let cli = super::Cli::new(vec![
            Command::builder("deploy")
                .summary("Deploy")
                .build()
                .unwrap(),
            Command::builder("describe")
                .summary("Describe")
                .build()
                .unwrap(),
        ])
        .with_query_support();

        // Before the fix this would have returned Err; now it must return Ok(())
        // and print structured JSON to stdout.
        let result = cli.run(["query", "dep"]);
        assert!(
            result.is_ok(),
            "ambiguous query should return Ok(()) with JSON on stdout, got {:?}",
            result
        );
    }

    #[test]
    fn test_query_examples_returns_examples() {
        use crate::model::{Command, Example};
        let cli = super::Cli::new(vec![Command::builder("deploy")
            .summary("Deploy svc")
            .example(Example::new(
                "Deploy to production",
                "deploy api --env prod",
            ))
            .build()
            .unwrap()])
        .with_query_support();

        let result = cli.run(["query", "examples", "deploy"]);
        assert!(
            result.is_ok(),
            "query examples for known command should return Ok(()), got {:?}",
            result
        );
    }

    #[test]
    fn test_query_examples_unknown_errors() {
        use crate::model::Command;
        let cli =
            super::Cli::new(vec![Command::builder("deploy").build().unwrap()]).with_query_support();

        let result = cli.run(["query", "examples", "nonexistent"]);
        assert!(
            result.is_err(),
            "query examples for unknown command should return Err, got {:?}",
            result
        );
    }

    #[test]
    fn test_warn_missing_dry_run_enabled_dispatches_ok() {
        // warn_missing_dry_run should not prevent dispatch — it is advisory only.
        use crate::model::Command;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let called = Arc::new(AtomicBool::new(false));
        let called2 = called.clone();

        let cmd = Command::builder("delete")
            .summary("Delete a resource")
            .mutating()
            .handler(Arc::new(move |_| {
                called2.store(true, Ordering::SeqCst);
                Ok(())
            }))
            .build()
            .unwrap();

        let cli = super::Cli::new(vec![cmd]).warn_missing_dry_run(true);
        // Dispatch should succeed even though there's no --dry-run flag.
        let result = cli.run(["delete"]);
        assert!(result.is_ok(), "dispatch should succeed, got {:?}", result);
        assert!(called.load(Ordering::SeqCst), "handler should have been called");
    }

    #[test]
    fn test_warn_missing_dry_run_with_dry_run_flag_no_warn() {
        // Even with warn enabled, a command that has --dry-run should not warn.
        use crate::model::{Command, Flag};
        use std::sync::Arc;

        let cmd = Command::builder("delete")
            .summary("Delete a resource")
            .mutating()
            .flag(Flag::builder("dry-run").description("Simulate").build().unwrap())
            .handler(Arc::new(|_| Ok(())))
            .build()
            .unwrap();

        let cli = super::Cli::new(vec![cmd]).warn_missing_dry_run(true);
        // This test verifies the code path doesn't crash; we can't easily
        // capture stderr in unit tests, but the absence of a panic is the key check.
        let result = cli.run(["delete"]);
        assert!(result.is_ok(), "dispatch should succeed, got {:?}", result);
    }

    #[test]
    fn test_warn_missing_dry_run_disabled_no_effect() {
        // With warn disabled (default), mutating commands without --dry-run are fine.
        use crate::model::Command;
        use std::sync::Arc;

        let cmd = Command::builder("delete")
            .summary("Delete a resource")
            .mutating()
            .handler(Arc::new(|_| Ok(())))
            .build()
            .unwrap();

        // Default: warn_missing_dry_run is false.
        let cli = super::Cli::new(vec![cmd]);
        let result = cli.run(["delete"]);
        assert!(result.is_ok(), "dispatch should succeed, got {:?}", result);
    }

    // ── Async unit tests ──────────────────────────────────────────────────────

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_run_async_empty_args() {
        let cli = make_cli_no_handler();
        let result = cli.run_async(std::iter::empty::<&str>()).await;
        assert!(
            result.is_ok(),
            "empty args should return Ok, got {:?}",
            result
        );
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_run_async_help_flag() {
        let cli = make_cli_no_handler();
        let result = cli.run_async(["--help"]).await;
        assert!(result.is_ok(), "--help should return Ok, got {:?}", result);
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_run_async_version_flag() {
        let cli = make_cli_no_handler();
        let result = cli.run_async(["--version"]).await;
        assert!(
            result.is_ok(),
            "--version should return Ok, got {:?}",
            result
        );
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_run_async_with_handler() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let called = Arc::new(AtomicBool::new(false));
        let called2 = called.clone();
        let cmd = Command::builder("greet")
            .summary("Say hello")
            .handler(Arc::new(move |_parsed| {
                called2.store(true, Ordering::SeqCst);
                Ok(())
            }))
            .build()
            .unwrap();
        let cli = super::Cli::new(vec![cmd])
            .app_name("testapp")
            .version("1.2.3");
        let result = cli.run_async(["greet"]).await;
        assert!(result.is_ok(), "handler should succeed, got {:?}", result);
        assert!(
            called.load(Ordering::SeqCst),
            "handler should have been called"
        );
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_run_async_unknown_command() {
        let cli = make_cli_no_handler();
        let result = cli.run_async(["unknowncmd"]).await;
        assert!(
            matches!(result, Err(CliError::Parse(_))),
            "unknown command should yield Parse error, got {:?}",
            result
        );
    }

    #[test]
    fn test_version_without_app_name() {
        let cmd = Command::builder("greet").build().unwrap();
        // version set but no app_name — should print just the version
        let cli = super::Cli::new(vec![cmd]).version("2.0.0");
        assert!(cli.run(["--version"]).is_ok());
    }

    #[test]
    fn test_version_not_set() {
        let cmd = Command::builder("greet").build().unwrap();
        // no version at all
        let cli = super::Cli::new(vec![cmd]);
        assert!(cli.run(["--version"]).is_ok());
    }

    #[test]
    fn test_middleware_after_dispatch_called_on_success() {
        use crate::middleware::Middleware;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        struct AfterFlag(Arc<AtomicBool>);
        impl Middleware for AfterFlag {
            fn after_dispatch(
                &self,
                _: &crate::model::ParsedCommand<'_>,
                _: &Result<(), Box<dyn std::error::Error + Send + Sync>>,
            ) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let called = Arc::new(AtomicBool::new(false));
        let cmd = Command::builder("run")
            .handler(Arc::new(|_| Ok(())))
            .build()
            .unwrap();
        let cli = super::Cli::new(vec![cmd]).with_middleware(AfterFlag(called.clone()));
        cli.run(["run"]).unwrap();
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_middleware_after_dispatch_called_on_error() {
        use crate::middleware::Middleware;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        struct AfterFlag(Arc<AtomicBool>);
        impl Middleware for AfterFlag {
            fn after_dispatch(
                &self,
                _: &crate::model::ParsedCommand<'_>,
                _: &Result<(), Box<dyn std::error::Error + Send + Sync>>,
            ) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let called = Arc::new(AtomicBool::new(false));
        let cmd = Command::builder("run")
            .handler(Arc::new(|_| Err("handler error".into())))
            .build()
            .unwrap();
        let cli = super::Cli::new(vec![cmd]).with_middleware(AfterFlag(called.clone()));
        let _ = cli.run(["run"]);
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_middleware_on_parse_error_called() {
        use crate::middleware::Middleware;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        struct OnErrFlag(Arc<AtomicBool>);
        impl Middleware for OnErrFlag {
            fn on_parse_error(&self, _: &crate::parser::ParseError) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let called = Arc::new(AtomicBool::new(false));
        let cmd = Command::builder("run").build().unwrap();
        let cli = super::Cli::new(vec![cmd]).with_middleware(OnErrFlag(called.clone()));
        let _ = cli.run(["unknown_xyz"]);
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_unknown_command_with_suggestions() {
        // "gree" is close to "greet" — should include suggestions in stderr
        let cmd = Command::builder("greet").build().unwrap();
        let cli = super::Cli::new(vec![cmd]);
        let result = cli.run(["gree"]);
        // Should fail with parse error (suggestion logic in the error path)
        assert!(result.is_err());
    }

    #[test]
    fn test_help_for_subcommand() {
        // --help with a known subcommand resolves to that command's help
        let sub = Command::builder("rollback")
            .summary("Roll back")
            .build()
            .unwrap();
        let parent = Command::builder("deploy")
            .summary("Deploy")
            .subcommand(sub)
            .build()
            .unwrap();
        let cli = super::Cli::new(vec![parent]);
        let result = cli.run(["deploy", "rollback", "--help"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_help_with_only_flags() {
        // --help with only flag-like tokens (no command words) renders top-level list
        let cmd = Command::builder("greet").build().unwrap();
        let cli = super::Cli::new(vec![cmd]);
        let result = cli.run(["--flag", "--help"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_help_for_unknown_command() {
        // --help with an unknown command name falls back to top-level list
        let cmd = Command::builder("greet").build().unwrap();
        let cli = super::Cli::new(vec![cmd]);
        let result = cli.run(["unknowncmd", "--help"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_query_with_no_arg_outputs_json() {
        // "query" alone (no subcommand) is same as "query commands"
        use crate::model::Command;
        let cli =
            super::Cli::new(vec![Command::builder("deploy").build().unwrap()]).with_query_support();
        assert!(cli.run(["query"]).is_ok());
    }

    #[test]
    fn test_query_examples_via_resolver() {
        // query examples where exact match fails but resolver finds it by prefix
        use crate::model::{Command, Example};
        let cli = super::Cli::new(vec![Command::builder("deploy")
            .summary("Deploy")
            .example(Example::new("prod", "deploy prod"))
            .build()
            .unwrap()])
        .with_query_support();
        // "dep" prefix-resolves to "deploy"
        let result = cli.run(["query", "examples", "dep"]);
        assert!(
            result.is_ok(),
            "query examples via prefix should succeed, got {:?}",
            result
        );
    }

    #[test]
    fn test_query_named_command_via_resolver() {
        // query <name> where exact match fails but resolver finds by prefix
        use crate::model::Command;
        let cli = super::Cli::new(vec![Command::builder("deploy")
            .summary("Deploy")
            .build()
            .unwrap()])
        .with_query_support();
        // "dep" prefix-resolves to "deploy"
        let result = cli.run(["query", "dep"]);
        assert!(
            result.is_ok(),
            "query prefix-resolved name should succeed, got {:?}",
            result
        );
    }

    #[test]
    fn test_query_examples_no_name_errors() {
        use crate::model::Command;
        let cli =
            super::Cli::new(vec![Command::builder("deploy").build().unwrap()]).with_query_support();
        // "query examples" with no command name should error
        let result = cli.run(["query", "examples"]);
        assert!(result.is_err(), "query examples with no name should error");
    }

    #[test]
    fn test_query_commands_stream_succeeds() {
        use crate::model::Command;
        let cli = super::Cli::new(vec![
            Command::builder("deploy").summary("Deploy").build().unwrap(),
            Command::builder("status").summary("Status").build().unwrap(),
        ])
        .with_query_support();
        let result = cli.run(["query", "commands", "--stream"]);
        assert!(
            result.is_ok(),
            "query commands --stream should return Ok, got {:?}",
            result
        );
    }

    #[test]
    fn test_query_commands_stream_with_fields_succeeds() {
        use crate::model::Command;
        let cli = super::Cli::new(vec![
            Command::builder("deploy").summary("Deploy").build().unwrap(),
        ])
        .with_query_support();
        let result = cli.run(["query", "commands", "--stream", "--fields", "canonical,summary"]);
        assert!(
            result.is_ok(),
            "query commands --stream --fields should return Ok, got {:?}",
            result
        );
    }

    #[test]
    fn test_query_named_command_stream_succeeds() {
        use crate::model::Command;
        let cli = super::Cli::new(vec![Command::builder("deploy")
            .summary("Deploy svc")
            .build()
            .unwrap()])
        .with_query_support();
        let result = cli.run(["query", "deploy", "--stream"]);
        assert!(
            result.is_ok(),
            "query <name> --stream should return Ok, got {:?}",
            result
        );
    }

    #[test]
    fn test_query_named_command_via_resolver_stream_succeeds() {
        use crate::model::Command;
        let cli = super::Cli::new(vec![Command::builder("deploy")
            .summary("Deploy svc")
            .build()
            .unwrap()])
        .with_query_support();
        // "dep" prefix-resolves to "deploy"
        let result = cli.run(["query", "dep", "--stream"]);
        assert!(
            result.is_ok(),
            "query prefix-resolved --stream should return Ok, got {:?}",
            result
        );
    }

    #[test]
    fn test_query_stream_bare_query_succeeds() {
        // "query --stream" alone (no subcommand) is same as "query commands --stream"
        use crate::model::Command;
        let cli =
            super::Cli::new(vec![Command::builder("deploy").build().unwrap()]).with_query_support();
        let result = cli.run(["query", "--stream"]);
        assert!(
            result.is_ok(),
            "query --stream (no subcommand) should return Ok, got {:?}",
            result
        );
    }
}
