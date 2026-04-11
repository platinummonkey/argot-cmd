//! Hardened input validation middleware for agent-generated CLI input.
//!
//! AI agents may hallucinate or produce adversarial values. This module
//! provides [`InputValidator`], an opt-in validator that can be wired into
//! the [`crate::cli::Cli`] dispatch loop as [`crate::middleware::Middleware`],
//! or called directly via [`InputValidator::validate_value`] and
//! [`InputValidator::validate_parsed`].
//!
//! ## Example
//!
//! ```
//! use argot_cmd::input_validation::InputValidator;
//! use argot_cmd::Middleware;
//! use argot_cmd::ParsedCommand;
//!
//! // Enable all checks at once.
//! let validator = InputValidator::strict();
//!
//! // Or selectively opt in.
//! let selective = InputValidator::new()
//!     .check_path_traversal()
//!     .check_control_chars();
//! ```

use thiserror::Error;

use crate::middleware::Middleware;
use crate::model::ParsedCommand;

/// Errors produced by [`InputValidator`].
#[derive(Debug, Error, PartialEq)]
pub enum ValidationError {
    /// A field value contains a path traversal sequence (`../`, `..\`, `/…`, or `~…`).
    #[error(
        "field `{field}` contains a path traversal sequence in value: {value:?}"
    )]
    PathTraversal {
        /// Name of the argument or flag that triggered the error.
        field: String,
        /// The offending value.
        value: String,
    },

    /// A field value contains an ASCII control character (0x00–0x1F or 0x7F),
    /// excluding horizontal tab (0x09) and newline (0x0A).
    #[error(
        "field `{field}` contains a control character in value: {value:?}"
    )]
    ControlCharacter {
        /// Name of the argument or flag that triggered the error.
        field: String,
        /// The offending value.
        value: String,
    },

    /// A field value appears to contain an embedded URL query string
    /// (`?` or `&key=val` patterns).
    #[error(
        "field `{field}` contains an embedded query parameter in value: {value:?}"
    )]
    QueryInjection {
        /// Name of the argument or flag that triggered the error.
        field: String,
        /// The offending value.
        value: String,
    },

    /// A field value contains a percent-encoded (`%XX`) sequence.
    #[error(
        "field `{field}` contains a URL-encoded sequence in value: {value:?}"
    )]
    UrlEncoding {
        /// Name of the argument or flag that triggered the error.
        field: String,
        /// The offending value.
        value: String,
    },
}

/// Opt-in validator for argument and flag values supplied to a parsed command.
///
/// Create a permissive instance with [`InputValidator::new`] and enable
/// individual checks through the builder methods, or use [`InputValidator::strict`]
/// to enable every check at once.
///
/// # Examples
///
/// ```
/// use argot_cmd::input_validation::InputValidator;
///
/// // Only check for path traversal.
/// let v = InputValidator::new().check_path_traversal();
/// assert!(v.validate_value("file", "safe_name.txt").is_ok());
/// assert!(v.validate_value("file", "../etc/passwd").is_err());
/// ```
#[derive(Debug, Clone, Default)]
pub struct InputValidator {
    path_traversal: bool,
    control_chars: bool,
    query_injection: bool,
    url_encoding: bool,
}

impl InputValidator {
    /// Create a new [`InputValidator`] with all checks **disabled**.
    ///
    /// Use the builder methods (`.check_path_traversal()`, etc.) to opt in
    /// to specific checks, or call [`InputValidator::strict`] to enable all
    /// of them at once.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an [`InputValidator`] with **all** checks enabled.
    ///
    /// Equivalent to:
    /// ```
    /// # use argot_cmd::input_validation::InputValidator;
    /// InputValidator::new()
    ///     .check_path_traversal()
    ///     .check_control_chars()
    ///     .check_query_injection()
    ///     .check_url_encoding();
    /// ```
    pub fn strict() -> Self {
        Self {
            path_traversal: true,
            control_chars: true,
            query_injection: true,
            url_encoding: true,
        }
    }

    /// Enable path-traversal detection.
    ///
    /// Flags values containing `../`, `..\`, or starting with `/` or `~`.
    pub fn check_path_traversal(mut self) -> Self {
        self.path_traversal = true;
        self
    }

    /// Enable control-character detection.
    ///
    /// Flags values containing ASCII bytes in the range 0x00–0x1F or 0x7F,
    /// **except** horizontal tab (0x09) and newline (0x0A).
    pub fn check_control_chars(mut self) -> Self {
        self.control_chars = true;
        self
    }

    /// Enable embedded query-parameter detection.
    ///
    /// Flags values that contain `?` or match the pattern `&<key>=<val>`,
    /// which may indicate URL-injection attempts.
    pub fn check_query_injection(mut self) -> Self {
        self.query_injection = true;
        self
    }

    /// Enable percent-encoded string detection.
    ///
    /// Flags values containing `%XX` sequences (where `XX` is a pair of hex
    /// digits), which may indicate attempts to smuggle disallowed characters
    /// past earlier checks.
    pub fn check_url_encoding(mut self) -> Self {
        self.url_encoding = true;
        self
    }

    /// Validate a single named value against all enabled checks.
    ///
    /// Returns the first [`ValidationError`] encountered, or `Ok(())` if the
    /// value passes every enabled check.
    ///
    /// # Arguments
    ///
    /// * `field` — the name of the argument or flag being validated (used in
    ///   the error message).
    /// * `value` — the string value to inspect.
    ///
    /// # Examples
    ///
    /// ```
    /// use argot_cmd::input_validation::InputValidator;
    ///
    /// let v = InputValidator::strict();
    /// assert!(v.validate_value("path", "hello.txt").is_ok());
    /// assert!(v.validate_value("path", "../secret").is_err());
    /// ```
    pub fn validate_value(&self, field: &str, value: &str) -> Result<(), ValidationError> {
        if self.path_traversal && contains_path_traversal(value) {
            return Err(ValidationError::PathTraversal {
                field: field.to_owned(),
                value: value.to_owned(),
            });
        }

        if self.control_chars && contains_control_char(value) {
            return Err(ValidationError::ControlCharacter {
                field: field.to_owned(),
                value: value.to_owned(),
            });
        }

        if self.query_injection && contains_query_injection(value) {
            return Err(ValidationError::QueryInjection {
                field: field.to_owned(),
                value: value.to_owned(),
            });
        }

        if self.url_encoding && contains_url_encoding(value) {
            return Err(ValidationError::UrlEncoding {
                field: field.to_owned(),
                value: value.to_owned(),
            });
        }

        Ok(())
    }

    /// Validate all argument and flag values in a [`ParsedCommand`].
    ///
    /// Iterates over every entry in `parsed.args` and `parsed.flags` and calls
    /// [`InputValidator::validate_value`] on each. Returns the first error
    /// encountered, or `Ok(())` when every value passes.
    ///
    /// # Examples
    ///
    /// ```
    /// use argot_cmd::{Command, Argument, Parser};
    /// use argot_cmd::input_validation::InputValidator;
    ///
    /// let cmd = Command::builder("get")
    ///     .argument(Argument::builder("id").required().build().unwrap())
    ///     .build()
    ///     .unwrap();
    /// let cmds = vec![cmd];
    /// let parser = Parser::new(&cmds);
    /// let parsed = parser.parse(&["get", "safe_value"]).unwrap();
    ///
    /// let v = InputValidator::strict();
    /// assert!(v.validate_parsed(&parsed).is_ok());
    /// ```
    pub fn validate_parsed(&self, parsed: &ParsedCommand<'_>) -> Result<(), ValidationError> {
        for (field, value) in &parsed.args {
            self.validate_value(field, value)?;
        }
        for (field, value) in &parsed.flags {
            self.validate_value(field, value)?;
        }
        Ok(())
    }
}

impl Middleware for InputValidator {
    /// Validate all argument and flag values before the handler is invoked.
    ///
    /// Returns a [`ValidationError`] (boxed) if any enabled check fails,
    /// which causes [`crate::cli::Cli`] to abort dispatch and surface the
    /// error to the caller.
    fn before_dispatch(
        &self,
        parsed: &ParsedCommand<'_>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.validate_parsed(parsed)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Returns `true` when `value` contains a path traversal pattern.
fn contains_path_traversal(value: &str) -> bool {
    value.contains("../")
        || value.contains("..\\")
        || value.starts_with('/')
        || value.starts_with('~')
}

/// Returns `true` when `value` contains an ASCII control character other than
/// horizontal tab (0x09) and newline (0x0A).
fn contains_control_char(value: &str) -> bool {
    value.bytes().any(|b| {
        let is_control = b <= 0x1F || b == 0x7F;
        let is_allowed = b == b'\t' || b == b'\n';
        is_control && !is_allowed
    })
}

/// Returns `true` when `value` contains `?` or an `&key=val` pattern.
fn contains_query_injection(value: &str) -> bool {
    if value.contains('?') {
        return true;
    }
    // Look for &key=val — an ampersand followed by at least one word char and
    // then an equals sign.
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            // Scan for '=' after the '&'
            let rest = &bytes[i + 1..];
            if let Some(eq_pos) = rest.iter().position(|&b| b == b'=') {
                // There must be at least one non-special byte between '&' and '='
                if eq_pos > 0 {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// Returns `true` when `value` contains a `%XX` percent-encoded sequence.
fn contains_url_encoding(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            return true;
        }
        i += 1;
    }
    false
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Argument, Command, Flag};
    use crate::parser::Parser;

    // ── Path traversal ────────────────────────────────────────────────────────

    #[test]
    fn path_traversal_forward_slash_prefix() {
        let v = InputValidator::new().check_path_traversal();
        assert!(v.validate_value("f", "/etc/passwd").is_err());
    }

    #[test]
    fn path_traversal_tilde_prefix() {
        let v = InputValidator::new().check_path_traversal();
        assert!(v.validate_value("f", "~/.ssh/id_rsa").is_err());
    }

    #[test]
    fn path_traversal_dotdot_unix() {
        let v = InputValidator::new().check_path_traversal();
        assert!(v.validate_value("f", "../../secret").is_err());
    }

    #[test]
    fn path_traversal_dotdot_windows() {
        let v = InputValidator::new().check_path_traversal();
        assert!(v.validate_value("f", "..\\windows\\system32").is_err());
    }

    #[test]
    fn path_traversal_safe_relative_path() {
        let v = InputValidator::new().check_path_traversal();
        assert!(v.validate_value("f", "subdir/file.txt").is_ok());
    }

    #[test]
    fn path_traversal_safe_filename() {
        let v = InputValidator::new().check_path_traversal();
        assert!(v.validate_value("f", "README.md").is_ok());
    }

    #[test]
    fn path_traversal_disabled_does_not_flag() {
        let v = InputValidator::new(); // traversal check off
        assert!(v.validate_value("f", "/etc/passwd").is_ok());
    }

    // ── Control characters ────────────────────────────────────────────────────

    #[test]
    fn control_char_null_byte() {
        let v = InputValidator::new().check_control_chars();
        assert!(v.validate_value("f", "hello\x00world").is_err());
    }

    #[test]
    fn control_char_carriage_return() {
        let v = InputValidator::new().check_control_chars();
        assert!(v.validate_value("f", "hello\rworld").is_err());
    }

    #[test]
    fn control_char_delete() {
        let v = InputValidator::new().check_control_chars();
        assert!(v.validate_value("f", "hello\x7fworld").is_err());
    }

    #[test]
    fn control_char_tab_is_allowed() {
        let v = InputValidator::new().check_control_chars();
        assert!(v.validate_value("f", "hello\tworld").is_ok());
    }

    #[test]
    fn control_char_newline_is_allowed() {
        let v = InputValidator::new().check_control_chars();
        assert!(v.validate_value("f", "hello\nworld").is_ok());
    }

    #[test]
    fn control_char_safe_value() {
        let v = InputValidator::new().check_control_chars();
        assert!(v.validate_value("f", "ordinary text 123").is_ok());
    }

    #[test]
    fn control_char_disabled_does_not_flag() {
        let v = InputValidator::new(); // control char check off
        assert!(v.validate_value("f", "hello\x00world").is_ok());
    }

    // ── Query injection ───────────────────────────────────────────────────────

    #[test]
    fn query_injection_question_mark() {
        let v = InputValidator::new().check_query_injection();
        assert!(v.validate_value("url", "example.com?admin=1").is_err());
    }

    #[test]
    fn query_injection_ampersand_key_val() {
        let v = InputValidator::new().check_query_injection();
        assert!(v.validate_value("q", "value&role=admin").is_err());
    }

    #[test]
    fn query_injection_ampersand_no_equals_safe() {
        let v = InputValidator::new().check_query_injection();
        // A lone '&' without an '=' after it is not flagged as key=val injection.
        assert!(v.validate_value("q", "Tom & Jerry").is_ok());
    }

    #[test]
    fn query_injection_safe_value() {
        let v = InputValidator::new().check_query_injection();
        assert!(v.validate_value("q", "normal search term").is_ok());
    }

    #[test]
    fn query_injection_disabled_does_not_flag() {
        let v = InputValidator::new(); // query check off
        assert!(v.validate_value("q", "example.com?admin=1").is_ok());
    }

    // ── URL encoding ──────────────────────────────────────────────────────────

    #[test]
    fn url_encoding_percent_2f() {
        let v = InputValidator::new().check_url_encoding();
        assert!(v.validate_value("f", "hello%2Fworld").is_err());
    }

    #[test]
    fn url_encoding_percent_00() {
        let v = InputValidator::new().check_url_encoding();
        assert!(v.validate_value("f", "null%00byte").is_err());
    }

    #[test]
    fn url_encoding_uppercase_hex() {
        let v = InputValidator::new().check_url_encoding();
        assert!(v.validate_value("f", "%2E%2E%2F").is_err());
    }

    #[test]
    fn url_encoding_lone_percent_is_safe() {
        let v = InputValidator::new().check_url_encoding();
        // A bare '%' not followed by two hex digits is not flagged.
        assert!(v.validate_value("f", "50% off").is_ok());
    }

    #[test]
    fn url_encoding_safe_value() {
        let v = InputValidator::new().check_url_encoding();
        assert!(v.validate_value("f", "hello world").is_ok());
    }

    #[test]
    fn url_encoding_disabled_does_not_flag() {
        let v = InputValidator::new(); // url encoding check off
        assert!(v.validate_value("f", "hello%2Fworld").is_ok());
    }

    // ── strict() helper ───────────────────────────────────────────────────────

    #[test]
    fn strict_catches_path_traversal() {
        let v = InputValidator::strict();
        let err = v.validate_value("f", "../etc").unwrap_err();
        assert!(matches!(err, ValidationError::PathTraversal { .. }));
    }

    #[test]
    fn strict_catches_control_char() {
        let v = InputValidator::strict();
        let err = v.validate_value("f", "a\x01b").unwrap_err();
        assert!(matches!(err, ValidationError::ControlCharacter { .. }));
    }

    #[test]
    fn strict_catches_query_injection() {
        let v = InputValidator::strict();
        let err = v.validate_value("f", "x?y=z").unwrap_err();
        assert!(matches!(err, ValidationError::QueryInjection { .. }));
    }

    #[test]
    fn strict_catches_url_encoding() {
        let v = InputValidator::strict();
        let err = v.validate_value("f", "%41").unwrap_err();
        assert!(matches!(err, ValidationError::UrlEncoding { .. }));
    }

    #[test]
    fn strict_safe_value_passes() {
        let v = InputValidator::strict();
        assert!(v.validate_value("f", "hello world").is_ok());
    }

    // ── validate_parsed ───────────────────────────────────────────────────────

    #[test]
    fn validate_parsed_clean_args_pass() {
        let cmd = Command::builder("get")
            .argument(Argument::builder("id").required().build().unwrap())
            .build()
            .unwrap();
        let cmds = vec![cmd];
        let parser = Parser::new(&cmds);
        let parsed = parser.parse(&["get", "42"]).unwrap();

        let v = InputValidator::strict();
        assert!(v.validate_parsed(&parsed).is_ok());
    }

    #[test]
    fn validate_parsed_bad_arg_fails() {
        let cmd = Command::builder("get")
            .argument(Argument::builder("id").required().build().unwrap())
            .build()
            .unwrap();
        let cmds = vec![cmd];
        let parser = Parser::new(&cmds);
        let parsed = parser.parse(&["get", "../secret"]).unwrap();

        let v = InputValidator::new().check_path_traversal();
        assert!(v.validate_parsed(&parsed).is_err());
    }

    #[test]
    fn validate_parsed_bad_flag_fails() {
        let cmd = Command::builder("deploy")
            .flag(
                Flag::builder("env")
                    .takes_value()
                    .required()
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        let cmds = vec![cmd];
        let parser = Parser::new(&cmds);
        let parsed = parser.parse(&["deploy", "--env", "prod?debug=1"]).unwrap();

        let v = InputValidator::new().check_query_injection();
        assert!(v.validate_parsed(&parsed).is_err());
    }

    // ── Middleware impl ───────────────────────────────────────────────────────

    #[test]
    fn middleware_before_dispatch_ok_for_clean_input() {
        let cmd = Command::builder("ping").build().unwrap();
        let cmds = vec![cmd];
        let parsed = Parser::new(&cmds).parse(&["ping"]).unwrap();

        let v = InputValidator::strict();
        assert!(v.before_dispatch(&parsed).is_ok());
    }

    #[test]
    fn middleware_before_dispatch_err_for_bad_input() {
        let cmd = Command::builder("get")
            .argument(Argument::builder("path").required().build().unwrap())
            .build()
            .unwrap();
        let cmds = vec![cmd];
        let parsed = Parser::new(&cmds).parse(&["get", "/etc/passwd"]).unwrap();

        let v = InputValidator::new().check_path_traversal();
        let result = v.before_dispatch(&parsed);
        assert!(result.is_err());
    }

    // ── Error messages ────────────────────────────────────────────────────────

    #[test]
    fn error_display_path_traversal() {
        let err = ValidationError::PathTraversal {
            field: "file".to_owned(),
            value: "../secret".to_owned(),
        };
        let msg = err.to_string();
        assert!(msg.contains("file"));
        assert!(msg.contains("../secret"));
    }

    #[test]
    fn error_display_control_character() {
        let err = ValidationError::ControlCharacter {
            field: "name".to_owned(),
            value: "a\x00b".to_owned(),
        };
        let msg = err.to_string();
        assert!(msg.contains("name"));
    }

    #[test]
    fn error_display_query_injection() {
        let err = ValidationError::QueryInjection {
            field: "q".to_owned(),
            value: "x?y=1".to_owned(),
        };
        let msg = err.to_string();
        assert!(msg.contains("q"));
    }

    #[test]
    fn error_display_url_encoding() {
        let err = ValidationError::UrlEncoding {
            field: "val".to_owned(),
            value: "%2F".to_owned(),
        };
        let msg = err.to_string();
        assert!(msg.contains("val"));
        assert!(msg.contains("%2F"));
    }
}
