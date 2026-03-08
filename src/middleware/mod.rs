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
    ///
    /// # Examples
    ///
    /// ```
    /// use argot::middleware::Middleware;
    /// use argot::ParsedCommand;
    ///
    /// struct RateLimiter { max: usize }
    ///
    /// impl Middleware for RateLimiter {
    ///     fn before_dispatch(
    ///         &self,
    ///         parsed: &ParsedCommand<'_>,
    ///     ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///         // Allow all commands in this example.
    ///         // A real implementation would check a counter.
    ///         println!("dispatching: {}", parsed.command.canonical);
    ///         Ok(())
    ///     }
    /// }
    /// ```
    fn before_dispatch(
        &self,
        _parsed: &ParsedCommand<'_>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    /// Called after the handler returns (whether `Ok` or `Err`).
    ///
    /// # Examples
    ///
    /// ```
    /// use argot::middleware::Middleware;
    /// use argot::ParsedCommand;
    ///
    /// struct AuditLog;
    ///
    /// impl Middleware for AuditLog {
    ///     fn after_dispatch(
    ///         &self,
    ///         parsed: &ParsedCommand<'_>,
    ///         result: &Result<(), Box<dyn std::error::Error + Send + Sync>>,
    ///     ) {
    ///         match result {
    ///             Ok(()) => println!("✓ {}", parsed.command.canonical),
    ///             Err(e) => eprintln!("✗ {}: {}", parsed.command.canonical, e),
    ///         }
    ///     }
    /// }
    /// ```
    fn after_dispatch(
        &self,
        _parsed: &ParsedCommand<'_>,
        _result: &Result<(), Box<dyn std::error::Error + Send + Sync>>,
    ) {
    }

    /// Called when `Parser::parse` returns an error, before it is surfaced to the caller.
    ///
    /// # Examples
    ///
    /// ```
    /// use argot::middleware::Middleware;
    /// use argot::parser::ParseError;
    ///
    /// struct ErrorLogger;
    ///
    /// impl Middleware for ErrorLogger {
    ///     fn on_parse_error(&self, err: &ParseError) {
    ///         eprintln!("parse failed: {}", err);
    ///     }
    /// }
    /// ```
    fn on_parse_error(&self, _error: &ParseError) {}
}
