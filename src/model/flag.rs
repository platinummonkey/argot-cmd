use serde::{Deserialize, Serialize};

use super::BuildError;

/// A named flag accepted by a command, e.g. `--verbose` or `-v`.
///
/// Flags can be boolean (no value) or value-taking (`--output json`). Boolean
/// flags are stored as `"true"` in [`crate::ParsedCommand::flags`] when
/// present. Use [`Flag::builder`] to construct instances.
///
/// # Examples
///
/// ```
/// # use argot::Flag;
/// let flag = Flag::builder("verbose")
///     .short('v')
///     .description("Enable verbose output")
///     .build()
///     .unwrap();
///
/// assert_eq!(flag.name, "verbose");
/// assert_eq!(flag.short, Some('v'));
/// assert!(!flag.takes_value);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Flag {
    /// The long flag name, used as `--name` on the command line and as the key
    /// in [`crate::ParsedCommand::flags`].
    pub name: String,
    /// Optional single-character short form, used as `-c` on the command line.
    pub short: Option<char>,
    /// Human-readable description shown in help output.
    pub description: String,
    /// Whether the parser returns an error when this flag is absent.
    pub required: bool,
    /// Whether the flag consumes the following token (or `=value`) as its value.
    ///
    /// When `false` the flag is boolean: its presence sets the value to
    /// `"true"`.
    pub takes_value: bool,
    /// Value substituted when the flag is not provided (optional flags only).
    pub default: Option<String>,
    /// If set, the flag value must be one of these strings (case-sensitive).
    /// Only meaningful when `takes_value` is true.
    pub choices: Option<Vec<String>>,
    /// If true, this flag may appear multiple times in an invocation.
    ///
    /// For boolean flags: occurrences are counted; stored as a numeric string
    /// (e.g., `-v -v -v` → `"3"`).
    /// For value-taking flags: values are collected into a JSON array string
    /// (e.g., `--tag a --tag b` → `["a","b"]`).
    pub repeatable: bool,
    /// Environment variable to check when the flag is absent from the command line.
    ///
    /// Lookup order: CLI argv → env var → `default` → required error.
    pub env: Option<String>,
}

/// Consuming builder for [`Flag`].
///
/// Obtain via [`Flag::builder`]. Call [`FlagBuilder::build`] when done.
///
/// # Examples
///
/// ```
/// # use argot::Flag;
/// let flag = Flag::builder("output")
///     .short('o')
///     .description("Output format")
///     .takes_value()
///     .default_value("text")
///     .build()
///     .unwrap();
///
/// assert!(flag.takes_value);
/// assert_eq!(flag.default.as_deref(), Some("text"));
/// ```
pub struct FlagBuilder {
    name: String,
    short: Option<char>,
    description: String,
    required: bool,
    takes_value: bool,
    default: Option<String>,
    choices: Option<Vec<String>>,
    repeatable: bool,
    env: Option<String>,
}

impl Flag {
    /// Create a new [`FlagBuilder`] with the given long flag name.
    ///
    /// # Arguments
    ///
    /// - `name` — The long flag name (without the `--` prefix). Must be
    ///   non-empty after trimming (enforced by [`FlagBuilder::build`]).
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot::Flag;
    /// let flag = Flag::builder("dry-run").build().unwrap();
    /// assert_eq!(flag.name, "dry-run");
    /// ```
    pub fn builder(name: impl Into<String>) -> FlagBuilder {
        FlagBuilder {
            name: name.into(),
            short: None,
            description: String::new(),
            required: false,
            takes_value: false,
            default: None,
            choices: None,
            repeatable: false,
            env: None,
        }
    }
}

impl FlagBuilder {
    /// Set the short single-character form of this flag (e.g. `'v'` for `-v`).
    pub fn short(mut self, c: char) -> Self {
        self.short = Some(c);
        self
    }

    /// Set the human-readable description shown in help output.
    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.description = d.into();
        self
    }

    /// Mark this flag as required.
    ///
    /// The parser will return [`crate::ParseError::MissingFlag`] if the flag
    /// is absent from the invocation.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Mark this flag as value-taking.
    ///
    /// When set, the parser expects either `--name=value` or `--name value`
    /// syntax. Without this, the flag is boolean and the mere presence of the
    /// flag sets the value to `"true"`.
    pub fn takes_value(mut self) -> Self {
        self.takes_value = true;
        self
    }

    /// Set the default value used when this flag is not provided.
    ///
    /// Only meaningful for optional (`!required`) value-taking flags. If the
    /// flag is absent from the invocation, the default is inserted into
    /// [`crate::ParsedCommand::flags`] automatically by the parser.
    pub fn default_value(mut self, d: impl Into<String>) -> Self {
        self.default = Some(d.into());
        self
    }

    /// Restrict this flag's value to one of the given choices.
    ///
    /// Only meaningful for value-taking flags (`takes_value()`).
    /// The parser returns [`crate::ParseError::InvalidChoice`] if the provided
    /// value is not in the list.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot::Flag;
    /// let flag = Flag::builder("format")
    ///     .takes_value()
    ///     .choices(["json", "yaml", "text"])
    ///     .build()
    ///     .unwrap();
    /// let expected: Vec<String> = vec!["json".into(), "yaml".into(), "text".into()];
    /// assert_eq!(flag.choices.as_deref(), Some(expected.as_slice()));
    /// ```
    pub fn choices(mut self, choices: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.choices = Some(choices.into_iter().map(Into::into).collect());
        self
    }

    /// Allow this flag to be specified more than once.
    ///
    /// For boolean flags: occurrences are counted and stored as a numeric string.
    /// For value-taking flags: values are collected into a JSON array string.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot::Flag;
    /// let flag = Flag::builder("verbose").repeatable().build().unwrap();
    /// assert!(flag.repeatable);
    /// ```
    pub fn repeatable(mut self) -> Self {
        self.repeatable = true;
        self
    }

    /// Register an environment variable as a fallback source for this flag.
    ///
    /// When the flag is not provided on the command line, the parser calls
    /// `std::env::var(var_name)`. If the variable is set and non-empty, its
    /// value is used (validated against `choices` if set). The full lookup
    /// order is: CLI → env var → default value → required error.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot::Flag;
    /// let flag = Flag::builder("token").takes_value().env("DEPLOY_TOKEN").build().unwrap();
    /// assert_eq!(flag.env.as_deref(), Some("DEPLOY_TOKEN"));
    /// ```
    pub fn env(mut self, var_name: impl Into<String>) -> Self {
        self.env = Some(var_name.into());
        self
    }

    /// Consume the builder and return a [`Flag`].
    ///
    /// # Errors
    ///
    /// Returns [`BuildError::EmptyCanonical`] if the flag name is empty or
    /// consists entirely of whitespace.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot::{Flag, BuildError};
    /// assert!(Flag::builder("verbose").build().is_ok());
    /// assert_eq!(Flag::builder("").build().unwrap_err(), BuildError::EmptyCanonical);
    /// ```
    pub fn build(self) -> Result<Flag, BuildError> {
        if self.name.trim().is_empty() {
            return Err(BuildError::EmptyCanonical);
        }
        Ok(Flag {
            name: self.name,
            short: self.short,
            description: self.description,
            required: self.required,
            takes_value: self.takes_value,
            default: self.default,
            choices: self.choices,
            repeatable: self.repeatable,
            env: self.env,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_happy_path() {
        let flag = Flag::builder("verbose")
            .short('v')
            .description("verbose output")
            .build()
            .unwrap();
        assert_eq!(flag.name, "verbose");
        assert_eq!(flag.short, Some('v'));
        assert!(!flag.required);
        assert!(!flag.takes_value);
    }

    #[test]
    fn test_builder_empty_name() {
        assert!(Flag::builder("").build().is_err());
        assert!(Flag::builder("  ").build().is_err());
    }

    #[test]
    fn test_takes_value_with_default() {
        let flag = Flag::builder("output")
            .takes_value()
            .default_value("stdout")
            .build()
            .unwrap();
        assert!(flag.takes_value);
        assert_eq!(flag.default.as_deref(), Some("stdout"));
    }

    #[test]
    fn test_serde_round_trip() {
        let flag = Flag::builder("format")
            .short('f')
            .takes_value()
            .required()
            .build()
            .unwrap();
        let json = serde_json::to_string(&flag).unwrap();
        let de: Flag = serde_json::from_str(&json).unwrap();
        assert_eq!(flag, de);
    }
}
