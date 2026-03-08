//! Data model for argot commands.
//!
//! Every item in the argot command tree is represented by a [`Command`]. Related
//! types — [`Argument`], [`Flag`], [`Example`] — attach metadata that drives
//! both parsing and documentation generation.
//!
//! ## Builder Pattern
//!
//! All model types are constructed through consuming builders:
//!
//! ```
//! # use argot::model::{Command, Argument, Flag, Example};
//! let cmd = Command::builder("deploy")
//!     .summary("Deploy the application")
//!     .argument(
//!         Argument::builder("env")
//!             .description("Target environment")
//!             .required()
//!             .build()
//!             .unwrap(),
//!     )
//!     .flag(
//!         Flag::builder("dry-run")
//!             .short('n')
//!             .description("Simulate without making changes")
//!             .build()
//!             .unwrap(),
//!     )
//!     .build()
//!     .unwrap();
//!
//! assert_eq!(cmd.canonical, "deploy");
//! ```
//!
//! ## Handler Functions and Parsed Commands
//!
//! A [`HandlerFn`] is an `Arc`-wrapped closure that receives a [`ParsedCommand`]
//! reference and returns `Result<(), Box<dyn Error>>`. The `Arc` wrapper means
//! cloning a [`Command`] only bumps a reference count — no deep copy of the
//! closure occurs.
//!
//! [`ParsedCommand`] is the output of a successful parse: it borrows the matched
//! [`Command`] from the registry and owns the resolved argument and flag maps.

/// Positional argument definition and builder.
pub mod argument;
/// Command definition, builder, handler type, and parsed command output.
pub mod command;
/// Usage example type for commands.
pub mod example;
/// Named flag definition and builder.
pub mod flag;

pub use argument::{Argument, ArgumentBuilder};
pub use command::{Command, CommandBuilder, HandlerFn, ParsedCommand};
pub use example::Example;
pub use flag::{Flag, FlagBuilder};

use thiserror::Error;

/// Error returned by builder `build()` methods.
///
/// Variants are returned from [`CommandBuilder::build`], [`ArgumentBuilder::build`],
/// and [`FlagBuilder::build`] when validation fails. The list of variants includes
/// checks for empty names, duplicate aliases, duplicate flags, duplicate arguments,
/// duplicate subcommands, and variadic argument ordering.
///
/// # Examples
///
/// ```
/// # use argot::model::{Command, BuildError};
/// assert_eq!(Command::builder("").build().unwrap_err(), BuildError::EmptyCanonical);
/// ```
#[derive(Debug, Error, PartialEq)]
pub enum BuildError {
    /// The canonical name (or argument/flag name) was empty or whitespace.
    #[error("canonical name must not be empty")]
    EmptyCanonical,

    /// Two aliases on the same command share the same string.
    #[error("duplicate alias `{0}`")]
    DuplicateAlias(String),

    /// An alias is identical to the command's canonical name.
    #[error("alias `{0}` duplicates the canonical name")]
    AliasEqualsCanonical(String),

    /// Two flags on the same command share the same long name.
    #[error("duplicate flag name `{0}`")]
    DuplicateFlagName(String),

    /// Two flags on the same command share the same short character.
    #[error("duplicate short flag `-{0}`")]
    DuplicateShortFlag(char),

    /// Two positional arguments on the same command share the same name.
    #[error("duplicate argument name `{0}`")]
    DuplicateArgumentName(String),

    /// Two subcommands at the same level share the same canonical name.
    #[error("duplicate subcommand `{0}`")]
    DuplicateSubcommandName(String),

    /// A variadic argument is not the last argument defined.
    #[error("variadic argument `{0}` must be the last argument")]
    VariadicNotLast(String),

    /// A flag's `choices` list is empty, which would reject all values.
    #[error("flag `{0}` has an empty choices list")]
    EmptyChoices(String),

    /// A mutual-exclusivity group contains fewer than two flag names.
    #[error("exclusive group must contain at least two flags")]
    ExclusiveGroupTooSmall,

    /// A flag referenced in a mutual-exclusivity group is not defined on the command.
    #[error("flag `{0}` in exclusive group is not defined on this command")]
    ExclusiveGroupUnknownFlag(String),
}
