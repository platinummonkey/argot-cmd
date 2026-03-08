//! Tokenization and argument parsing for raw `argv` slices.
//!
//! The parser operates in three stages:
//!
//! 1. **Tokenize** — the raw `&[&str]` argv is converted to a typed
//!    token stream by the internal `tokenizer` module.
//!    Each token is one of: a plain word, a long flag (`--name` / `--name=val`),
//!    a short flag (`-f` / `-fval`), or the `--` separator.
//!
//! 2. **Subcommand tree walk** — the first word token is resolved to a
//!    top-level [`Command`] using the [`Resolver`]. While the
//!    resolved command has subcommands and the next token is a word that
//!    resolves to one of them, the parser descends into the subcommand tree.
//!    A word that fails to resolve is treated as the start of positional
//!    arguments rather than an error.
//!
//! 3. **Flag and argument binding** — remaining tokens are bound using a
//!    queue-based loop: long and short flag tokens are matched against the
//!    resolved command's flag definitions; plain word tokens are accumulated
//!    as positional arguments. Adjacent short flags (`-abc`) are expanded
//!    inline: each boolean flag in the run registers `"true"` and the
//!    remaining characters are re-queued as a new `ShortFlag` token.
//!    Boolean flags also support `--no-{name}` negation syntax, which sets
//!    the value to `"false"`. After all tokens are consumed, positional
//!    arguments are bound by declaration order (variadic last arguments
//!    collect all remaining positionals into a JSON array); required flags
//!    and arguments are validated; and defaults and environment-variable fallbacks are applied.
//!
//! # Example
//!
//! ```
//! # use argot::{Command, Argument, Flag, Parser};
//! let cmd = Command::builder("list")
//!     .argument(Argument::builder("filter").build().unwrap())
//!     .flag(Flag::builder("verbose").short('v').build().unwrap())
//!     .build()
//!     .unwrap();
//!
//! let cmds = vec![cmd];
//! let parser = Parser::new(&cmds);
//!
//! let parsed = parser.parse(&["list", "foo", "-v"]).unwrap();
//! assert_eq!(parsed.command.canonical, "list");
//! assert_eq!(parsed.args["filter"], "foo");
//! assert_eq!(parsed.flags["verbose"], "true");
//! ```

mod tokenizer;

use std::collections::{HashMap, VecDeque};

use thiserror::Error;

use crate::model::{Command, ParsedCommand};
use crate::resolver::{ResolveError, Resolver};

use tokenizer::{tokenize, Token};

/// Errors produced by [`Parser::parse`].
#[derive(Debug, Error, PartialEq)]
pub enum ParseError {
    /// The argv slice was empty — no command name was provided.
    #[error("no command provided")]
    NoCommand,
    /// The command (or subcommand) token could not be resolved.
    ///
    /// Wraps a [`ResolveError`] transparently so callers can match on
    /// [`ResolveError::Unknown`] and [`ResolveError::Ambiguous`] directly.
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    /// A required positional argument was not supplied.
    ///
    /// The inner `String` is the argument's canonical name.
    #[error("missing required argument: {0}")]
    MissingArgument(String),
    /// More positional arguments were supplied than the command declares.
    ///
    /// The inner `String` is the first unexpected token.
    #[error("unexpected argument: {0}")]
    UnexpectedArgument(String),
    /// A required flag was not supplied, and no environment-variable fallback
    /// (registered with [`crate::FlagBuilder::env`]) provided a value.
    ///
    /// The inner `String` is the flag's long name (without `--`).
    #[error("missing required flag: --{0}")]
    MissingFlag(String),
    /// A value-taking flag was provided without a following value.
    #[error("flag --{name} requires a value")]
    FlagMissingValue {
        /// The long name of the flag that was missing its value.
        name: String,
    },
    /// A flag token (`--name` or `-c`) was not recognized by the resolved
    /// command.
    ///
    /// The inner `String` includes the leading dashes, e.g. `"--foo"` or
    /// `"-x"`. This variant is also raised when `--no-{name}` negation syntax
    /// is used with an unknown flag name or with a value-taking flag (which
    /// cannot be negated).
    #[error("unknown flag: {0}")]
    UnknownFlag(String),
    /// A word token following a subcommand-only parent did not match any
    /// declared subcommand.
    ///
    /// Only raised when the parent command has no positional arguments defined;
    /// otherwise the word is treated as a positional value.
    #[error("unknown subcommand `{got}` for `{parent}`")]
    UnknownSubcommand {
        /// The canonical name of the parent command.
        parent: String,
        /// The unrecognised token as supplied by the caller.
        got: String,
    },
    /// A flag value was provided that is not in the flag's allowed choices.
    #[error("invalid value `{value}` for `--{flag}`: expected one of {choices:?}")]
    InvalidChoice {
        /// The flag's long name.
        flag: String,
        /// The invalid value that was supplied.
        value: String,
        /// The allowed values.
        choices: Vec<String>,
    },
    /// Two or more mutually exclusive flags were provided in the same invocation.
    ///
    /// The `flags` field lists the conflicting flags (with `--` prefix).
    #[error("flags {flags:?} are mutually exclusive — provide at most one")]
    MutuallyExclusive {
        /// The conflicting flag names that were all set (with `--` prefix).
        flags: Vec<String>,
    },
}

/// Parses raw argument slices against a slice of registered [`Command`]s.
///
/// Create a `Parser` with [`Parser::new`], then call [`Parser::parse`] for
/// each invocation. The parser borrows the command slice for lifetime `'a`;
/// the returned [`ParsedCommand`] also carries that lifetime.
///
/// # Examples
///
/// ```
/// # use argot::{Command, Parser};
/// let cmds = vec![Command::builder("status").build().unwrap()];
/// let parser = Parser::new(&cmds);
/// let parsed = parser.parse(&["status"]).unwrap();
/// assert_eq!(parsed.command.canonical, "status");
/// ```
pub struct Parser<'a> {
    commands: &'a [Command],
}

impl<'a> Parser<'a> {
    /// Create a new `Parser` over the given command slice.
    ///
    /// # Arguments
    ///
    /// - `commands` — Top-level commands to parse against. The lifetime `'a`
    ///   is propagated to the [`ParsedCommand`] returned by [`Parser::parse`].
    pub fn new(commands: &'a [Command]) -> Self {
        Self { commands }
    }

    /// Parse `argv` (the full argument list including the command name) into a
    /// [`ParsedCommand`] that borrows from the registered command tree.
    ///
    /// The first element of `argv` must be a word token naming the top-level
    /// command. Subsequent tokens are processed as described in the
    /// [module documentation][self].
    ///
    /// # Arguments
    ///
    /// - `argv` — The argument slice to parse. Should **not** include the
    ///   program name (`argv[0]` in `std::env::args`); the first element must
    ///   be the command name.
    ///
    /// # Errors
    ///
    /// - [`ParseError::NoCommand`] — `argv` is empty.
    /// - [`ParseError::Resolve`] — the command or subcommand token could not
    ///   be resolved (wraps [`ResolveError::Unknown`] or
    ///   [`ResolveError::Ambiguous`]).
    /// - [`ParseError::MissingArgument`] — a required positional argument was
    ///   absent.
    /// - [`ParseError::UnexpectedArgument`] — more positional arguments were
    ///   provided than the command declares.
    /// - [`ParseError::MissingFlag`] — a required flag was absent.
    /// - [`ParseError::FlagMissingValue`] — a value-taking flag had no
    ///   following value.
    /// - [`ParseError::UnknownFlag`] — an unrecognized flag was encountered.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot::{Command, Flag, Parser};
    /// let cmd = Command::builder("build")
    ///     .flag(
    ///         Flag::builder("target")
    ///             .takes_value()
    ///             .default_value("debug")
    ///             .build()
    ///             .unwrap(),
    ///     )
    ///     .build()
    ///     .unwrap();
    ///
    /// let cmds = vec![cmd];
    /// let parser = Parser::new(&cmds);
    ///
    /// let parsed = parser.parse(&["build", "--target=release"]).unwrap();
    /// assert_eq!(parsed.flags["target"], "release");
    ///
    /// // Default is applied when flag is absent
    /// let parsed2 = parser.parse(&["build"]).unwrap();
    /// assert_eq!(parsed2.flags["target"], "debug");
    /// ```
    pub fn parse(&self, argv: &[&str]) -> Result<ParsedCommand<'a>, ParseError> {
        let tokens = tokenize(argv);
        let mut pos = 0;

        // First token must be a Word naming the top-level command.
        let cmd_name = match tokens.get(pos) {
            Some(Token::Word(w)) => {
                pos += 1;
                w.clone()
            }
            _ => return Err(ParseError::NoCommand),
        };

        let resolver = Resolver::new(self.commands);
        let mut cmd: &'a Command = resolver.resolve(&cmd_name)?;

        // Walk the subcommand tree while the next token is a Word that resolves.
        loop {
            if cmd.subcommands.is_empty() {
                break;
            }
            match tokens.get(pos) {
                Some(Token::Word(w)) => {
                    let sub_resolver = Resolver::new(&cmd.subcommands);
                    match sub_resolver.resolve(w) {
                        Ok(sub) => {
                            cmd = sub;
                            pos += 1;
                        }
                        Err(e) => match e {
                            // Ambiguous is always a user error — propagate it.
                            ResolveError::Ambiguous { .. } => return Err(ParseError::Resolve(e)),
                            // Unknown: propagate only when the parent has no
                            // positional arguments (nowhere legitimate for the
                            // word to land). Otherwise break and treat as positional.
                            ResolveError::Unknown { .. } => {
                                if cmd.arguments.is_empty() {
                                    return Err(ParseError::UnknownSubcommand {
                                        parent: cmd.canonical.clone(),
                                        got: w.clone(),
                                    });
                                }
                                break;
                            }
                        },
                    }
                }
                _ => break,
            }
        }

        // Process remaining tokens: flags and positional arguments.
        // Uses a queue so that adjacent short flags (-abc) can push synthetic
        // ShortFlag tokens back to the front for processing.
        let mut positionals: Vec<String> = Vec::new();
        let mut flags: HashMap<String, String> = HashMap::new();

        let mut queue: VecDeque<Token> = tokens[pos..].iter().cloned().collect();

        while let Some(token) = queue.pop_front() {
            match token {
                Token::Separator => {
                    // Everything after -- is a positional word (tokenizer already
                    // converts post-separator args to Token::Word, so this is a
                    // no-op guard for the separator token itself).
                }
                Token::Word(w) => {
                    positionals.push(w);
                }
                Token::LongFlag { name, value } => {
                    // Check --no-{name} negation for boolean flags.
                    if let Some(base) = name.strip_prefix("no-") {
                        if let Some(flag_def) =
                            cmd.flags.iter().find(|f| f.name == base && !f.takes_value)
                        {
                            if value.is_some() {
                                return Err(ParseError::UnknownFlag(format!("--{}", name)));
                            }
                            flags.insert(flag_def.name.clone(), "false".to_string());
                            continue;
                        }
                    }

                    let flag_def = cmd
                        .flags
                        .iter()
                        .find(|f| f.name == name)
                        .ok_or_else(|| ParseError::UnknownFlag(format!("--{}", name)))?;

                    let val = if flag_def.takes_value {
                        if let Some(v) = value {
                            v
                        } else {
                            match queue.pop_front() {
                                Some(Token::Word(w)) => w,
                                _ => {
                                    return Err(ParseError::FlagMissingValue {
                                        name: flag_def.name.clone(),
                                    })
                                }
                            }
                        }
                    } else {
                        "true".to_string()
                    };

                    // Validate choice constraint
                    if let Some(choices) = &flag_def.choices {
                        if !choices.contains(&val) {
                            return Err(ParseError::InvalidChoice {
                                flag: flag_def.name.clone(),
                                value: val,
                                choices: choices.clone(),
                            });
                        }
                    }

                    // Repeatable flag accumulation
                    if flag_def.repeatable {
                        if flag_def.takes_value {
                            // Accumulate into JSON array
                            let new_val = match flags.get(&flag_def.name) {
                                None => serde_json::to_string(&[&val]).expect("serde_json serialization of &[&str] is infallible for simple string types"),
                                Some(existing) => {
                                    let mut arr: Vec<String> = serde_json::from_str(existing)
                                        .unwrap_or_else(|_| vec![existing.clone()]);
                                    arr.push(val);
                                    serde_json::to_string(&arr).expect("serde_json serialization of Vec<String> is infallible")
                                }
                            };
                            flags.insert(flag_def.name.clone(), new_val);
                        } else {
                            // Count occurrences
                            let count = flags
                                .get(&flag_def.name)
                                .and_then(|v| v.parse::<u64>().ok())
                                .unwrap_or(0);
                            flags.insert(flag_def.name.clone(), (count + 1).to_string());
                        }
                    } else {
                        flags.insert(flag_def.name.clone(), val);
                    }
                }
                Token::ShortFlag { name: c, value } => {
                    let flag_def = cmd
                        .flags
                        .iter()
                        .find(|f| f.short == Some(c))
                        .ok_or_else(|| ParseError::UnknownFlag(format!("-{}", c)))?;

                    if flag_def.takes_value {
                        let val = if let Some(v) = value {
                            v
                        } else {
                            match queue.pop_front() {
                                Some(Token::Word(w)) => w,
                                _ => {
                                    return Err(ParseError::FlagMissingValue {
                                        name: flag_def.name.clone(),
                                    })
                                }
                            }
                        };

                        // Validate choice constraint
                        if let Some(choices) = &flag_def.choices {
                            if !choices.contains(&val) {
                                return Err(ParseError::InvalidChoice {
                                    flag: flag_def.name.clone(),
                                    value: val,
                                    choices: choices.clone(),
                                });
                            }
                        }

                        // Repeatable flag accumulation
                        if flag_def.repeatable {
                            let new_val = match flags.get(&flag_def.name) {
                                None => serde_json::to_string(&[&val]).expect("serde_json serialization of &[&str] is infallible for simple string types"),
                                Some(existing) => {
                                    let mut arr: Vec<String> = serde_json::from_str(existing)
                                        .unwrap_or_else(|_| vec![existing.clone()]);
                                    arr.push(val);
                                    serde_json::to_string(&arr).expect("serde_json serialization of Vec<String> is infallible")
                                }
                            };
                            flags.insert(flag_def.name.clone(), new_val);
                        } else {
                            flags.insert(flag_def.name.clone(), val);
                        }
                    } else {
                        // Boolean flag: register as true (or count if repeatable) and expand remaining chars.
                        if flag_def.repeatable {
                            let count = flags
                                .get(&flag_def.name)
                                .and_then(|v| v.parse::<u64>().ok())
                                .unwrap_or(0);
                            flags.insert(flag_def.name.clone(), (count + 1).to_string());
                        } else {
                            flags.insert(flag_def.name.clone(), "true".to_string());
                        }
                        if let Some(rest) = value {
                            if !rest.is_empty() {
                                let mut chars = rest.chars();
                                let next_c =
                                    chars.next().expect("guarded by is_empty() check above");
                                let remainder: String = chars.collect();
                                queue.push_front(Token::ShortFlag {
                                    name: next_c,
                                    value: if remainder.is_empty() {
                                        None
                                    } else {
                                        Some(remainder)
                                    },
                                });
                            }
                        }
                    }
                }
            }
        }

        // Bind positional arguments to declared argument slots.
        let mut args: HashMap<String, String> = HashMap::new();
        for (i, arg_def) in cmd.arguments.iter().enumerate() {
            if arg_def.variadic {
                // Collect all remaining positionals into a JSON array string.
                let values: Vec<&String> = positionals[i..].iter().collect();
                if values.is_empty() && arg_def.required {
                    return Err(ParseError::MissingArgument(arg_def.name.clone()));
                } else if !values.is_empty() {
                    let json_val = serde_json::to_string(
                        &values.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                    )
                    .expect(
                        "serde_json serialization of &[&str] is infallible for simple string types",
                    );
                    args.insert(arg_def.name.clone(), json_val);
                } else if let Some(default) = &arg_def.default {
                    args.insert(arg_def.name.clone(), default.clone());
                }
                break; // variadic is always last
            }
            if let Some(val) = positionals.get(i) {
                args.insert(arg_def.name.clone(), val.clone());
            } else if arg_def.required {
                return Err(ParseError::MissingArgument(arg_def.name.clone()));
            } else if let Some(default) = &arg_def.default {
                args.insert(arg_def.name.clone(), default.clone());
            }
        }

        // Only error on unexpected positionals if the last argument is NOT variadic.
        let last_is_variadic = cmd.arguments.last().map(|a| a.variadic).unwrap_or(false);
        if positionals.len() > cmd.arguments.len() && !last_is_variadic {
            return Err(ParseError::UnexpectedArgument(
                positionals[cmd.arguments.len()].clone(),
            ));
        }

        // Env var fallback: for each flag not yet set by argv, check its env var.
        for flag_def in &cmd.flags {
            if !flags.contains_key(&flag_def.name) {
                if let Some(ref var_name) = flag_def.env {
                    if let Ok(val) = std::env::var(var_name) {
                        if !val.is_empty() {
                            // Validate against choices if present
                            if let Some(ref choices) = flag_def.choices {
                                if !choices.contains(&val) {
                                    return Err(ParseError::InvalidChoice {
                                        flag: flag_def.name.clone(),
                                        value: val,
                                        choices: choices.clone(),
                                    });
                                }
                            }
                            flags.insert(flag_def.name.clone(), val);
                        }
                    }
                }
            }
        }

        // Enforce mutual exclusivity groups.
        for group in &cmd.exclusive_groups {
            let set: Vec<String> = group
                .iter()
                .filter(|name| flags.contains_key(*name))
                .map(|name| format!("--{}", name))
                .collect();
            if set.len() > 1 {
                return Err(ParseError::MutuallyExclusive { flags: set });
            }
        }

        // Validate required flags; apply defaults.
        for flag_def in &cmd.flags {
            if flag_def.required && !flags.contains_key(&flag_def.name) {
                return Err(ParseError::MissingFlag(flag_def.name.clone()));
            }
            if !flags.contains_key(&flag_def.name) {
                if let Some(default) = &flag_def.default {
                    flags.insert(flag_def.name.clone(), default.clone());
                }
            }
        }

        Ok(ParsedCommand {
            command: cmd,
            args,
            flags,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Argument, Command, Example, Flag};

    fn build_commands() -> Vec<Command> {
        let remote_add = Command::builder("add")
            .summary("Add a remote")
            .argument(
                Argument::builder("name")
                    .description("remote name")
                    .required()
                    .build()
                    .unwrap(),
            )
            .argument(
                Argument::builder("url")
                    .description("remote url")
                    .required()
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();

        let remote_remove = Command::builder("remove")
            .alias("rm")
            .summary("Remove a remote")
            .argument(
                Argument::builder("name")
                    .description("remote name")
                    .required()
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();

        let remote = Command::builder("remote")
            .summary("Manage remotes")
            .subcommand(remote_add)
            .subcommand(remote_remove)
            .build()
            .unwrap();

        let list = Command::builder("list")
            .alias("ls")
            .summary("List items")
            .argument(
                Argument::builder("filter")
                    .description("optional filter")
                    .build()
                    .unwrap(),
            )
            .flag(
                Flag::builder("verbose")
                    .short('v')
                    .description("verbose output")
                    .build()
                    .unwrap(),
            )
            .flag(
                Flag::builder("output")
                    .short('o')
                    .description("output format")
                    .takes_value()
                    .default_value("text")
                    .build()
                    .unwrap(),
            )
            .example(Example::new("list all", "myapp list"))
            .build()
            .unwrap();

        let deploy = Command::builder("deploy")
            .summary("Deploy")
            .flag(
                Flag::builder("env")
                    .description("target environment")
                    .takes_value()
                    .required()
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();

        vec![list, remote, deploy]
    }

    struct TestCase {
        name: &'static str,
        argv: &'static [&'static str],
        expect_err: bool,
        expected_canonical: Option<&'static str>,
        expected_args: Vec<(&'static str, &'static str)>,
        expected_flags: Vec<(&'static str, &'static str)>,
    }

    #[test]
    fn test_parse() {
        let commands = build_commands();
        let parser = Parser::new(&commands);

        let cases = vec![
            TestCase {
                name: "flat command no args",
                argv: &["list"],
                expect_err: false,
                expected_canonical: Some("list"),
                expected_args: vec![],
                expected_flags: vec![("output", "text")],
            },
            TestCase {
                name: "flat command with positional",
                argv: &["list", "foo"],
                expect_err: false,
                expected_canonical: Some("list"),
                expected_args: vec![("filter", "foo")],
                expected_flags: vec![("output", "text")],
            },
            TestCase {
                name: "alias resolved",
                argv: &["ls"],
                expect_err: false,
                expected_canonical: Some("list"),
                expected_args: vec![],
                expected_flags: vec![("output", "text")],
            },
            TestCase {
                name: "boolean flag short",
                argv: &["list", "-v"],
                expect_err: false,
                expected_canonical: Some("list"),
                expected_args: vec![],
                expected_flags: vec![("verbose", "true"), ("output", "text")],
            },
            TestCase {
                name: "long flag equals",
                argv: &["list", "--output=json"],
                expect_err: false,
                expected_canonical: Some("list"),
                expected_args: vec![],
                expected_flags: vec![("output", "json")],
            },
            TestCase {
                name: "long flag space value",
                argv: &["list", "--output", "json"],
                expect_err: false,
                expected_canonical: Some("list"),
                expected_args: vec![],
                expected_flags: vec![("output", "json")],
            },
            TestCase {
                name: "short flag space value",
                argv: &["list", "-o", "json"],
                expect_err: false,
                expected_canonical: Some("list"),
                expected_args: vec![],
                expected_flags: vec![("output", "json")],
            },
            TestCase {
                name: "two-level subcommand",
                argv: &["remote", "add", "origin", "https://example.com"],
                expect_err: false,
                expected_canonical: Some("add"),
                expected_args: vec![("name", "origin"), ("url", "https://example.com")],
                expected_flags: vec![],
            },
            TestCase {
                name: "subcommand alias",
                argv: &["remote", "rm", "origin"],
                expect_err: false,
                expected_canonical: Some("remove"),
                expected_args: vec![("name", "origin")],
                expected_flags: vec![],
            },
            TestCase {
                name: "no command",
                argv: &[],
                expect_err: true,
                expected_canonical: None,
                expected_args: vec![],
                expected_flags: vec![],
            },
            TestCase {
                name: "unknown command",
                argv: &["unknown"],
                expect_err: true,
                expected_canonical: None,
                expected_args: vec![],
                expected_flags: vec![],
            },
            TestCase {
                name: "unknown flag",
                argv: &["list", "--nope"],
                expect_err: true,
                expected_canonical: None,
                expected_args: vec![],
                expected_flags: vec![],
            },
            TestCase {
                name: "missing required flag",
                argv: &["deploy"],
                expect_err: true,
                expected_canonical: None,
                expected_args: vec![],
                expected_flags: vec![],
            },
            TestCase {
                name: "unexpected positional",
                argv: &["list", "one", "two"],
                expect_err: true,
                expected_canonical: None,
                expected_args: vec![],
                expected_flags: vec![],
            },
        ];

        for tc in &cases {
            let result = parser.parse(tc.argv);
            if tc.expect_err {
                assert!(result.is_err(), "case '{}': expected error", tc.name);
            } else {
                let parsed = result
                    .unwrap_or_else(|e| panic!("case '{}': unexpected error: {}", tc.name, e));
                assert_eq!(
                    parsed.command.canonical,
                    tc.expected_canonical.unwrap(),
                    "case '{}'",
                    tc.name
                );
                for (k, v) in &tc.expected_args {
                    assert_eq!(
                        parsed.args.get(*k).map(String::as_str),
                        Some(*v),
                        "case '{}': arg {}",
                        tc.name,
                        k
                    );
                }
                for (k, v) in &tc.expected_flags {
                    assert_eq!(
                        parsed.flags.get(*k).map(String::as_str),
                        Some(*v),
                        "case '{}': flag {}",
                        tc.name,
                        k
                    );
                }
            }
        }
    }

    #[test]
    fn test_double_dash_separator() {
        let cmds = vec![Command::builder("run")
            .argument(
                Argument::builder("script")
                    .description("script to run")
                    .required()
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);
        // "--" separator should make "--not-a-flag" treated as a positional word.
        // But our command only has one argument, so the second word would be unexpected.
        // Let's just verify `--` itself doesn't cause a parse error on the separator.
        let result = parser.parse(&["run", "--", "myscript"]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().args["script"], "myscript");
    }

    #[test]
    fn test_missing_required_argument() {
        let cmds = vec![Command::builder("get")
            .argument(
                Argument::builder("id")
                    .description("item id")
                    .required()
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);
        assert!(matches!(
            parser.parse(&["get"]),
            Err(ParseError::MissingArgument(ref s)) if s == "id"
        ));
    }

    #[test]
    fn test_flag_missing_value() {
        let cmds = vec![Command::builder("build")
            .flag(Flag::builder("target").takes_value().build().unwrap())
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);
        assert!(matches!(
            parser.parse(&["build", "--target"]),
            Err(ParseError::FlagMissingValue { .. })
        ));
    }

    #[test]
    fn test_ambiguous_subcommand() {
        let fetch = Command::builder("fetch").build().unwrap();
        let force_push = Command::builder("force-push").build().unwrap();
        let cmds = vec![Command::builder("git")
            .subcommand(fetch)
            .subcommand(force_push)
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);
        let result = parser.parse(&["git", "f"]);
        assert!(
            matches!(
                result,
                Err(ParseError::Resolve(ResolveError::Ambiguous { .. }))
            ),
            "expected Resolve(Ambiguous), got {:?}",
            result
        );
    }

    #[test]
    fn test_unknown_subcommand_on_no_positionals() {
        let cmds = vec![Command::builder("remote")
            .subcommand(Command::builder("add").build().unwrap())
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);
        assert!(matches!(
            parser.parse(&["remote", "xyz"]),
            Err(ParseError::UnknownSubcommand { .. })
        ));
    }

    #[test]
    fn test_unknown_word_treated_as_positional_when_parent_has_args() {
        let cmds = vec![Command::builder("deploy")
            .subcommand(Command::builder("production").build().unwrap())
            .argument(
                Argument::builder("target")
                    .description("deployment target")
                    .required()
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);
        let result = parser.parse(&["deploy", "staging"]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let parsed = result.unwrap();
        assert_eq!(parsed.command.canonical, "deploy");
        assert_eq!(
            parsed.args.get("target").map(String::as_str),
            Some("staging")
        );
    }

    // -----------------------------------------------------------------------
    // Enhancement 1: Adjacent short flags (-abc → -a -b -c)
    // -----------------------------------------------------------------------

    fn build_multi_flag_command() -> Vec<Command> {
        vec![Command::builder("cmd")
            .flag(Flag::builder("verbose").short('v').build().unwrap())
            .flag(Flag::builder("no-wait").short('n').build().unwrap())
            .flag(
                Flag::builder("output")
                    .short('o')
                    .takes_value()
                    .default_value("text")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap()]
    }

    #[test]
    fn test_adjacent_short_flags() {
        let cmds = build_multi_flag_command();
        let parser = Parser::new(&cmds);

        // -vo json: -v is boolean (→ verbose=true), -o takes a value (→ output=json)
        let result = parser.parse(&["cmd", "-vo", "json"]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let parsed = result.unwrap();
        assert_eq!(
            parsed.flags.get("verbose").map(String::as_str),
            Some("true")
        );
        assert_eq!(parsed.flags.get("output").map(String::as_str), Some("json"));

        // -vn: both boolean flags → verbose=true, no-wait=true
        let result2 = parser.parse(&["cmd", "-vn"]);
        assert!(result2.is_ok(), "expected Ok, got {:?}", result2);
        let parsed2 = result2.unwrap();
        assert_eq!(
            parsed2.flags.get("verbose").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            parsed2.flags.get("no-wait").map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn test_adjacent_short_flags_with_value() {
        let cmds = build_multi_flag_command();
        let parser = Parser::new(&cmds);

        // -ofile.txt: -o takes_value, so "file.txt" is the inline value
        let result = parser.parse(&["cmd", "-ofile.txt"]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let parsed = result.unwrap();
        assert_eq!(
            parsed.flags.get("output").map(String::as_str),
            Some("file.txt")
        );
    }

    // -----------------------------------------------------------------------
    // Enhancement 2: --no-flag negation
    // -----------------------------------------------------------------------

    #[test]
    fn test_flag_negation() {
        let cmds = vec![Command::builder("cmd")
            .flag(Flag::builder("verbose").short('v').build().unwrap())
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);

        let result = parser.parse(&["cmd", "--no-verbose"]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let parsed = result.unwrap();
        assert_eq!(
            parsed.flags.get("verbose").map(String::as_str),
            Some("false")
        );
    }

    #[test]
    fn test_flag_negation_unknown() {
        let cmds = vec![Command::builder("cmd")
            .flag(Flag::builder("verbose").short('v').build().unwrap())
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);

        // --no-nonexistent should return UnknownFlag
        let result = parser.parse(&["cmd", "--no-nonexistent"]);
        assert!(
            matches!(result, Err(ParseError::UnknownFlag(_))),
            "expected UnknownFlag, got {:?}",
            result
        );
    }

    // -----------------------------------------------------------------------
    // Enhancement 3: Variadic arguments
    // -----------------------------------------------------------------------

    #[test]
    fn test_variadic_argument() {
        let cmds = vec![Command::builder("cmd")
            .argument(Argument::builder("files").variadic().build().unwrap())
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);

        let result = parser.parse(&["cmd", "a", "b", "c"]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let parsed = result.unwrap();
        let raw = parsed.args.get("files").expect("files key missing");
        let values: Vec<String> = serde_json::from_str(raw).expect("not valid JSON array");
        assert_eq!(values, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_variadic_argument_required_empty() {
        let cmds = vec![Command::builder("cmd")
            .argument(
                Argument::builder("files")
                    .required()
                    .variadic()
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);

        let result = parser.parse(&["cmd"]);
        assert!(
            matches!(result, Err(ParseError::MissingArgument(ref s)) if s == "files"),
            "expected MissingArgument(files), got {:?}",
            result
        );
    }

    #[test]
    fn test_variadic_argument_default() {
        let cmds = vec![Command::builder("cmd")
            .argument(
                Argument::builder("files")
                    .default_value("[]")
                    .variadic()
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);

        // No positionals provided → default applies
        let result = parser.parse(&["cmd"]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let parsed = result.unwrap();
        assert_eq!(parsed.args.get("files").map(String::as_str), Some("[]"));
    }

    // -----------------------------------------------------------------------
    // Flag choices and repeatable
    // -----------------------------------------------------------------------

    #[test]
    fn test_flag_choices_valid() {
        let cmds = vec![Command::builder("build")
            .flag(
                Flag::builder("format")
                    .takes_value()
                    .choices(["json", "yaml", "text"])
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);
        let parsed = parser.parse(&["build", "--format=json"]).unwrap();
        assert_eq!(parsed.flags["format"], "json");
    }

    #[test]
    fn test_flag_choices_invalid() {
        let cmds = vec![Command::builder("build")
            .flag(
                Flag::builder("format")
                    .takes_value()
                    .choices(["json", "yaml", "text"])
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);
        let result = parser.parse(&["build", "--format=xml"]);
        assert!(
            matches!(result, Err(ParseError::InvalidChoice { ref value, .. }) if value == "xml")
        );
    }

    #[test]
    fn test_repeatable_boolean_flag() {
        let cmds = vec![Command::builder("run")
            .flag(
                Flag::builder("verbose")
                    .short('v')
                    .repeatable()
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);
        // Three separate -v flags
        let parsed = parser.parse(&["run", "-v", "-v", "-v"]).unwrap();
        assert_eq!(parsed.flags["verbose"], "3");
    }

    #[test]
    fn test_repeatable_value_flag() {
        let cmds = vec![Command::builder("run")
            .flag(
                Flag::builder("tag")
                    .takes_value()
                    .repeatable()
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);
        let parsed = parser.parse(&["run", "--tag=alpha", "--tag=beta"]).unwrap();
        let tags: Vec<String> = serde_json::from_str(&parsed.flags["tag"]).unwrap();
        assert_eq!(tags, vec!["alpha", "beta"]);
    }

    #[test]
    fn test_adjacent_short_repeatable() {
        // -vvv should expand to three -v flags and count correctly
        let cmds = vec![Command::builder("run")
            .flag(
                Flag::builder("verbose")
                    .short('v')
                    .repeatable()
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);
        let parsed = parser.parse(&["run", "-vvv"]).unwrap();
        assert_eq!(parsed.flags["verbose"], "3");
    }

    #[test]
    fn test_empty_choices_build_error() {
        use crate::model::BuildError;
        let flag = Flag::builder("format")
            .takes_value()
            .choices(Vec::<String>::new())
            .build()
            .unwrap();
        let result = Command::builder("cmd").flag(flag).build();
        assert!(matches!(result, Err(BuildError::EmptyChoices(_))));
    }

    #[test]
    fn test_env_var_fallback_basic() {
        let var = "ARGOT_TEST_ENVFLAG_BASIC_11111";
        std::env::remove_var(var);
        let cmds = vec![Command::builder("cmd")
            .flag(
                Flag::builder("token")
                    .takes_value()
                    .env(var)
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);

        // Not set → absent
        assert!(!parser.parse(&["cmd"]).unwrap().flags.contains_key("token"));

        // Set → present
        std::env::set_var(var, "abc123");
        assert_eq!(parser.parse(&["cmd"]).unwrap().flags["token"], "abc123");

        // CLI overrides env
        assert_eq!(
            parser.parse(&["cmd", "--token=override"]).unwrap().flags["token"],
            "override"
        );
        std::env::remove_var(var);
    }

    #[test]
    fn test_env_var_fallback_with_default() {
        let var = "ARGOT_TEST_ENVFLAG_DEFAULT_22222";
        std::env::remove_var(var);
        let cmds = vec![Command::builder("cmd")
            .flag(
                Flag::builder("mode")
                    .takes_value()
                    .env(var)
                    .default_value("dev")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);

        // No CLI, no env → default
        assert_eq!(parser.parse(&["cmd"]).unwrap().flags["mode"], "dev");

        // No CLI, env set → env wins over default
        std::env::set_var(var, "prod");
        assert_eq!(parser.parse(&["cmd"]).unwrap().flags["mode"], "prod");
        std::env::remove_var(var);
    }

    #[test]
    fn test_env_var_satisfies_required_flag() {
        let var = "ARGOT_TEST_ENVFLAG_REQUIRED_33333";
        std::env::remove_var(var);
        let cmds = vec![Command::builder("cmd")
            .flag(
                Flag::builder("token")
                    .takes_value()
                    .required()
                    .env(var)
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);

        // Required + env set → OK
        std::env::set_var(var, "secret");
        assert_eq!(parser.parse(&["cmd"]).unwrap().flags["token"], "secret");

        // Required + env absent → MissingFlag
        std::env::remove_var(var);
        assert!(matches!(
            parser.parse(&["cmd"]),
            Err(ParseError::MissingFlag(_))
        ));
    }

    #[test]
    fn test_env_var_validates_choices() {
        let var = "ARGOT_TEST_ENVFLAG_CHOICES_44444";
        std::env::remove_var(var);
        let cmds = vec![Command::builder("cmd")
            .flag(
                Flag::builder("env_name")
                    .takes_value()
                    .choices(["prod", "staging"])
                    .env(var)
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap()];
        let parser = Parser::new(&cmds);

        std::env::set_var(var, "staging");
        assert_eq!(parser.parse(&["cmd"]).unwrap().flags["env_name"], "staging");

        std::env::set_var(var, "local");
        assert!(matches!(
            parser.parse(&["cmd"]),
            Err(ParseError::InvalidChoice { .. })
        ));

        std::env::remove_var(var);
    }

    #[test]
    fn test_exclusive_flags_one_set_ok() {
        let cmd = Command::builder("export")
            .flag(Flag::builder("json").build().unwrap())
            .flag(Flag::builder("yaml").build().unwrap())
            .exclusive(["json", "yaml"])
            .build()
            .unwrap();
        let cmds = vec![cmd];
        let parser = Parser::new(&cmds);
        let parsed = parser.parse(&["export", "--json"]).unwrap();
        assert_eq!(parsed.flags["json"], "true");
    }

    #[test]
    fn test_exclusive_flags_two_set_errors() {
        let cmd = Command::builder("export")
            .flag(Flag::builder("json").build().unwrap())
            .flag(Flag::builder("yaml").build().unwrap())
            .exclusive(["json", "yaml"])
            .build()
            .unwrap();
        let cmds = vec![cmd];
        let parser = Parser::new(&cmds);
        assert!(matches!(
            parser.parse(&["export", "--json", "--yaml"]),
            Err(ParseError::MutuallyExclusive { .. })
        ));
    }

    #[test]
    fn test_exclusive_neither_set_ok() {
        let cmd = Command::builder("export")
            .flag(Flag::builder("json").build().unwrap())
            .flag(Flag::builder("yaml").build().unwrap())
            .exclusive(["json", "yaml"])
            .build()
            .unwrap();
        let cmds = vec![cmd];
        let parser = Parser::new(&cmds);
        assert!(parser.parse(&["export"]).is_ok());
    }

    #[test]
    fn test_exclusive_group_unknown_flag_build_error() {
        use crate::model::BuildError;
        let result = Command::builder("cmd")
            .flag(Flag::builder("json").build().unwrap())
            .exclusive(["json", "nonexistent"])
            .build();
        assert!(matches!(
            result,
            Err(BuildError::ExclusiveGroupUnknownFlag(_))
        ));
    }

    #[test]
    fn test_exclusive_group_too_small_build_error() {
        use crate::model::BuildError;
        let result = Command::builder("cmd")
            .flag(Flag::builder("json").build().unwrap())
            .exclusive(["json"])
            .build();
        assert!(matches!(result, Err(BuildError::ExclusiveGroupTooSmall)));
    }
}

#[cfg(test)]
mod typed_getter_tests {
    use super::*;
    use crate::model::{Argument, Command, Flag};

    #[test]
    fn test_parsed_command_typed_getters() {
        let cmd = Command::builder("run")
            .argument(Argument::builder("script").required().build().unwrap())
            .flag(Flag::builder("verbose").short('v').build().unwrap())
            .flag(
                Flag::builder("output")
                    .takes_value()
                    .default_value("text")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        let cmds = vec![cmd];
        let parser = Parser::new(&cmds);
        let parsed = parser.parse(&["run", "myscript", "-v"]).unwrap();

        assert_eq!(parsed.arg("script"), Some("myscript"));
        assert_eq!(parsed.arg("missing"), None);
        assert_eq!(parsed.flag("verbose"), Some("true"));
        assert_eq!(parsed.flag("output"), Some("text")); // default
        assert!(parsed.flag_bool("verbose"));
        assert!(!parsed.flag_bool("output")); // not a boolean flag
        assert_eq!(parsed.flag_count("verbose"), 1);
        assert_eq!(parsed.flag_count("missing"), 0);
        assert_eq!(parsed.flag_values("output"), vec!["text"]);
        assert!(parsed.flag_values("missing").is_empty());
    }
}

#[cfg(test)]
mod has_flag_tests {
    use super::*;
    use crate::model::{Command, Flag};

    #[test]
    fn test_has_flag() {
        let cmd = Command::builder("run")
            .flag(Flag::builder("verbose").build().unwrap())
            .flag(
                Flag::builder("output")
                    .takes_value()
                    .default_value("text")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        let cmds = vec![cmd];
        let parser = Parser::new(&cmds);
        let parsed = parser.parse(&["run", "--verbose"]).unwrap();
        assert!(parsed.has_flag("verbose"));
        assert!(parsed.has_flag("output")); // present via default
        assert!(!parsed.has_flag("nonexistent"));
    }
}

#[cfg(test)]
mod coercion_tests {
    use super::*;
    use crate::model::{Argument, Command, Flag};

    #[test]
    fn test_arg_as_u32() {
        let cmd = Command::builder("resize")
            .argument(Argument::builder("width").required().build().unwrap())
            .build()
            .unwrap();
        let cmds = vec![cmd];
        let parsed = Parser::new(&cmds).parse(&["resize", "1920"]).unwrap();
        let w: u32 = parsed.arg_as("width").unwrap().unwrap();
        assert_eq!(w, 1920);
    }

    #[test]
    fn test_arg_as_parse_error() {
        let cmd = Command::builder("cmd")
            .argument(Argument::builder("n").required().build().unwrap())
            .build()
            .unwrap();
        let cmds = vec![cmd];
        let parsed = Parser::new(&cmds).parse(&["cmd", "notanumber"]).unwrap();
        assert!(parsed.arg_as::<u32>("n").unwrap().is_err());
    }

    #[test]
    fn test_arg_as_absent() {
        let cmd = Command::builder("cmd").build().unwrap();
        let cmds = vec![cmd];
        let parsed = Parser::new(&cmds).parse(&["cmd"]).unwrap();
        assert!(parsed.arg_as::<u32>("missing").is_none());
    }

    #[test]
    fn test_flag_as_u16() {
        let cmd = Command::builder("serve")
            .flag(
                Flag::builder("port")
                    .takes_value()
                    .default_value("8080")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        let cmds = vec![cmd];
        let parsed = Parser::new(&cmds).parse(&["serve"]).unwrap();
        let port: u16 = parsed.flag_as("port").unwrap().unwrap();
        assert_eq!(port, 8080);
    }

    #[test]
    fn test_flag_as_bool() {
        let cmd = Command::builder("run")
            .flag(Flag::builder("verbose").build().unwrap())
            .build()
            .unwrap();
        let cmds = vec![cmd];
        let parsed = Parser::new(&cmds).parse(&["run", "--verbose"]).unwrap();
        let v: bool = parsed.flag_as("verbose").unwrap().unwrap();
        assert!(v);
    }

    #[test]
    fn test_arg_as_or_default() {
        let cmd = Command::builder("run")
            .argument(Argument::builder("count").build().unwrap())
            .build()
            .unwrap();
        let cmds = vec![cmd];
        let parsed = Parser::new(&cmds).parse(&["run"]).unwrap();
        assert_eq!(parsed.arg_as_or("count", 42u32), 42u32);
    }

    #[test]
    fn test_flag_as_or_default() {
        let cmd = Command::builder("serve")
            .flag(Flag::builder("workers").takes_value().build().unwrap())
            .build()
            .unwrap();
        let cmds = vec![cmd];
        let parsed = Parser::new(&cmds).parse(&["serve"]).unwrap();
        assert_eq!(parsed.flag_as_or("workers", 4u32), 4u32);
    }
}
