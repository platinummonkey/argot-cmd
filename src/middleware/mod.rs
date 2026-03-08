//! Middleware hooks for the [`crate::cli::Cli`] dispatch loop.
//!
//! Implement [`Middleware`] and register it with [`crate::Cli::with_middleware`]
//! to intercept the parse-dispatch lifecycle.

use crate::model::ParsedCommand;
use crate::parser::ParseError;

/// Hook into the [`crate::cli::Cli`] parse-and-dispatch lifecycle.
///
/// All methods have default no-op implementations so you only need to
/// override the hooks you care about.
///
/// # Examples
///
/// ```
/// use argot::middleware::Middleware;
/// use argot::ParsedCommand;
///
/// struct Logger;
///
/// impl Middleware for Logger {
///     fn before_dispatch(&self, parsed: &ParsedCommand<'_>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
///         eprintln!("[log] dispatching: {}", parsed.command.canonical);
///         Ok(())
///     }
/// }
/// ```
pub trait Middleware: Send + Sync {
    /// Called after a successful parse, before the handler is invoked.
    ///
    /// Return `Err(...)` to abort dispatch with a [`crate::cli::CliError::Handler`].
    fn before_dispatch(
        &self,
        _parsed: &ParsedCommand<'_>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    /// Called after the handler returns (whether `Ok` or `Err`).
    fn after_dispatch(
        &self,
        _parsed: &ParsedCommand<'_>,
        _result: &Result<(), Box<dyn std::error::Error + Send + Sync>>,
    ) {
    }

    /// Called when `Parser::parse` returns an error, before it is surfaced to the caller.
    fn on_parse_error(&self, _error: &ParseError) {}
}
