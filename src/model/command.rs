use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{Argument, BuildError, Example, Flag};

/// A handler function that can be registered on a [`Command`].
///
/// The function is stored in an [`Arc`] so that [`Command::clone`] only
/// increments a reference count rather than copying the closure. The
/// higher-ranked trait bound (`for<'a>`) allows the handler to be called with
/// a [`ParsedCommand`] of any lifetime, which is required because the parsed
/// command borrows from the command tree at call time.
///
/// # Examples
///
/// ```
/// # use std::sync::Arc;
/// # use argot::HandlerFn;
/// let handler: HandlerFn = Arc::new(|parsed| {
///     println!("running command: {}", parsed.command.canonical);
///     Ok(())
/// });
/// ```
pub type HandlerFn =
    Arc<dyn for<'a> Fn(&ParsedCommand<'a>) -> Result<(), Box<dyn std::error::Error>> + Send + Sync>;

/// The result of successfully parsing an invocation against a [`Command`].
///
/// `ParsedCommand` borrows the matched [`Command`] from the registry (lifetime
/// `'a`) and owns the resolved argument and flag value maps. Keys in both maps
/// are the canonical names of the argument/flag definitions.
///
/// # Examples
///
/// ```
/// # use argot::{Command, Argument, Parser};
/// let cmd = Command::builder("get")
///     .argument(
///         Argument::builder("id")
///             .required()
///             .build()
///             .unwrap(),
///     )
///     .build()
///     .unwrap();
///
/// let cmds = vec![cmd];
/// let parser = Parser::new(&cmds);
/// let parsed = parser.parse(&["get", "42"]).unwrap();
///
/// assert_eq!(parsed.command.canonical, "get");
/// assert_eq!(parsed.args["id"], "42");
/// ```
#[derive(Debug)]
pub struct ParsedCommand<'a> {
    /// The matched [`Command`] definition, borrowed from the registry.
    pub command: &'a Command,
    /// Resolved positional argument values, keyed by argument name.
    pub args: HashMap<String, String>,
    /// Resolved flag values, keyed by flag name.
    ///
    /// Boolean flags (those without `takes_value`) are stored as `"true"`.
    /// Flags that were not provided but have a `default` value are included
    /// with that default.
    pub flags: HashMap<String, String>,
}

impl<'a> ParsedCommand<'a> {
    /// Return a positional argument value by name, or `None` if absent.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot::{Argument, Command, Parser};
    /// let cmd = Command::builder("get")
    ///     .argument(Argument::builder("id").required().build().unwrap())
    ///     .build().unwrap();
    /// let cmds = vec![cmd];
    /// let parsed = Parser::new(&cmds).parse(&["get", "42"]).unwrap();
    /// assert_eq!(parsed.arg("id"), Some("42"));
    /// assert_eq!(parsed.arg("missing"), None);
    /// ```
    pub fn arg(&self, name: &str) -> Option<&str> {
        self.args.get(name).map(String::as_str)
    }

    /// Return a flag value by name, or `None` if absent.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot::{Command, Flag, Parser};
    /// let cmd = Command::builder("run")
    ///     .flag(Flag::builder("output").takes_value().default_value("text").build().unwrap())
    ///     .build().unwrap();
    /// let cmds = vec![cmd];
    /// let parsed = Parser::new(&cmds).parse(&["run", "--output=json"]).unwrap();
    /// assert_eq!(parsed.flag("output"), Some("json"));
    /// ```
    pub fn flag(&self, name: &str) -> Option<&str> {
        self.flags.get(name).map(String::as_str)
    }

    /// Return `true` if a boolean flag is present and set to `"true"`, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot::{Command, Flag, Parser};
    /// let cmd = Command::builder("build")
    ///     .flag(Flag::builder("release").build().unwrap())
    ///     .build().unwrap();
    /// let cmds = vec![cmd];
    /// let parsed = Parser::new(&cmds).parse(&["build", "--release"]).unwrap();
    /// assert!(parsed.flag_bool("release"));
    /// assert!(!parsed.flag_bool("missing"));
    /// ```
    pub fn flag_bool(&self, name: &str) -> bool {
        self.flags.get(name).map(|v| v == "true").unwrap_or(false)
    }

    /// Return the occurrence count for a repeatable boolean flag (stored as a numeric string).
    /// Returns `0` if the flag was not provided.
    ///
    /// For flags stored as `"true"` (non-repeatable), returns `1`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot::{Command, Flag, Parser};
    /// // With a repeatable flag (see Flag::repeatable), -v -v -v → flag_count("verbose") == 3
    /// // With a normal flag, --verbose → flag_count("verbose") == 1 (stored as "true")
    /// let cmd = Command::builder("run")
    ///     .flag(Flag::builder("verbose").short('v').build().unwrap())
    ///     .build().unwrap();
    /// let cmds = vec![cmd];
    /// let parsed = Parser::new(&cmds).parse(&["run", "-v"]).unwrap();
    /// // Non-repeatable flag stores "true"; flag_count returns 1
    /// assert_eq!(parsed.flag_count("verbose"), 1);
    /// assert_eq!(parsed.flag_count("missing"), 0);
    /// ```
    pub fn flag_count(&self, name: &str) -> u64 {
        match self.flags.get(name) {
            None => 0,
            Some(v) if v == "true" => 1,
            Some(v) if v == "false" => 0,
            Some(v) => v.parse().unwrap_or(0),
        }
    }

    /// Return all values for a repeatable value-taking flag as a `Vec<String>`.
    ///
    /// - If the flag was provided multiple times (repeatable), the stored JSON array is
    ///   deserialized into a `Vec`.
    /// - If the flag was provided once (non-repeatable), returns a single-element `Vec`.
    /// - If the flag was not provided, returns an empty `Vec`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot::{Command, Flag, Parser};
    /// let cmd = Command::builder("run")
    ///     .flag(Flag::builder("output").takes_value().default_value("text").build().unwrap())
    ///     .build().unwrap();
    /// let cmds = vec![cmd];
    /// let parsed = Parser::new(&cmds).parse(&["run", "--output=json"]).unwrap();
    /// assert_eq!(parsed.flag_values("output"), vec!["json"]);
    /// assert!(parsed.flag_values("missing").is_empty());
    /// ```
    pub fn flag_values(&self, name: &str) -> Vec<String> {
        match self.flags.get(name) {
            None => vec![],
            Some(v) => serde_json::from_str::<Vec<String>>(v).unwrap_or_else(|_| vec![v.clone()]),
        }
    }

    /// Return `true` if `name` is present in the parsed flag map.
    ///
    /// A flag is present when it was explicitly provided on the command line
    /// **or** when the parser inserted a default or env-var value.
    /// To distinguish explicit from default, use [`ParsedCommand::flag`] and
    /// compare with the flag definition's default.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot::{Command, Flag, Parser};
    /// let cmd = Command::builder("run")
    ///     .flag(Flag::builder("verbose").build().unwrap())
    ///     .flag(Flag::builder("output").takes_value().default_value("text").build().unwrap())
    ///     .build().unwrap();
    /// let cmds = vec![cmd];
    /// let parsed = Parser::new(&cmds).parse(&["run", "--verbose"]).unwrap();
    /// assert!(parsed.has_flag("verbose"));
    /// assert!(parsed.has_flag("output"));  // present via default
    /// assert!(!parsed.has_flag("missing"));
    /// ```
    pub fn has_flag(&self, name: &str) -> bool {
        self.flags.contains_key(name)
    }
}

/// A command in the registry, potentially with subcommands.
///
/// Commands are the central unit of argot. Each command has a canonical name,
/// optional aliases and alternate spellings, human-readable documentation,
/// typed positional arguments, named flags, usage examples, and an optional
/// handler closure. Commands can be nested arbitrarily deep via
/// [`Command::subcommands`].
///
/// Use [`Command::builder`] to construct instances — direct struct
/// construction is intentionally not exposed.
///
/// # Serialization
///
/// `Command` implements `serde::Serialize` / `Deserialize`. The [`handler`]
/// field is skipped during serialization (it cannot be represented in JSON)
/// and will be `None` after deserialization.
///
/// # Examples
///
/// ```
/// # use argot::{Command, Argument, Flag, Example};
/// let cmd = Command::builder("deploy")
///     .alias("d")
///     .summary("Deploy the app")
///     .description("Deploys to the specified environment.")
///     .argument(
///         Argument::builder("env")
///             .description("Target environment")
///             .required()
///             .build()
///             .unwrap(),
///     )
///     .flag(
///         Flag::builder("dry-run")
///             .short('n')
///             .description("Simulate only")
///             .build()
///             .unwrap(),
///     )
///     .example(Example::new("deploy to prod", "myapp deploy production"))
///     .build()
///     .unwrap();
///
/// assert_eq!(cmd.canonical, "deploy");
/// assert_eq!(cmd.aliases, vec!["d"]);
/// ```
///
/// [`handler`]: Command::handler
#[derive(Clone, Serialize, Deserialize)]
pub struct Command {
    /// The primary, canonical name used to invoke this command.
    pub canonical: String,
    /// Alternative names that resolve to this command (e.g. `"ls"` for `"list"`).
    pub aliases: Vec<String>,
    /// Alternate capitalizations or spellings (e.g. `"LIST"` for `"list"`).
    ///
    /// Spellings differ from aliases semantically: they represent the same
    /// word written differently rather than a short-form abbreviation.
    pub spellings: Vec<String>,
    /// One-line description shown in command listings.
    pub summary: String,
    /// Full prose description shown in detailed help output.
    pub description: String,
    /// Ordered list of positional arguments accepted by this command.
    pub arguments: Vec<Argument>,
    /// Named flags (long and/or short) accepted by this command.
    pub flags: Vec<Flag>,
    /// Usage examples shown in help and Markdown documentation.
    pub examples: Vec<Example>,
    /// Nested sub-commands (e.g. `remote add`, `remote remove`).
    pub subcommands: Vec<Command>,
    /// Prose tips about correct usage, surfaced to AI agents.
    pub best_practices: Vec<String>,
    /// Prose warnings about incorrect usage, surfaced to AI agents.
    pub anti_patterns: Vec<String>,
    /// Optional runtime handler invoked by [`crate::cli::Cli::run`].
    ///
    /// Skipped during JSON serialization/deserialization.
    #[serde(skip)]
    pub handler: Option<HandlerFn>,
    /// Arbitrary application-defined metadata.
    ///
    /// Use this to attach structured data that is not covered by the built-in
    /// fields (e.g., permission requirements, category tags, telemetry labels).
    ///
    /// Serialized to JSON as an object; absent from output when empty.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Command")
            .field("canonical", &self.canonical)
            .field("aliases", &self.aliases)
            .field("spellings", &self.spellings)
            .field("summary", &self.summary)
            .field("description", &self.description)
            .field("arguments", &self.arguments)
            .field("flags", &self.flags)
            .field("examples", &self.examples)
            .field("subcommands", &self.subcommands)
            .field("best_practices", &self.best_practices)
            .field("anti_patterns", &self.anti_patterns)
            .field("handler", &self.handler.as_ref().map(|_| "<handler>"))
            .field("extra", &self.extra)
            .finish()
    }
}

impl PartialEq for Command {
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
            && self.aliases == other.aliases
            && self.spellings == other.spellings
            && self.summary == other.summary
            && self.description == other.description
            && self.arguments == other.arguments
            && self.flags == other.flags
            && self.examples == other.examples
            && self.subcommands == other.subcommands
            && self.best_practices == other.best_practices
            && self.anti_patterns == other.anti_patterns
            && self.extra == other.extra
    }
}

impl Eq for Command {}

impl std::hash::Hash for Command {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.canonical.hash(state);
        self.aliases.hash(state);
        self.spellings.hash(state);
        self.summary.hash(state);
        self.description.hash(state);
        self.arguments.hash(state);
        self.flags.hash(state);
        self.examples.hash(state);
        self.subcommands.hash(state);
        self.best_practices.hash(state);
        self.anti_patterns.hash(state);
        // handler is intentionally excluded (not hashable)
        // extra: hash keys in sorted order, with their JSON string representation
        {
            let mut keys: Vec<&String> = self.extra.keys().collect();
            keys.sort();
            for k in keys {
                k.hash(state);
                self.extra[k].to_string().hash(state);
            }
        }
    }
}

impl PartialOrd for Command {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Commands are ordered by canonical name, then by their full field contents.
impl Ord for Command {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.canonical
            .cmp(&other.canonical)
            .then_with(|| self.summary.cmp(&other.summary))
            .then_with(|| self.aliases.cmp(&other.aliases))
    }
}

impl Command {
    /// Create a new [`CommandBuilder`] with the given canonical name.
    ///
    /// # Arguments
    ///
    /// - `canonical` — The primary command name. Must be non-empty after
    ///   trimming (enforced by [`CommandBuilder::build`]).
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot::Command;
    /// let cmd = Command::builder("list").build().unwrap();
    /// assert_eq!(cmd.canonical, "list");
    /// ```
    pub fn builder(canonical: impl Into<String>) -> CommandBuilder {
        CommandBuilder {
            canonical: canonical.into(),
            aliases: Vec::new(),
            spellings: Vec::new(),
            summary: String::new(),
            description: String::new(),
            arguments: Vec::new(),
            flags: Vec::new(),
            examples: Vec::new(),
            subcommands: Vec::new(),
            best_practices: Vec::new(),
            anti_patterns: Vec::new(),
            handler: None,
            extra: HashMap::new(),
        }
    }

    /// All strings this command can be matched by (canonical + aliases + spellings), lowercased.
    pub(crate) fn matchable_strings(&self) -> Vec<String> {
        let mut v = vec![self.canonical.to_lowercase()];
        v.extend(self.aliases.iter().map(|s| s.to_lowercase()));
        v.extend(self.spellings.iter().map(|s| s.to_lowercase()));
        v
    }
}

/// Consuming builder for [`Command`].
///
/// Obtain an instance via [`Command::builder`]. All setter methods consume and
/// return `self` to allow method chaining. Call [`CommandBuilder::build`] to
/// produce a [`Command`].
///
/// # Examples
///
/// ```
/// # use argot::{Command, Flag};
/// let cmd = Command::builder("run")
///     .alias("r")
///     .summary("Run the pipeline")
///     .flag(Flag::builder("verbose").short('v').build().unwrap())
///     .build()
///     .unwrap();
///
/// assert_eq!(cmd.canonical, "run");
/// assert_eq!(cmd.aliases, vec!["r"]);
/// ```
pub struct CommandBuilder {
    canonical: String,
    aliases: Vec<String>,
    spellings: Vec<String>,
    summary: String,
    description: String,
    arguments: Vec<Argument>,
    flags: Vec<Flag>,
    examples: Vec<Example>,
    subcommands: Vec<Command>,
    best_practices: Vec<String>,
    anti_patterns: Vec<String>,
    handler: Option<HandlerFn>,
    extra: HashMap<String, serde_json::Value>,
}

impl CommandBuilder {
    /// Replace the entire alias list with the given collection.
    ///
    /// To add a single alias use [`CommandBuilder::alias`].
    pub fn aliases(mut self, aliases: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.aliases = aliases.into_iter().map(Into::into).collect();
        self
    }

    /// Append a single alias.
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Replace the entire spelling list with the given collection.
    ///
    /// To add a single spelling use [`CommandBuilder::spelling`].
    pub fn spellings(mut self, spellings: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.spellings = spellings.into_iter().map(Into::into).collect();
        self
    }

    /// Append a single alternate spelling.
    pub fn spelling(mut self, spelling: impl Into<String>) -> Self {
        self.spellings.push(spelling.into());
        self
    }

    /// Set the one-line summary shown in command listings.
    pub fn summary(mut self, s: impl Into<String>) -> Self {
        self.summary = s.into();
        self
    }

    /// Set the full prose description shown in detailed help output.
    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.description = d.into();
        self
    }

    /// Append a positional [`Argument`] definition.
    ///
    /// Arguments are bound in declaration order when the command is parsed.
    pub fn argument(mut self, arg: Argument) -> Self {
        self.arguments.push(arg);
        self
    }

    /// Append a [`Flag`] definition.
    pub fn flag(mut self, flag: Flag) -> Self {
        self.flags.push(flag);
        self
    }

    /// Append a usage [`Example`].
    pub fn example(mut self, example: Example) -> Self {
        self.examples.push(example);
        self
    }

    /// Append a subcommand.
    ///
    /// Subcommands are resolved before positional arguments during parsing, so
    /// a token that matches a subcommand name is consumed as the subcommand
    /// selector, not as a positional value.
    pub fn subcommand(mut self, cmd: Command) -> Self {
        self.subcommands.push(cmd);
        self
    }

    /// Append a best-practice tip surfaced to AI agents.
    pub fn best_practice(mut self, bp: impl Into<String>) -> Self {
        self.best_practices.push(bp.into());
        self
    }

    /// Append an anti-pattern warning surfaced to AI agents.
    pub fn anti_pattern(mut self, ap: impl Into<String>) -> Self {
        self.anti_patterns.push(ap.into());
        self
    }

    /// Set the runtime handler invoked when this command is dispatched.
    ///
    /// The handler receives a [`ParsedCommand`] and should return `Ok(())`
    /// on success or a boxed error on failure.
    pub fn handler(mut self, h: HandlerFn) -> Self {
        self.handler = Some(h);
        self
    }

    /// Set an arbitrary metadata entry on this command.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot::Command;
    /// # use serde_json::json;
    /// let cmd = Command::builder("deploy")
    ///     .meta("category", json!("infrastructure"))
    ///     .meta("min_role", json!("ops"))
    ///     .build()
    ///     .unwrap();
    ///
    /// assert_eq!(cmd.extra["category"], json!("infrastructure"));
    /// ```
    pub fn meta(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.extra.insert(key.into(), value);
        self
    }

    /// Consume the builder and return a [`Command`].
    ///
    /// # Errors
    ///
    /// - [`BuildError::EmptyCanonical`] — canonical name is empty or whitespace.
    /// - [`BuildError::AliasEqualsCanonical`] — an alias is identical to the
    ///   canonical name (case-insensitive).
    /// - [`BuildError::DuplicateAlias`] — two aliases share the same string
    ///   (case-insensitive comparison).
    /// - [`BuildError::DuplicateFlagName`] — two flags share the same long name.
    /// - [`BuildError::DuplicateShortFlag`] — two flags share the same short
    ///   character.
    /// - [`BuildError::DuplicateArgumentName`] — two positional arguments share
    ///   the same name.
    /// - [`BuildError::DuplicateSubcommandName`] — two subcommands share the
    ///   same canonical name.
    /// - [`BuildError::VariadicNotLast`] — a variadic argument is not the last
    ///   argument in the list.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot::{Command, BuildError};
    /// assert!(Command::builder("ok").build().is_ok());
    /// assert_eq!(Command::builder("").build().unwrap_err(), BuildError::EmptyCanonical);
    /// ```
    pub fn build(self) -> Result<Command, BuildError> {
        if self.canonical.trim().is_empty() {
            return Err(BuildError::EmptyCanonical);
        }

        // 1. Alias equals canonical
        let canonical_lower = self.canonical.to_lowercase();
        for alias in &self.aliases {
            if alias.to_lowercase() == canonical_lower {
                return Err(BuildError::AliasEqualsCanonical(alias.clone()));
            }
        }

        // 2. Duplicate aliases
        let mut seen_aliases = std::collections::HashSet::new();
        for alias in &self.aliases {
            let key = alias.to_lowercase();
            if !seen_aliases.insert(key) {
                return Err(BuildError::DuplicateAlias(alias.clone()));
            }
        }

        // 3. Duplicate flag names (long)
        let mut seen_flag_names = std::collections::HashSet::new();
        for flag in &self.flags {
            if !seen_flag_names.insert(flag.name.clone()) {
                return Err(BuildError::DuplicateFlagName(flag.name.clone()));
            }
        }

        // 4. Duplicate short flags
        let mut seen_short_flags = std::collections::HashSet::new();
        for flag in &self.flags {
            if let Some(c) = flag.short {
                if !seen_short_flags.insert(c) {
                    return Err(BuildError::DuplicateShortFlag(c));
                }
            }
        }

        // 5. Duplicate argument names
        let mut seen_arg_names = std::collections::HashSet::new();
        for arg in &self.arguments {
            if !seen_arg_names.insert(arg.name.clone()) {
                return Err(BuildError::DuplicateArgumentName(arg.name.clone()));
            }
        }

        // 6. Duplicate subcommand canonical names
        let mut seen_sub_names = std::collections::HashSet::new();
        for sub in &self.subcommands {
            if !seen_sub_names.insert(sub.canonical.clone()) {
                return Err(BuildError::DuplicateSubcommandName(sub.canonical.clone()));
            }
        }

        // 7. Variadic argument must be last
        for (i, arg) in self.arguments.iter().enumerate() {
            if arg.variadic && i != self.arguments.len() - 1 {
                return Err(BuildError::VariadicNotLast(arg.name.clone()));
            }
        }

        // 8. Flags with choices must have a non-empty choices list
        for flag in &self.flags {
            if let Some(choices) = &flag.choices {
                if choices.is_empty() {
                    return Err(BuildError::EmptyChoices(flag.name.clone()));
                }
            }
        }

        Ok(Command {
            canonical: self.canonical,
            aliases: self.aliases,
            spellings: self.spellings,
            summary: self.summary,
            description: self.description,
            arguments: self.arguments,
            flags: self.flags,
            examples: self.examples,
            subcommands: self.subcommands,
            best_practices: self.best_practices,
            anti_patterns: self.anti_patterns,
            handler: self.handler,
            extra: self.extra,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Argument, Flag};

    fn make_simple_cmd() -> Command {
        Command::builder("run")
            .alias("r")
            .spelling("RUN")
            .summary("Run something")
            .description("Runs the thing.")
            .build()
            .unwrap()
    }

    #[test]
    fn test_builder_happy_path() {
        let cmd = make_simple_cmd();
        assert_eq!(cmd.canonical, "run");
        assert_eq!(cmd.aliases, vec!["r"]);
        assert_eq!(cmd.spellings, vec!["RUN"]);
    }

    #[test]
    fn test_builder_empty_canonical() {
        assert_eq!(
            Command::builder("").build().unwrap_err(),
            BuildError::EmptyCanonical
        );
        assert_eq!(
            Command::builder("   ").build().unwrap_err(),
            BuildError::EmptyCanonical
        );
    }

    #[test]
    fn test_partial_eq_ignores_handler() {
        let cmd1 = Command::builder("run").build().unwrap();
        let mut cmd2 = cmd1.clone();
        cmd2.handler = Some(Arc::new(|_| Ok(())));
        assert_eq!(cmd1, cmd2);
    }

    #[test]
    fn test_serde_round_trip_skips_handler() {
        let cmd = Command::builder("deploy")
            .summary("Deploy the app")
            .argument(
                Argument::builder("env")
                    .description("target env")
                    .required()
                    .build()
                    .unwrap(),
            )
            .flag(
                Flag::builder("dry-run")
                    .description("dry run mode")
                    .build()
                    .unwrap(),
            )
            .handler(Arc::new(|_| Ok(())))
            .build()
            .unwrap();

        let json = serde_json::to_string(&cmd).unwrap();
        let de: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, de);
        assert!(de.handler.is_none());
    }

    #[test]
    fn test_matchable_strings() {
        let cmd = Command::builder("Git")
            .alias("g")
            .spelling("GIT")
            .build()
            .unwrap();
        let matchables = cmd.matchable_strings();
        assert!(matchables.contains(&"git".to_string()));
        assert!(matchables.contains(&"g".to_string()));
        assert!(matchables.contains(&"git".to_string())); // spelling lowercased
    }

    #[test]
    fn test_clone_shares_handler() {
        let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called_clone = called.clone();
        let cmd = Command::builder("x")
            .handler(Arc::new(move |_| {
                called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }))
            .build()
            .unwrap();
        let cmd2 = cmd.clone();
        assert!(std::sync::Arc::ptr_eq(
            cmd.handler.as_ref().unwrap(),
            cmd2.handler.as_ref().unwrap()
        ));
    }

    #[test]
    fn test_duplicate_alias_rejected() {
        let err = Command::builder("cmd")
            .alias("a")
            .alias("a")
            .build()
            .unwrap_err();
        assert!(matches!(err, BuildError::DuplicateAlias(_)));
    }

    #[test]
    fn test_alias_equals_canonical_rejected() {
        let err = Command::builder("deploy")
            .alias("deploy")
            .build()
            .unwrap_err();
        assert!(matches!(err, BuildError::AliasEqualsCanonical(_)));
    }

    #[test]
    fn test_duplicate_flag_name_rejected() {
        let flag = Flag::builder("verbose").build().unwrap();
        let err = Command::builder("cmd")
            .flag(flag.clone())
            .flag(flag)
            .build()
            .unwrap_err();
        assert!(matches!(err, BuildError::DuplicateFlagName(_)));
    }

    #[test]
    fn test_duplicate_short_flag_rejected() {
        let f1 = Flag::builder("verbose").short('v').build().unwrap();
        let f2 = Flag::builder("version").short('v').build().unwrap();
        let err = Command::builder("cmd")
            .flag(f1)
            .flag(f2)
            .build()
            .unwrap_err();
        assert!(matches!(err, BuildError::DuplicateShortFlag('v')));
    }

    #[test]
    fn test_duplicate_argument_name_rejected() {
        let arg = Argument::builder("env").build().unwrap();
        let err = Command::builder("cmd")
            .argument(arg.clone())
            .argument(arg)
            .build()
            .unwrap_err();
        assert!(matches!(err, BuildError::DuplicateArgumentName(_)));
    }

    #[test]
    fn test_duplicate_subcommand_name_rejected() {
        let sub = Command::builder("add").build().unwrap();
        let err = Command::builder("remote")
            .subcommand(sub.clone())
            .subcommand(sub)
            .build()
            .unwrap_err();
        assert!(matches!(err, BuildError::DuplicateSubcommandName(_)));
    }

    #[test]
    fn test_variadic_must_be_last() {
        let variadic = Argument::builder("files").variadic().build().unwrap();
        let after = Argument::builder("extra").build().unwrap();
        let err = Command::builder("cmd")
            .argument(variadic)
            .argument(after)
            .build()
            .unwrap_err();
        assert!(matches!(err, BuildError::VariadicNotLast(_)));
    }

    #[test]
    fn test_meta_field_serde() {
        let cmd = Command::builder("x")
            .meta("role", serde_json::json!("admin"))
            .build()
            .unwrap();
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("admin"));
        let de: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(de.extra["role"], serde_json::json!("admin"));
    }

    #[test]
    fn test_meta_empty_not_serialized() {
        let cmd = Command::builder("x").build().unwrap();
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(!json.contains("extra"));
    }
}
