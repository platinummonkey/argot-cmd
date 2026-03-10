//! # argot
//!
//! An agent-first command interface framework for Rust.
//!
//! argot makes it easy to define structured CLI commands that are equally
//! useful for human operators and AI agents. Every command carries rich
//! metadata — summaries, descriptions, typed arguments, flags, examples, and
//! best-practice guidance — that can be serialized to JSON or rendered as
//! Markdown for consumption by an LLM tool-call layer.
//!
//! ## Architecture
//!
//! The library is built around five cooperating layers:
//!
//! 1. **[`model`]** — the data model: [`Command`], [`Argument`], [`Flag`],
//!    [`Example`], and their builders.
//! 2. **[`resolver`]** — maps a raw string token to a [`Command`] via exact →
//!    prefix → ambiguous resolution.
//! 3. **[`parser`]** — tokenizes a raw `argv` slice, walks the subcommand tree
//!    using the resolver, then binds flags and positional arguments.
//! 4. **[`query`]** — [`Registry`] is the central command store; it supports
//!    lookup by canonical name, subcommand path, substring search, and fuzzy
//!    search.
//! 5. **[`render`]** — three plain-text / Markdown renderers: [`render_help`],
//!    [`render_subcommand_list`], and [`render_markdown`].
//!
//! A convenience [`cli`] module provides the [`Cli`] struct, which wires all
//! five layers together so you can go from `Vec<Command>` to a fully
//! functional CLI dispatch loop in a few lines.
//!
//! ## Quick Start
//!
//! ```no_run
//! use std::sync::Arc;
//! use argot_cmd::{Command, Flag, Registry, Parser, render_help};
//!
//! // 1. Build commands.
//! let cmd = Command::builder("deploy")
//!     .summary("Deploy the application to an environment")
//!     .flag(
//!         Flag::builder("env")
//!             .description("Target environment")
//!             .takes_value()
//!             .required()
//!             .build()
//!             .unwrap(),
//!     )
//!     .handler(Arc::new(|parsed| {
//!         println!("deploying to {}", parsed.flags["env"]);
//!         Ok(())
//!     }))
//!     .build()
//!     .unwrap();
//!
//! // 2. Store in a registry.
//! let registry = Registry::new(vec![cmd]);
//!
//! // 3. Parse raw arguments.
//! let parser = Parser::new(registry.commands());
//! let parsed = parser.parse(&["deploy", "--env", "production"]).unwrap();
//!
//! // 4. Render help.
//! let help = render_help(parsed.command);
//! println!("{}", help);
//! ```
//!
//! ## Feature Flags
//!
//! | Feature   | Description |
//! |-----------|-------------|
//! | `derive`  | Enables the `#[derive(ArgotCommand)]` proc-macro from `argot-cmd-derive`. |
//! | `fuzzy`   | Enables [`Registry::fuzzy_search`] via the `fuzzy-matcher` crate.    |
//! | `mcp`     | Enables the MCP stdio transport server ([`transport`]).               |
//!
//! ## Modules
//!
//! - [`cli`] — high-level [`Cli`] entry point
//! - [`model`] — data model and builders
//! - [`resolver`] — string-to-command resolution
//! - [`parser`] — `argv` parsing
//! - [`query`] — command registry and search
//! - [`render`] — human-readable and Markdown output

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cli;
pub mod input_validation;
pub mod middleware;
pub mod model;
pub mod parser;
pub mod query;
pub mod render;
pub mod resolver;

pub use cli::{Cli, CliError};
pub use input_validation::{InputValidator, ValidationError};
pub use middleware::Middleware;

pub use model::{
    Argument, ArgumentBuilder, BuildError, Command, CommandBuilder, Example, Flag, FlagBuilder,
    HandlerFn, ParsedCommand,
};

#[cfg(feature = "async")]
pub use model::AsyncHandlerFn;
pub use parser::{ParseError, Parser};
pub use query::{CommandEntry, QueryError, Registry};
pub use render::{
    render_ambiguity, render_completion, render_docs, render_help, render_json_schema,
    render_markdown, render_resolve_error, render_subcommand_list, DefaultRenderer, Renderer,
    Shell,
};
pub use resolver::{ResolveError, Resolver};

/// Trait implemented by types annotated with `#[derive(ArgotCommand)]`.
///
/// Call `T::command()` to obtain a fully-built [`Command`] from the struct's
/// `#[argot(...)]` attributes.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "derive")] {
/// use argot_cmd::ArgotCommand;
///
/// #[derive(argot_cmd::ArgotCommand)]
/// #[argot(summary = "Greet the world")]
/// struct Greet;
///
/// let cmd = Greet::command();
/// assert_eq!(cmd.canonical, "greet");
/// # }
/// ```
pub trait ArgotCommand {
    /// Return the [`Command`] described by this type's `#[argot(...)]`
    /// attributes.
    fn command() -> Command;
}

#[cfg(feature = "derive")]
pub use argot_cmd_derive::ArgotCommand;

/// MCP (Model Context Protocol) stdio transport server.
///
/// Enable with the `mcp` feature flag. Exposes the command registry as MCP
/// tools over a newline-delimited JSON-RPC 2.0 stream.
#[cfg(feature = "mcp")]
pub mod transport;
#[cfg(feature = "mcp")]
pub use transport::McpServer;
