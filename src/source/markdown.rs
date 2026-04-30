//! [`CommandSource`] backed by a directory of Markdown files with YAML
//! frontmatter.
//!
//! Each `*.md` file in the directory describes one command. The frontmatter
//! block (`---` … `---`) carries metadata; the markdown body becomes the
//! command's `description`.
//!
//! # File format
//!
//! ```text
//! ---
//! name: deploy
//! aliases:
//!   - d
//!   - ship
//! summary: Deploy the application
//! priority: 10
//! overrides: deploy
//! mutating: true
//! semantic_aliases:
//!   - release to production
//! best_practices:
//!   - Always run with --dry-run first
//! anti_patterns:
//!   - Deploy on Fridays
//! extra:
//!   category: "infrastructure"
//!   min_role: "ops"
//! ---
//!
//! Deploy the application to a specified environment.
//!
//! Long-form description follows here in Markdown.
//! ```
//!
//! # Supported frontmatter keys
//!
//! Scalar:
//! - `name` (required) — canonical name
//! - `summary` — one-line summary
//! - `mutating` — bool; `true` marks the command as state-mutating
//! - `priority` — integer used as the [`crate::source::LoadedCommand::priority`] hint
//! - `overrides` — canonical name of a lower-layer command this entry shadows
//! - `layer` — `embedded` | `user` | `project` | `local` (overrides the
//!   directory's default layer for this single file)
//!
//! List of strings:
//! - `aliases`, `spellings`, `semantic_aliases`, `best_practices`, `anti_patterns`
//!
//! Map of scalar values:
//! - `extra` — flat string→scalar map, copied into [`crate::Command::extra`] as
//!   JSON. Quoted strings are treated as strings; bare `true` / `false` /
//!   integers are decoded into typed JSON values.
//!
//! # Body sections
//!
//! After the frontmatter, the markdown body is split into a free-text prelude
//! (used as the command's `description`) and three optional structured
//! sections recognised by their `## ` headings:
//!
//! ```text
//! ## Arguments
//!
//! - `env` (required): Target environment
//! - `service`: Specific service to deploy
//! - `paths` (variadic): Files to operate on
//!
//! ## Flags
//!
//! - `--port, -p` <NUM> (default: 8080): Listen port
//! - `--host` <HOST> (env: SERVE_HOST): Bind address
//! - `--verbose, -v` (repeatable): Increase verbosity
//! - `--format` <FMT> (choices: json|yaml|text): Output format
//!
//! ## Examples
//!
//! - Basic deploy: `myapp deploy production`
//! - `myapp deploy --dry-run`
//! ```
//!
//! Heading matching is case-insensitive. Unrecognised `##` headings are kept
//! as prose inside whichever section they follow (so `## Notes` does not
//! break parsing). Per-bullet parse failures emit `SchemaWarning`s and skip
//! the bullet, never aborting the file load.
//!
//! Handlers cannot be expressed in markdown — attach them by post-processing
//! the loaded `Command` if you need runtime behaviour.
//!
//! Unknown frontmatter keys produce a
//! [`crate::source::LoadDiagnostic::SchemaWarning`] but do not block loading.

use std::fs;
use std::path::{Path, PathBuf};

use crate::model::{Argument, Command, Example, Flag};

use super::{CommandSource, Layer, LoadDiagnostic, LoadedCommand, SourceLoad, SourceOrigin};

/// Loads commands from `*.md` files in a single directory.
///
/// Subdirectories are not traversed — keep the layout flat in v1.
///
/// Files whose canonical name would be empty (after parsing) are skipped with
/// a [`LoadDiagnostic::SchemaWarning`].
pub struct MarkdownDirSource {
    name: String,
    dir: PathBuf,
    layer: Layer,
    /// If `true`, a missing directory produces a `SchemaWarning` instead of a
    /// `SourceError`. Useful for optional layers (e.g. a user override
    /// directory that may legitimately not exist).
    optional: bool,
}

impl MarkdownDirSource {
    /// Construct a source that reads from `dir` and tags loaded commands at
    /// the given `layer`.
    pub fn new(name: impl Into<String>, dir: impl Into<PathBuf>, layer: Layer) -> Self {
        Self {
            name: name.into(),
            dir: dir.into(),
            layer,
            optional: false,
        }
    }

    /// Mark the source as optional — a missing directory is reported as a
    /// schema warning rather than a hard error.
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Convenience: build a [`Layer::User`] source rooted at the platform's
    /// user-config directory for `app_name`, marked [`Self::optional`].
    ///
    /// Resolution order (delegated to [`user_config_dir`]):
    /// 1. `$XDG_CONFIG_HOME/<app_name>/commands` if `XDG_CONFIG_HOME` is set
    ///    and non-empty (per the XDG Base Directory Specification — an empty
    ///    `XDG_CONFIG_HOME` falls through to `HOME`).
    /// 2. `$HOME/.config/<app_name>/commands` on Unix.
    /// 3. `%APPDATA%\<app_name>\commands` on Windows.
    ///
    /// Returns `None` if no home directory can be resolved (e.g. neither
    /// `HOME` nor `USERPROFILE` are set — common in stripped-environment
    /// contexts like `sudo -E` or systemd services without `User=`). Use
    /// [`user_config_dir`] directly if you need to inspect the resolved path
    /// before constructing the source.
    ///
    /// `app_name` should be a single path component (no `/`, `\`, or `..`);
    /// path-separator characters in `app_name` produce a surprising path.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use argot_cmd::source::{LayeredBuilder, markdown::MarkdownDirSource};
    ///
    /// let builder = LayeredBuilder::new();
    /// let builder = match MarkdownDirSource::user_config("myapp") {
    ///     Some(src) => builder.add(src),
    ///     None => builder, // home directory unresolvable; skip the user layer
    /// };
    /// ```
    pub fn user_config(app_name: impl AsRef<str>) -> Option<Self> {
        let dir = user_config_dir(app_name.as_ref())?;
        Some(Self::new("user-config", dir, Layer::User).optional())
    }

    /// Convenience: build a [`Layer::Project`] source rooted at the nearest
    /// `.<app_name>/commands/` directory walking up from the current working
    /// directory, marked [`Self::optional`].
    ///
    /// Returns `None` when:
    /// - no `.<app_name>/commands/` directory exists between `cwd` and the
    ///   filesystem root, or
    /// - `std::env::current_dir()` fails (the cwd was deleted, permission was
    ///   revoked, etc.).
    ///
    /// These two failure modes are flattened into a single `None`. Use
    /// [`find_project_dir`] with an explicit `start: &Path` if you need to
    /// distinguish them, or to start the search from a path other than the
    /// current directory.
    ///
    /// Non-directory entries at the marker location (a regular file named
    /// `.<app_name>/commands` or a symlink to a non-directory) do not satisfy
    /// the lookup; the walk continues to the next ancestor.
    ///
    /// `app_name` should be a single path component (no `/`, `\`, or `..`).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use argot_cmd::source::{LayeredBuilder, markdown::MarkdownDirSource};
    ///
    /// let builder = LayeredBuilder::new();
    /// let builder = match MarkdownDirSource::project_root("myapp") {
    ///     Some(src) => builder.add(src),
    ///     None => builder, // not in a project tree, or cwd unavailable
    /// };
    /// ```
    pub fn project_root(app_name: impl AsRef<str>) -> Option<Self> {
        let cwd = std::env::current_dir().ok()?;
        let dir = find_project_dir(app_name.as_ref(), &cwd)?;
        Some(Self::new("project-root", dir, Layer::Project).optional())
    }
}

/// Resolve the user-config commands directory for `app_name`.
///
/// See [`MarkdownDirSource::user_config`] for the resolution order. This free
/// function exposes the same resolution without constructing a source so the
/// caller can log the path, fall back to a different layer, or compose with
/// other directories.
///
/// An empty `XDG_CONFIG_HOME` is treated as unset (per the XDG Base Directory
/// Specification) and falls through to the next resolution step.
///
/// `app_name` should be a single path component (no `/`, `\`, or `..`);
/// it is concatenated as-is. Pass a sanitized identifier.
///
/// # Examples
///
/// ```no_run
/// use argot_cmd::source::markdown::user_config_dir;
///
/// match user_config_dir("myapp") {
///     Some(path) => eprintln!("user config dir: {}", path.display()),
///     None => eprintln!("no user-config path resolvable"),
/// }
/// ```
pub fn user_config_dir(app_name: &str) -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        let p = PathBuf::from(xdg);
        if !p.as_os_str().is_empty() {
            return Some(p.join(app_name).join("commands"));
        }
    }
    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let p = PathBuf::from(appdata);
            if !p.as_os_str().is_empty() {
                return Some(p.join(app_name).join("commands"));
            }
        }
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let p = PathBuf::from(home);
    if p.as_os_str().is_empty() {
        return None;
    }
    Some(p.join(".config").join(app_name).join("commands"))
}

/// Walk up from `start` looking for a `.<app_name>/commands` directory.
///
/// Returns the first match found between `start` and the filesystem root,
/// or `None` if no such directory exists.
///
/// Match semantics: the candidate must be a directory (or a symlink that
/// resolves to a directory). The following are silently treated as
/// non-matches and the walk continues to the next ancestor:
/// - a regular file at the marker location,
/// - a broken symlink,
/// - a symlink to a non-directory,
/// - a path that exists but cannot be stat'd (e.g. permission denied).
///
/// `app_name` should be a single path component (no separators).
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use argot_cmd::source::markdown::find_project_dir;
///
/// if let Some(dir) = find_project_dir("myapp", Path::new("/path/to/checkout/sub/dir")) {
///     eprintln!("found project commands at: {}", dir.display());
/// }
/// ```
pub fn find_project_dir(app_name: &str, start: &Path) -> Option<PathBuf> {
    let marker_parent = format!(".{}", app_name);
    let mut cur: Option<&Path> = Some(start);
    while let Some(dir) = cur {
        let candidate = dir.join(&marker_parent).join("commands");
        if candidate.is_dir() {
            return Some(candidate);
        }
        cur = dir.parent();
    }
    None
}

/// Body of a markdown command file split into its free-text prelude and
/// recognised structured sections.
#[derive(Default)]
struct BodySections {
    /// Lines preceding the first recognised `##` section. Becomes the
    /// command's `description`.
    prelude: String,
    /// Raw lines under `## Arguments`.
    arguments: Vec<String>,
    /// Raw lines under `## Flags`.
    flags: Vec<String>,
    /// Raw lines under `## Examples`.
    examples: Vec<String>,
}

/// Split a markdown body into prelude + recognised `##` sections.
///
/// Section headings are matched case-insensitively against `Arguments` /
/// `Flags` / `Examples`. An unrecognised `## Heading` switches the parser
/// back to "prelude mode" — its content is appended to the prelude (which
/// becomes the command's `description`) so user-authored prose like
/// `## Notes` after the structured sections is preserved as documentation
/// for the agent reader, not silently mis-routed into Examples.
fn split_body_sections(body: &str) -> BodySections {
    enum Mode {
        Prelude,
        Arguments,
        Flags,
        Examples,
    }
    let mut out = BodySections::default();
    let mut mode = Mode::Prelude;

    for raw_line in body.lines() {
        // Detect `## <heading>` (one or more spaces after `##`).
        if let Some(rest) = raw_line.strip_prefix("## ") {
            let heading = rest.trim().to_ascii_lowercase();
            match heading.as_str() {
                "arguments" => {
                    mode = Mode::Arguments;
                    continue;
                }
                "flags" => {
                    mode = Mode::Flags;
                    continue;
                }
                "examples" => {
                    mode = Mode::Examples;
                    continue;
                }
                _ => {
                    // Unrecognised heading: route subsequent content back
                    // into the prelude so it survives as part of
                    // `description` — and re-emit the heading itself so the
                    // resulting markdown is still well-formed.
                    mode = Mode::Prelude;
                    out.prelude.push_str(raw_line);
                    out.prelude.push('\n');
                    continue;
                }
            }
        }
        match mode {
            Mode::Prelude => {
                out.prelude.push_str(raw_line);
                out.prelude.push('\n');
            }
            Mode::Arguments => out.arguments.push(raw_line.to_string()),
            Mode::Flags => out.flags.push(raw_line.to_string()),
            Mode::Examples => out.examples.push(raw_line.to_string()),
        }
    }
    out
}

/// Strip a single leading `- ` or `* ` bullet marker. Returns `None` if the
/// line is blank, a non-bullet, or just a bullet with no content.
fn strip_bullet(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if t.is_empty() {
        return None;
    }
    let rest = t.strip_prefix("- ").or_else(|| t.strip_prefix("* "))?;
    let rest = rest.trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

/// Extract a backtick-delimited token at the start of `s`. Returns the token
/// and the remainder (with leading whitespace consumed). `None` if `s` does
/// not start with a backtick or has no closing backtick.
fn take_backticks(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    let after_open = s.strip_prefix('`')?;
    let close = after_open.find('`')?;
    let token = &after_open[..close];
    let rest = after_open[close + 1..].trim_start();
    Some((token, rest))
}

/// Outcome of `take_modifiers` — distinguishes the three states callers care
/// about so unterminated `(` doesn't silently disappear.
enum ModifierParse<'a> {
    /// No leading `(`. The entire input is unconsumed remainder.
    Absent { rest: &'a str },
    /// Well-formed `(modifier[, ...])`. `rest` is the trimmed remainder.
    Parsed { mods: Vec<&'a str>, rest: &'a str },
    /// Leading `(` with no matching `)`. The unterminated text (everything
    /// after the unmatched `(`) is captured in `unterminated` so the caller
    /// can include it in a diagnostic. There is no `rest` field by design:
    /// once we know the modifier list is malformed, we discard the tail to
    /// stop the caller from re-parsing it as a description and producing a
    /// second misleading warning.
    Unterminated { unterminated: &'a str },
}

/// Parse `(modifier[, modifier]...)` if present at the start of `s`.
fn take_modifiers(s: &str) -> ModifierParse<'_> {
    let s = s.trim_start();
    let Some(after_open) = s.strip_prefix('(') else {
        return ModifierParse::Absent { rest: s };
    };
    let Some(close) = after_open.find(')') else {
        return ModifierParse::Unterminated {
            unterminated: after_open,
        };
    };
    let inside = &after_open[..close];
    let rest = after_open[close + 1..].trim_start();
    let mods = inside
        .split(',')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .collect();
    ModifierParse::Parsed { mods, rest }
}

/// Parse a `## Arguments` section.
///
/// Bullet grammar: `` - `name` [(modifier[, modifier]...)]?: description ``
/// Modifiers: `required`, `variadic`, `default: <value>`.
fn parse_arguments_section(
    lines: &[String],
    origin: &SourceOrigin,
    warnings: &mut Vec<LoadDiagnostic>,
) -> Vec<Argument> {
    let mut out = Vec::new();
    for raw in lines {
        let Some(content) = strip_bullet(raw) else {
            continue;
        };
        let Some((name, rest)) = take_backticks(content) else {
            warnings.push(LoadDiagnostic::SchemaWarning {
                origin: origin.clone(),
                field: "Arguments".into(),
                message: format!("expected `name` in backticks, got {:?}", raw),
            });
            continue;
        };
        if name.is_empty() {
            warnings.push(LoadDiagnostic::SchemaWarning {
                origin: origin.clone(),
                field: "Arguments".into(),
                message: format!("empty argument name in {:?}", raw),
            });
            continue;
        }
        let (mods, after_mods) = match take_modifiers(rest) {
            ModifierParse::Absent { rest } => (Vec::new(), rest),
            ModifierParse::Parsed { mods, rest } => (mods, rest),
            ModifierParse::Unterminated { unterminated } => {
                warnings.push(LoadDiagnostic::SchemaWarning {
                    origin: origin.clone(),
                    field: "Arguments".into(),
                    message: format!(
                        "argument `{}` has unterminated `(` in modifier list — modifiers ignored: {:?}",
                        name, unterminated
                    ),
                });
                (Vec::new(), "")
            }
        };
        let description = after_mods.strip_prefix(':').unwrap_or(after_mods).trim();

        let mut builder = Argument::builder(name);
        if !description.is_empty() {
            builder = builder.description(description);
        }
        for m in mods {
            if let Some(rest) = m.strip_prefix("default:") {
                builder = builder.default_value(rest.trim());
            } else {
                match m {
                    "required" => builder = builder.required(),
                    "variadic" => builder = builder.variadic(),
                    "optional" => {} // explicit no-op (the default)
                    other => warnings.push(LoadDiagnostic::SchemaWarning {
                        origin: origin.clone(),
                        field: "Arguments".into(),
                        message: format!("unknown argument modifier '{}' on `{}`", other, name),
                    }),
                }
            }
        }
        match builder.build() {
            Ok(a) => out.push(a),
            Err(e) => warnings.push(LoadDiagnostic::SchemaWarning {
                origin: origin.clone(),
                field: "Arguments".into(),
                message: format!("argument `{}` build failed: {}", name, e),
            }),
        }
    }
    out
}

/// Parse a `## Flags` section.
///
/// Bullet grammar:
/// `` - `--long[, -short]` [<VALUE>]? [(modifier[, modifier]...)]?: description ``
/// Modifiers: `required`, `repeatable`, `default: <value>`,
/// `env: <VAR_NAME>`, `choices: a|b|c`.
/// The presence of `<VALUE>` (any token in `<...>`) marks the flag as
/// taking a value.
fn parse_flags_section(
    lines: &[String],
    origin: &SourceOrigin,
    warnings: &mut Vec<LoadDiagnostic>,
) -> Vec<Flag> {
    let mut out = Vec::new();
    for raw in lines {
        let Some(content) = strip_bullet(raw) else {
            continue;
        };
        let Some((token, rest)) = take_backticks(content) else {
            warnings.push(LoadDiagnostic::SchemaWarning {
                origin: origin.clone(),
                field: "Flags".into(),
                message: format!("expected `--flag` in backticks, got {:?}", raw),
            });
            continue;
        };

        // Parse `--long[, -short]` from inside the backticks.
        let parts: Vec<&str> = token.split(',').map(str::trim).collect();
        let Some(long) = parts.iter().find_map(|p| p.strip_prefix("--")) else {
            warnings.push(LoadDiagnostic::SchemaWarning {
                origin: origin.clone(),
                field: "Flags".into(),
                message: format!("flag `{}` missing --long form", token),
            });
            continue;
        };
        // Short alias must be exactly one character after the `-` prefix.
        // A multi-char form like `-vv` is structurally rejected — but we warn
        // when we see one so the user isn't left wondering why their short
        // flag silently disappeared from the loaded Command.
        let mut short: Option<char> = None;
        for p in parts.iter() {
            let Some(after_dash) = p.strip_prefix('-') else {
                continue;
            };
            if after_dash.starts_with('-') {
                // This is the long form (`--name`) — skip without comment.
                continue;
            }
            if after_dash.chars().count() == 1 {
                short = after_dash.chars().next();
                break;
            }
            // Single `-` prefix but multi-char body: structurally invalid as
            // a short alias.
            warnings.push(LoadDiagnostic::SchemaWarning {
                origin: origin.clone(),
                field: "Flags".into(),
                message: format!(
                    "flag `{}` short alias `{}` must be a single character — short ignored",
                    token, p
                ),
            });
        }

        // Optional `<VALUE>` after the backticks marks takes_value. If the
        // closing `>` is missing, the author's intent is unambiguous (they
        // typed `<` to declare a value placeholder), so we honour
        // `takes_value = true` and advance `after_token` to whichever stop
        // point still lets us pick up modifiers — preferring `(` (modifier
        // list start) and falling back to whitespace. Without that advance,
        // the rest of the bullet would be silently mis-parsed as a giant
        // description blob.
        let mut after_token = rest;
        let takes_value = if let Some(rest_after_v) = after_token.strip_prefix('<') {
            if let Some(close) = rest_after_v.find('>') {
                after_token = rest_after_v[close + 1..].trim_start();
                true
            } else {
                let stop = rest_after_v
                    .find(['(', ':'])
                    .or_else(|| rest_after_v.find(char::is_whitespace))
                    .unwrap_or(rest_after_v.len());
                after_token = rest_after_v[stop..].trim_start();
                warnings.push(LoadDiagnostic::SchemaWarning {
                    origin: origin.clone(),
                    field: "Flags".into(),
                    message: format!(
                        "flag `{}` has unterminated `<VALUE>` — assuming takes_value=true and skipping to next modifier or description",
                        token
                    ),
                });
                true
            }
        } else {
            false
        };

        let (mods, after_mods) = match take_modifiers(after_token) {
            ModifierParse::Absent { rest } => (Vec::new(), rest),
            ModifierParse::Parsed { mods, rest } => (mods, rest),
            ModifierParse::Unterminated { unterminated } => {
                warnings.push(LoadDiagnostic::SchemaWarning {
                    origin: origin.clone(),
                    field: "Flags".into(),
                    message: format!(
                        "flag `{}` has unterminated `(` in modifier list — modifiers ignored: {:?}",
                        token, unterminated
                    ),
                });
                (Vec::new(), "")
            }
        };
        let description = after_mods.strip_prefix(':').unwrap_or(after_mods).trim();

        let mut builder = Flag::builder(long);
        if let Some(c) = short {
            builder = builder.short(c);
        }
        if takes_value {
            builder = builder.takes_value();
        }
        if !description.is_empty() {
            builder = builder.description(description);
        }
        for m in mods {
            if let Some(rest) = m.strip_prefix("default:") {
                builder = builder.default_value(rest.trim());
            } else if let Some(rest) = m.strip_prefix("env:") {
                builder = builder.env(rest.trim());
            } else if let Some(rest) = m.strip_prefix("choices:") {
                let raw_choices: Vec<&str> = rest.split('|').map(str::trim).collect();
                let dropped_empty = raw_choices.iter().any(|c| c.is_empty());
                let choices: Vec<&str> =
                    raw_choices.into_iter().filter(|c| !c.is_empty()).collect();
                if dropped_empty {
                    warnings.push(LoadDiagnostic::SchemaWarning {
                        origin: origin.clone(),
                        field: "Flags".into(),
                        message: format!(
                            "flag `{}` choices list contained empty entries (dropped) — check for stray '|'",
                            token
                        ),
                    });
                }
                if choices.is_empty() {
                    warnings.push(LoadDiagnostic::SchemaWarning {
                        origin: origin.clone(),
                        field: "Flags".into(),
                        message: format!(
                            "flag `{}` has empty `choices:` modifier — choices ignored",
                            token
                        ),
                    });
                } else {
                    builder = builder.choices(choices);
                }
            } else {
                match m {
                    "required" => builder = builder.required(),
                    "repeatable" => builder = builder.repeatable(),
                    other => warnings.push(LoadDiagnostic::SchemaWarning {
                        origin: origin.clone(),
                        field: "Flags".into(),
                        message: format!("unknown flag modifier '{}' on `{}`", other, token),
                    }),
                }
            }
        }
        match builder.build() {
            Ok(f) => out.push(f),
            Err(e) => warnings.push(LoadDiagnostic::SchemaWarning {
                origin: origin.clone(),
                field: "Flags".into(),
                message: format!("flag `{}` build failed: {}", token, e),
            }),
        }
    }
    out
}

/// Parse a `## Examples` section.
///
/// Bullet grammar (either form):
/// - `` - title: `command` ``
/// - `` - `command` ``  (uses the command itself as the title)
///
/// Title detection: the title is everything before the **last** `:` that is
/// followed (after whitespace) by an opening backtick. This handles titles
/// that themselves contain colons (e.g. URLs, namespaces) without splitting
/// at the wrong place. If no `: \`` boundary is found, the whole bullet is
/// treated as a backticked command with the command-text as the title.
fn parse_examples_section(
    lines: &[String],
    origin: &SourceOrigin,
    warnings: &mut Vec<LoadDiagnostic>,
) -> Vec<Example> {
    let mut out = Vec::new();
    for raw in lines {
        let Some(content) = strip_bullet(raw) else {
            continue;
        };

        let (title, command_part) = split_example_title(content);

        let command = match take_backticks(command_part) {
            Some((cmd, _)) if !cmd.is_empty() => cmd.to_string(),
            _ => {
                warnings.push(LoadDiagnostic::SchemaWarning {
                    origin: origin.clone(),
                    field: "Examples".into(),
                    message: format!(
                        "expected `command` in backticks (optionally preceded by `title: `), got {:?}",
                        raw
                    ),
                });
                continue;
            }
        };
        let title = title.unwrap_or_else(|| command.clone());
        out.push(Example::new(title, command));
    }
    out
}

/// Locate the `: \`` boundary that separates an example's title from its
/// command. Scans for the right-most `:` that is followed (after optional
/// whitespace) by a backtick — so `Note: see https://x.com: \`cmd\`` splits
/// at the second colon, not the first.
///
/// Returns `(title, command_part)`. Title is `None` when the bullet has no
/// `: \`` boundary; in that case `command_part` is the whole content (which
/// is expected to start with a backtick).
fn split_example_title(content: &str) -> (Option<String>, &str) {
    if content.starts_with('`') {
        return (None, content);
    }
    let bytes = content.as_bytes();
    // Walk from the end so the last `:` that meets the rule wins.
    for (i, &b) in bytes.iter().enumerate().rev() {
        if b == b':' {
            let after = content[i + 1..].trim_start();
            if after.starts_with('`') {
                let title = content[..i].trim().to_string();
                let command_part = content[i + 1..].trim_start();
                return (Some(title), command_part);
            }
        }
    }
    // No `: \`` boundary; let the backtick parser fail with its own diagnostic.
    (None, content)
}

impl CommandSource for MarkdownDirSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn load(&self) -> SourceLoad {
        let mut commands: Vec<LoadedCommand> = Vec::new();
        let mut diagnostics: Vec<LoadDiagnostic> = Vec::new();

        let entries = match fs::read_dir(&self.dir) {
            Ok(it) => it,
            Err(e) => {
                let msg = format!("read_dir failed: {}", e);
                if self.optional && e.kind() == std::io::ErrorKind::NotFound {
                    diagnostics.push(LoadDiagnostic::SchemaWarning {
                        origin: SourceOrigin {
                            source: self.name.clone(),
                            layer: self.layer,
                            path: Some(self.dir.display().to_string()),
                        },
                        field: "<directory>".into(),
                        message: format!("optional directory absent: {}", e),
                    });
                } else {
                    diagnostics.push(LoadDiagnostic::SourceError {
                        source: self.name.clone(),
                        path: Some(self.dir.display().to_string()),
                        message: msg,
                    });
                }
                return SourceLoad {
                    commands,
                    diagnostics,
                };
            }
        };

        // Collect + sort entries for deterministic load order.
        // Per-entry IO errors (permission denied on a single file, stale NFS
        // handle, etc.) are surfaced as SourceError diagnostics rather than
        // silently skipped — the caller needs to know a file they expect to
        // see is missing from the load.
        let mut paths: Vec<PathBuf> = Vec::new();
        for entry in entries {
            match entry {
                Ok(e) => {
                    let path = e.path();
                    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("md") {
                        paths.push(path);
                    }
                }
                Err(e) => diagnostics.push(LoadDiagnostic::SourceError {
                    source: self.name.clone(),
                    path: Some(self.dir.display().to_string()),
                    message: format!("read_dir entry failed: {}", e),
                }),
            }
        }
        paths.sort();

        for path in paths {
            match load_one(&path, &self.name, self.layer) {
                Ok((Some(loaded), warnings)) => {
                    diagnostics.extend(warnings);
                    commands.push(loaded);
                }
                Ok((None, warnings)) => {
                    diagnostics.extend(warnings);
                }
                Err(msg) => diagnostics.push(LoadDiagnostic::SourceError {
                    source: self.name.clone(),
                    path: Some(path.display().to_string()),
                    message: msg,
                }),
            }
        }

        SourceLoad {
            commands,
            diagnostics,
        }
    }
}

fn load_one(
    path: &Path,
    source_name: &str,
    default_layer: Layer,
) -> Result<(Option<LoadedCommand>, Vec<LoadDiagnostic>), String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("read failed: {}", e))?;
    let (fm_text, body) = split_frontmatter(&raw).ok_or_else(|| {
        "missing or unterminated frontmatter (expected leading --- ... ---)".to_string()
    })?;

    let mut warnings: Vec<LoadDiagnostic> = Vec::new();
    let origin_for_warnings = SourceOrigin {
        source: source_name.to_string(),
        layer: default_layer,
        path: Some(path.display().to_string()),
    };

    let fm = parse_frontmatter(fm_text, &origin_for_warnings, &mut warnings);

    // name is required.
    let name = match fm.name {
        Some(n) if !n.trim().is_empty() => n,
        _ => {
            warnings.push(LoadDiagnostic::SchemaWarning {
                origin: origin_for_warnings,
                field: "name".into(),
                message: "missing or empty 'name' — file skipped".into(),
            });
            return Ok((None, warnings));
        }
    };

    // Decide layer: explicit frontmatter override or default.
    let layer = match fm.layer.as_deref() {
        None => default_layer,
        Some("embedded") => Layer::Embedded,
        Some("user") => Layer::User,
        Some("project") => Layer::Project,
        Some("local") => Layer::Local,
        Some(other) => {
            warnings.push(LoadDiagnostic::SchemaWarning {
                origin: SourceOrigin {
                    source: source_name.to_string(),
                    layer: default_layer,
                    path: Some(path.display().to_string()),
                },
                field: "layer".into(),
                message: format!(
                    "unknown layer '{}', falling back to source default '{}'",
                    other,
                    default_layer.label()
                ),
            });
            default_layer
        }
    };

    let origin = SourceOrigin {
        source: source_name.to_string(),
        layer,
        path: Some(path.display().to_string()),
    };

    let mut builder = Command::builder(&name);
    if let Some(s) = fm.summary {
        builder = builder.summary(s);
    }

    // Split the body into a free-text prelude (used as the description) and
    // structured `## Arguments` / `## Flags` / `## Examples` sections. Per-line
    // parse failures emit warnings but do not abort.
    let body_parts = split_body_sections(body);
    let description = body_parts.prelude.trim();
    if !description.is_empty() {
        builder = builder.description(description);
    }
    let origin_for_sections = SourceOrigin {
        source: source_name.to_string(),
        layer,
        path: Some(path.display().to_string()),
    };
    for arg in parse_arguments_section(&body_parts.arguments, &origin_for_sections, &mut warnings) {
        builder = builder.argument(arg);
    }
    for flag in parse_flags_section(&body_parts.flags, &origin_for_sections, &mut warnings) {
        builder = builder.flag(flag);
    }
    for ex in parse_examples_section(&body_parts.examples, &origin_for_sections, &mut warnings) {
        builder = builder.example(ex);
    }

    for a in fm.aliases {
        builder = builder.alias(a);
    }
    for s in fm.spellings {
        builder = builder.spelling(s);
    }
    for sa in fm.semantic_aliases {
        builder = builder.semantic_alias(sa);
    }
    for bp in fm.best_practices {
        builder = builder.best_practice(bp);
    }
    for ap in fm.anti_patterns {
        builder = builder.anti_pattern(ap);
    }
    for (k, v) in fm.extra {
        builder = builder.meta(k, v);
    }
    if fm.mutating {
        builder = builder.mutating();
    }

    let cmd = builder
        .build()
        .map_err(|e| format!("Command::build failed for canonical '{}': {}", name, e))?;

    let mut loaded = LoadedCommand::new(cmd, origin);
    if let Some(p) = fm.priority {
        loaded = loaded.with_priority(p);
    }
    if let Some(ov) = fm.overrides {
        loaded = loaded.overriding(ov);
    }

    Ok((Some(loaded), warnings))
}

/// Split a raw file into `(frontmatter_body, content_after)`. Returns `None`
/// if there is no leading `---` block or it is unterminated.
fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let trimmed = raw.trim_start_matches('\u{feff}');
    let after_first = trimmed.strip_prefix("---")?;
    let after_first = after_first
        .strip_prefix('\n')
        .or_else(|| after_first.strip_prefix("\r\n"))?;
    // Find the closing --- on its own line.
    let mut search_pos = 0;
    while search_pos < after_first.len() {
        let rest = &after_first[search_pos..];
        if let Some(idx) = rest.find("---") {
            let abs = search_pos + idx;
            // Must be at start of a line.
            let line_start = abs == 0 || &after_first[abs - 1..abs] == "\n";
            // Must be followed by EOL or EOF.
            let after_marker = abs + 3;
            let line_end = after_marker == after_first.len()
                || after_first[after_marker..].starts_with('\n')
                || after_first[after_marker..].starts_with("\r\n");
            if line_start && line_end {
                let fm = &after_first[..abs];
                let after = &after_first[after_marker..];
                let after = after
                    .strip_prefix("\r\n")
                    .or_else(|| after.strip_prefix('\n'))
                    .unwrap_or(after);
                return Some((fm, after));
            }
            search_pos = abs + 3;
        } else {
            return None;
        }
    }
    None
}

#[derive(Default)]
struct ParsedFrontmatter {
    name: Option<String>,
    summary: Option<String>,
    layer: Option<String>,
    overrides: Option<String>,
    priority: Option<i32>,
    mutating: bool,
    aliases: Vec<String>,
    spellings: Vec<String>,
    semantic_aliases: Vec<String>,
    best_practices: Vec<String>,
    anti_patterns: Vec<String>,
    extra: Vec<(String, serde_json::Value)>,
}

/// Parse the small YAML subset we accept:
/// - `key: value` scalar lines
/// - `key:` followed by indented `  - item` list lines
/// - `extra:` followed by indented `  k: v` map lines
///
/// Comments (`#` lines) and blank lines are ignored. Anything we do not
/// recognise is reported as a `SchemaWarning` but does not abort the parse.
fn parse_frontmatter(
    text: &str,
    origin: &SourceOrigin,
    warnings: &mut Vec<LoadDiagnostic>,
) -> ParsedFrontmatter {
    let mut out = ParsedFrontmatter::default();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let raw = lines[i];
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            i += 1;
            continue;
        }
        // Indented lines under no key are stray.
        if line.starts_with(' ') || line.starts_with('\t') {
            warnings.push(LoadDiagnostic::SchemaWarning {
                origin: origin.clone(),
                field: "<indent>".into(),
                message: format!("unexpected indented line: {:?}", line),
            });
            i += 1;
            continue;
        }

        let (key, rest) = match split_key(line) {
            Some(p) => p,
            None => {
                warnings.push(LoadDiagnostic::SchemaWarning {
                    origin: origin.clone(),
                    field: "<line>".into(),
                    message: format!("could not parse line: {:?}", line),
                });
                i += 1;
                continue;
            }
        };

        if rest.is_empty() {
            // Block: collect indented child lines.
            let (children, consumed) = collect_indented_block(&lines[i + 1..]);
            i += 1 + consumed;
            apply_block(&key, &children, &mut out, origin, warnings);
        } else {
            // Scalar.
            apply_scalar(&key, &rest, &mut out, origin, warnings);
            i += 1;
        }
    }

    out
}

/// Split `"key: rest"` into (key, rest_trimmed). Returns None if no `:` found.
fn split_key(line: &str) -> Option<(String, String)> {
    let idx = line.find(':')?;
    let (k, v) = line.split_at(idx);
    let k = k.trim().to_string();
    let v = v[1..].trim().to_string();
    if k.is_empty() {
        return None;
    }
    Some((k, v))
}

/// Collect all indented lines starting at `lines[0]`. Returns the child lines
/// (with their leading indentation preserved) and how many lines were
/// consumed.
fn collect_indented_block<'a>(lines: &'a [&'a str]) -> (Vec<&'a str>, usize) {
    let mut out = Vec::new();
    let mut n = 0;
    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            // blank line continues the block but is not collected
            n += 1;
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            out.push(*line);
            n += 1;
        } else {
            break;
        }
    }
    (out, n)
}

fn apply_scalar(
    key: &str,
    value: &str,
    out: &mut ParsedFrontmatter,
    origin: &SourceOrigin,
    warnings: &mut Vec<LoadDiagnostic>,
) {
    let unquoted = unquote(value);
    match key {
        "name" => out.name = Some(unquoted),
        "summary" => out.summary = Some(unquoted),
        "layer" => out.layer = Some(unquoted),
        "overrides" => out.overrides = Some(unquoted),
        "priority" => match value.parse::<i32>() {
            Ok(n) => out.priority = Some(n),
            Err(e) => warnings.push(LoadDiagnostic::SchemaWarning {
                origin: origin.clone(),
                field: "priority".into(),
                message: format!("not an integer: {}", e),
            }),
        },
        "mutating" => match value {
            "true" => out.mutating = true,
            "false" => out.mutating = false,
            other => warnings.push(LoadDiagnostic::SchemaWarning {
                origin: origin.clone(),
                field: "mutating".into(),
                message: format!("expected 'true' or 'false', got {:?}", other),
            }),
        },
        "aliases" | "spellings" | "semantic_aliases" | "best_practices" | "anti_patterns"
        | "extra" => {
            warnings.push(LoadDiagnostic::SchemaWarning {
                origin: origin.clone(),
                field: key.into(),
                message: "expected a list/map block (use 'key:' on its own line followed by indented entries)".into(),
            });
        }
        other => warnings.push(LoadDiagnostic::SchemaWarning {
            origin: origin.clone(),
            field: other.into(),
            message: "unknown frontmatter key".into(),
        }),
    }
}

fn apply_block(
    key: &str,
    children: &[&str],
    out: &mut ParsedFrontmatter,
    origin: &SourceOrigin,
    warnings: &mut Vec<LoadDiagnostic>,
) {
    match key {
        "aliases" => out
            .aliases
            .extend(parse_string_list(children, key, origin, warnings)),
        "spellings" => out
            .spellings
            .extend(parse_string_list(children, key, origin, warnings)),
        "semantic_aliases" => out
            .semantic_aliases
            .extend(parse_string_list(children, key, origin, warnings)),
        "best_practices" => out
            .best_practices
            .extend(parse_string_list(children, key, origin, warnings)),
        "anti_patterns" => out
            .anti_patterns
            .extend(parse_string_list(children, key, origin, warnings)),
        "extra" => out
            .extra
            .extend(parse_extra_map(children, origin, warnings)),
        other => warnings.push(LoadDiagnostic::SchemaWarning {
            origin: origin.clone(),
            field: other.into(),
            message: "unknown block key".into(),
        }),
    }
}

fn parse_string_list(
    children: &[&str],
    key: &str,
    origin: &SourceOrigin,
    warnings: &mut Vec<LoadDiagnostic>,
) -> Vec<String> {
    let mut out = Vec::new();
    for line in children {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("- ") {
            out.push(unquote(rest.trim()));
        } else if t == "-" {
            // bare dash — empty entry, ignore.
        } else if t.is_empty() {
            continue;
        } else {
            warnings.push(LoadDiagnostic::SchemaWarning {
                origin: origin.clone(),
                field: key.into(),
                message: format!("expected '- item' line, got {:?}", line),
            });
        }
    }
    out
}

fn parse_extra_map(
    children: &[&str],
    origin: &SourceOrigin,
    warnings: &mut Vec<LoadDiagnostic>,
) -> Vec<(String, serde_json::Value)> {
    let mut out = Vec::new();
    for line in children {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let Some(idx) = trimmed.find(':') else {
            warnings.push(LoadDiagnostic::SchemaWarning {
                origin: origin.clone(),
                field: "extra".into(),
                message: format!("expected 'key: value', got {:?}", line),
            });
            continue;
        };
        let (k, v) = trimmed.split_at(idx);
        let k = k.trim().to_string();
        let v = v[1..].trim();
        out.push((k, scalar_to_json(v)));
    }
    out
}

/// Strip surrounding double or single quotes if present.
fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        let first = bytes[0];
        let last = bytes[s.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

/// Decode an unquoted scalar into a JSON value.
///
/// `true` / `false` → bool. Pure integer → number. `"..."` → string.
/// Anything else → string.
fn scalar_to_json(raw: &str) -> serde_json::Value {
    if raw.starts_with('"') || raw.starts_with('\'') {
        return serde_json::Value::String(unquote(raw));
    }
    if raw == "true" {
        return serde_json::Value::Bool(true);
    }
    if raw == "false" {
        return serde_json::Value::Bool(false);
    }
    if let Ok(n) = raw.parse::<i64>() {
        return serde_json::Value::Number(n.into());
    }
    serde_json::Value::String(raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_md(dir: &Path, name: &str, content: &str) {
        let p = dir.join(name);
        let mut f = fs::File::create(p).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn split_frontmatter_basic() {
        let raw = "---\nname: x\n---\nbody here\n";
        let (fm, body) = split_frontmatter(raw).unwrap();
        assert_eq!(fm, "name: x\n");
        assert_eq!(body, "body here\n");
    }

    #[test]
    fn split_frontmatter_missing_close_returns_none() {
        let raw = "---\nname: x\nbody here\n";
        assert!(split_frontmatter(raw).is_none());
    }

    #[test]
    fn split_frontmatter_no_leading_marker_returns_none() {
        assert!(split_frontmatter("nothing here").is_none());
    }

    #[test]
    fn unquote_handles_quotes() {
        assert_eq!(unquote("\"hello\""), "hello");
        assert_eq!(unquote("'hi'"), "hi");
        assert_eq!(unquote("plain"), "plain");
    }

    #[test]
    fn scalar_to_json_typed() {
        assert_eq!(scalar_to_json("true"), serde_json::Value::Bool(true));
        assert_eq!(scalar_to_json("42"), serde_json::Value::Number(42.into()));
        assert_eq!(scalar_to_json("hi"), serde_json::Value::String("hi".into()));
        assert_eq!(
            scalar_to_json("\"hi\""),
            serde_json::Value::String("hi".into())
        );
    }

    #[test]
    fn loads_basic_command_from_dir() {
        let dir = tempdir();
        write_md(
            dir.path(),
            "deploy.md",
            "---\nname: deploy\nsummary: Deploy the app\nmutating: true\n---\nLong description here.\n",
        );
        let src = MarkdownDirSource::new("test", dir.path(), Layer::Project);
        let load = src.load();
        assert_eq!(load.commands.len(), 1, "{:?}", load.diagnostics);
        let lc = &load.commands[0];
        assert_eq!(lc.command.canonical, "deploy");
        assert_eq!(lc.command.summary, "Deploy the app");
        assert!(lc.command.mutating);
        assert!(lc.command.description.contains("Long description"));
        assert!(matches!(lc.origin.layer, Layer::Project));
    }

    #[test]
    fn loads_lists_and_extras() {
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            r#"---
name: x
aliases:
  - a
  - b
spellings:
  - X
semantic_aliases:
  - "do the thing"
best_practices:
  - "always dry-run"
anti_patterns:
  - "deploy on Friday"
extra:
  category: "infra"
  count: 7
  flag: true
---
"#,
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        assert_eq!(load.commands.len(), 1, "{:?}", load.diagnostics);
        let cmd = &load.commands[0].command;
        assert_eq!(cmd.aliases, vec!["a", "b"]);
        assert_eq!(cmd.spellings, vec!["X"]);
        assert_eq!(cmd.semantic_aliases, vec!["do the thing"]);
        assert_eq!(cmd.best_practices, vec!["always dry-run"]);
        assert_eq!(cmd.anti_patterns, vec!["deploy on Friday"]);
        assert_eq!(cmd.extra["category"], serde_json::json!("infra"));
        assert_eq!(cmd.extra["count"], serde_json::json!(7));
        assert_eq!(cmd.extra["flag"], serde_json::json!(true));
    }

    #[test]
    fn priority_and_overrides_are_attached() {
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            "---\nname: deploy\npriority: 99\noverrides: deploy\n---\n",
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Local).load();
        let lc = &load.commands[0];
        assert_eq!(lc.priority, 99);
        assert_eq!(lc.overrides.as_deref(), Some("deploy"));
    }

    #[test]
    fn explicit_layer_field_overrides_default() {
        let dir = tempdir();
        write_md(dir.path(), "x.md", "---\nname: x\nlayer: local\n---\n");
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        assert!(matches!(load.commands[0].origin.layer, Layer::Local));
    }

    #[test]
    fn unknown_layer_emits_warning_and_falls_back() {
        let dir = tempdir();
        write_md(dir.path(), "x.md", "---\nname: x\nlayer: bogus\n---\n");
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        assert!(matches!(load.commands[0].origin.layer, Layer::Project));
        assert!(load
            .diagnostics
            .iter()
            .any(|d| matches!(d, LoadDiagnostic::SchemaWarning { field, .. } if field == "layer")));
    }

    #[test]
    fn missing_name_skips_file_with_warning() {
        let dir = tempdir();
        write_md(dir.path(), "x.md", "---\nsummary: nope\n---\n");
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        assert!(load.commands.is_empty());
        assert!(load
            .diagnostics
            .iter()
            .any(|d| matches!(d, LoadDiagnostic::SchemaWarning { field, .. } if field == "name")));
    }

    #[test]
    fn missing_directory_is_source_error_by_default() {
        let load =
            MarkdownDirSource::new("t", "/nonexistent/path/argot-test", Layer::Project).load();
        assert!(load.commands.is_empty());
        assert!(load
            .diagnostics
            .iter()
            .any(|d| matches!(d, LoadDiagnostic::SourceError { .. })));
    }

    #[test]
    fn missing_directory_when_optional_is_warning() {
        let load = MarkdownDirSource::new("t", "/nonexistent/path/argot-test", Layer::Project)
            .optional()
            .load();
        assert!(load.commands.is_empty());
        assert!(load
            .diagnostics
            .iter()
            .all(|d| !matches!(d, LoadDiagnostic::SourceError { .. })));
        assert!(load
            .diagnostics
            .iter()
            .any(|d| matches!(d, LoadDiagnostic::SchemaWarning { .. })));
    }

    #[test]
    fn unknown_keys_warn_but_do_not_block() {
        let dir = tempdir();
        write_md(dir.path(), "x.md", "---\nname: x\nbogus_key: value\n---\n");
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        assert_eq!(load.commands.len(), 1);
        assert!(load.diagnostics.iter().any(
            |d| matches!(d, LoadDiagnostic::SchemaWarning { field, .. } if field == "bogus_key")
        ));
    }

    #[test]
    fn deterministic_file_load_order() {
        let dir = tempdir();
        write_md(dir.path(), "z.md", "---\nname: z\n---\n");
        write_md(dir.path(), "a.md", "---\nname: a\n---\n");
        write_md(dir.path(), "m.md", "---\nname: m\n---\n");
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        let names: Vec<&str> = load
            .commands
            .iter()
            .map(|c| c.command.canonical.as_str())
            .collect();
        // sorted by file path, which is alphabetical.
        assert_eq!(names, vec!["a", "m", "z"]);
    }

    #[test]
    fn crlf_line_endings_supported() {
        // Windows-authored file. The splitter handles \r\n explicitly; this
        // pins that contract.
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            "---\r\nname: x\r\nsummary: Win-style\r\n---\r\nbody line\r\n",
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        assert_eq!(load.commands.len(), 1, "{:?}", load.diagnostics);
        let cmd = &load.commands[0].command;
        assert_eq!(cmd.canonical, "x");
        assert_eq!(cmd.summary, "Win-style");
    }

    #[test]
    fn body_with_horizontal_rule_keeps_first_split() {
        // A body that contains a `---` markdown horizontal rule must not
        // re-split the frontmatter; only the first closing fence terminates.
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            "---\nname: x\n---\nfirst para\n---\nsecond para\n",
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        assert_eq!(load.commands.len(), 1, "{:?}", load.diagnostics);
        let desc = &load.commands[0].command.description;
        assert!(desc.contains("first para"));
        assert!(desc.contains("---"));
        assert!(desc.contains("second para"));
    }

    #[test]
    fn origin_path_is_set_for_disk_loaded_commands() {
        // Diagnostics rely on origin.path being populated for file-backed
        // sources. Pin that contract.
        let dir = tempdir();
        write_md(dir.path(), "x.md", "---\nname: x\n---\n");
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        let path = load.commands[0].origin.path.as_deref().unwrap();
        assert!(
            path.ends_with("x.md"),
            "expected ends_with x.md, got {}",
            path
        );
    }

    #[test]
    fn empty_directory_no_commands_no_diagnostics() {
        let dir = tempdir();
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        assert!(load.commands.is_empty());
        assert!(load.diagnostics.is_empty());
    }

    #[test]
    fn end_to_end_markdown_shadow_via_layered_builder() {
        // Build two MarkdownDirSource instances at different layers, the
        // higher one shadowing a same-named command from the lower. Verify
        // the registry sees the upper command and the diagnostic carries the
        // file paths from both winner and loser.
        use crate::source::LayeredBuilder;

        let lower = tempdir();
        write_md(
            lower.path(),
            "deploy.md",
            "---\nname: deploy\nsummary: Builtin deploy\n---\n",
        );
        let upper = tempdir();
        write_md(
            upper.path(),
            "deploy.md",
            "---\nname: deploy\nsummary: User deploy\n---\n",
        );

        let (registry, diags) = LayeredBuilder::new()
            .add(MarkdownDirSource::new(
                "builtin",
                lower.path(),
                Layer::Embedded,
            ))
            .add(MarkdownDirSource::new("user", upper.path(), Layer::User))
            .build();

        assert_eq!(
            registry.get_command("deploy").unwrap().summary,
            "User deploy",
            "user-layer markdown command must shadow embedded one"
        );
        let shadow = diags
            .iter()
            .find_map(|d| match d {
                LoadDiagnostic::Shadowed {
                    shadowed, winner, ..
                } => Some((shadowed, winner)),
                _ => None,
            })
            .expect("missing Shadowed diagnostic");
        assert!(shadow.0.path.as_deref().unwrap().ends_with("deploy.md"));
        assert!(shadow.1.path.as_deref().unwrap().ends_with("deploy.md"));
        assert!(matches!(shadow.0.layer, Layer::Embedded));
        assert!(matches!(shadow.1.layer, Layer::User));
    }

    // RAII guard that snapshots HOME/XDG_CONFIG_HOME/USERPROFILE/APPDATA on
    // construction and restores them on Drop — so even if an assertion in the
    // bundled test panics, the next test in the suite sees the developer's
    // real environment, not the test's last mutation.
    struct EnvSnapshot {
        xdg: Option<std::ffi::OsString>,
        home: Option<std::ffi::OsString>,
        userprofile: Option<std::ffi::OsString>,
        appdata: Option<std::ffi::OsString>,
    }
    impl EnvSnapshot {
        fn capture() -> Self {
            Self {
                xdg: std::env::var_os("XDG_CONFIG_HOME"),
                home: std::env::var_os("HOME"),
                userprofile: std::env::var_os("USERPROFILE"),
                appdata: std::env::var_os("APPDATA"),
            }
        }
    }
    impl Drop for EnvSnapshot {
        fn drop(&mut self) {
            fn restore(name: &str, prior: &Option<std::ffi::OsString>) {
                match prior {
                    Some(v) => std::env::set_var(name, v),
                    None => std::env::remove_var(name),
                }
            }
            restore("XDG_CONFIG_HOME", &self.xdg);
            restore("HOME", &self.home);
            restore("USERPROFILE", &self.userprofile);
            restore("APPDATA", &self.appdata);
        }
    }

    // All env-mutation cases are bundled into a single #[test] because cargo
    // runs tests in parallel and process env is shared mutable state.
    // Splitting these would race with each other and with other tests that
    // happen to read HOME/XDG_CONFIG_HOME. Restoration is via a Drop guard so
    // that a failed assertion does not leak the test's mutated env.
    #[test]
    fn user_config_dir_resolution_order() {
        let _guard = EnvSnapshot::capture();

        // Case 1: XDG_CONFIG_HOME wins over HOME.
        let tmp_xdg = tempdir();
        let tmp_home = tempdir();
        std::env::set_var("XDG_CONFIG_HOME", tmp_xdg.path());
        std::env::set_var("HOME", tmp_home.path());
        let resolved = user_config_dir("argot-test").unwrap();
        assert_eq!(
            resolved,
            tmp_xdg.path().join("argot-test").join("commands"),
            "XDG_CONFIG_HOME should take precedence over HOME"
        );

        // Case 1b: convenience constructor wraps the resolved dir in an
        // optional source. Pin both the Some-path and the .optional() flag.
        let src = MarkdownDirSource::user_config("argot-test")
            .expect("XDG was set, user_config must return Some");
        let load = src.load();
        // Directory does not exist on disk, but .optional() means we get a
        // SchemaWarning rather than a hard SourceError.
        assert!(
            load.diagnostics
                .iter()
                .all(|d| !matches!(d, LoadDiagnostic::SourceError { .. })),
            "user_config() must produce an optional source; got SourceError in {:?}",
            load.diagnostics
        );

        // Case 2: HOME fallback when XDG is unset.
        std::env::remove_var("XDG_CONFIG_HOME");
        let resolved = user_config_dir("argot-test").unwrap();
        assert_eq!(
            resolved,
            tmp_home
                .path()
                .join(".config")
                .join("argot-test")
                .join("commands"),
            "HOME fallback should resolve to $HOME/.config/<app>/commands"
        );

        // Case 3: empty XDG_CONFIG_HOME is treated as unset (XDG Base Dir spec).
        std::env::set_var("XDG_CONFIG_HOME", "");
        let resolved = user_config_dir("argot-test").unwrap();
        assert!(
            resolved.starts_with(tmp_home.path()),
            "empty XDG_CONFIG_HOME should fall through to HOME"
        );

        // Case 4: nothing set at all → None.
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("HOME");
        std::env::remove_var("USERPROFILE");
        std::env::remove_var("APPDATA");
        assert!(
            user_config_dir("argot-test").is_none(),
            "no home env should yield None"
        );
        assert!(
            MarkdownDirSource::user_config("argot-test").is_none(),
            "convenience constructor should propagate None"
        );
    }

    #[test]
    fn find_project_dir_walks_up_from_subdir() {
        // Layout:
        //   <root>/.argot-test/commands/
        //   <root>/sub/deep/   <- start here, walks up to find the marker
        let root = tempdir();
        fs::create_dir_all(root.path().join(".argot-test").join("commands")).unwrap();
        let deep = root.path().join("sub").join("deep");
        fs::create_dir_all(&deep).unwrap();

        let found = find_project_dir("argot-test", &deep).unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            root.path()
                .join(".argot-test")
                .join("commands")
                .canonicalize()
                .unwrap()
        );
    }

    #[test]
    fn find_project_dir_returns_none_when_no_marker() {
        let root = tempdir();
        let deep = root.path().join("sub");
        fs::create_dir_all(&deep).unwrap();
        assert!(find_project_dir("nonexistent-app", &deep).is_none());
    }

    #[test]
    fn find_project_dir_skips_non_directory_at_marker() {
        // If `.foo/commands` is a regular file (not a directory), it does not
        // satisfy the marker and the walk continues. Pin this contract so a
        // future refactor that changes is_dir() semantics is caught.
        let root = tempdir();
        let app_dir = root.path().join(".argot-non-dir");
        fs::create_dir_all(&app_dir).unwrap();
        // Create a regular file at .argot-non-dir/commands instead of a directory.
        std::fs::write(app_dir.join("commands"), b"not a directory").unwrap();
        let deep = root.path().join("sub");
        fs::create_dir_all(&deep).unwrap();
        assert!(
            find_project_dir("argot-non-dir", &deep).is_none(),
            "non-directory at marker location must not satisfy the lookup"
        );
    }

    #[test]
    fn project_root_returns_none_for_missing_marker() {
        // From the developer's normal cwd (this repo's root or the temp dir
        // a test runner uses) there is no .<random>/commands ancestor.
        assert!(
            MarkdownDirSource::project_root("xyzzy-no-such-app-9f8e7d").is_none(),
            "project_root must return None when no marker dir exists"
        );
    }

    #[test]
    fn parses_arguments_section_into_typed_arguments() {
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            "---\nname: deploy\nsummary: Deploy\n---\n\
             Description prelude.\n\
             \n## Arguments\n\
             \n- `env` (required): Target environment\n\
             - `service`: Specific service\n\
             - `paths` (variadic): Files to operate on\n",
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        assert_eq!(load.commands.len(), 1, "{:?}", load.diagnostics);
        let cmd = &load.commands[0].command;
        assert_eq!(cmd.arguments.len(), 3);
        assert_eq!(cmd.arguments[0].name, "env");
        assert!(cmd.arguments[0].required);
        assert_eq!(cmd.arguments[0].description, "Target environment");
        assert_eq!(cmd.arguments[1].name, "service");
        assert!(!cmd.arguments[1].required);
        assert!(cmd.arguments[2].variadic);
        // Description prelude is still captured (the `## Arguments` section is
        // stripped from the description body).
        assert!(cmd.description.contains("Description prelude"));
        assert!(!cmd.description.contains("## Arguments"));
    }

    #[test]
    fn parses_flags_section_with_short_value_and_modifiers() {
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            "---\nname: serve\n---\n\
             ## Flags\n\
             \n- `--port, -p` <NUM> (default: 8080): Listen port\n\
             - `--host` <HOST> (env: SERVE_HOST): Bind address\n\
             - `--verbose, -v` (repeatable): Increase verbosity\n\
             - `--format` <FMT> (choices: json|yaml|text): Output format\n",
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        assert_eq!(load.commands.len(), 1, "{:?}", load.diagnostics);
        let cmd = &load.commands[0].command;
        assert_eq!(cmd.flags.len(), 4);

        let port = cmd.flags.iter().find(|f| f.name == "port").unwrap();
        assert_eq!(port.short, Some('p'));
        assert!(port.takes_value);
        assert_eq!(port.default.as_deref(), Some("8080"));

        let host = cmd.flags.iter().find(|f| f.name == "host").unwrap();
        assert!(host.takes_value);
        assert_eq!(host.env.as_deref(), Some("SERVE_HOST"));

        let verbose = cmd.flags.iter().find(|f| f.name == "verbose").unwrap();
        assert_eq!(verbose.short, Some('v'));
        assert!(verbose.repeatable);

        let format = cmd.flags.iter().find(|f| f.name == "format").unwrap();
        let choices = format.choices.as_ref().expect("choices set");
        assert_eq!(*choices, vec!["json", "yaml", "text"]);
    }

    #[test]
    fn parses_examples_section_with_and_without_titles() {
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            "---\nname: deploy\n---\n\
             ## Examples\n\
             \n- Basic deploy: `myapp deploy production`\n\
             - `myapp deploy --dry-run`\n",
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        let cmd = &load.commands[0].command;
        assert_eq!(cmd.examples.len(), 2);
        assert_eq!(cmd.examples[0].description, "Basic deploy");
        assert_eq!(cmd.examples[0].command, "myapp deploy production");
        // Title-less form falls back to using the command as the title.
        assert_eq!(cmd.examples[1].command, "myapp deploy --dry-run");
        assert_eq!(cmd.examples[1].description, "myapp deploy --dry-run");
    }

    #[test]
    fn malformed_section_bullets_warn_but_dont_abort() {
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            "---\nname: x\n---\n\
             ## Arguments\n\
             \n- `valid`: ok\n\
             - this line has no backticks\n\
             - `bad` (unknown_modifier): still loads\n",
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        let cmd = &load.commands[0].command;
        // `valid` and `bad` both load; the malformed line is dropped.
        assert_eq!(cmd.arguments.len(), 2);
        // We expect at least two SchemaWarnings (no-backticks line + unknown modifier).
        let warning_count = load
            .diagnostics
            .iter()
            .filter(|d| matches!(d, LoadDiagnostic::SchemaWarning { field, .. } if field == "Arguments"))
            .count();
        assert!(
            warning_count >= 2,
            "got diagnostics: {:?}",
            load.diagnostics
        );
    }

    #[test]
    fn heading_match_is_case_insensitive() {
        // Pin documented case-insensitivity. A future refactor that switches
        // to ==/eq would silently downgrade structured sections to prose,
        // leaving cmd.arguments / cmd.flags empty without any diagnostic.
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            "---\nname: x\n---\n\
             ## arguments\n- `env` (required): the env\n\
             ## FLAGS\n- `--verbose, -v`: noisy\n\
             ## Examples\n- `myapp x`\n",
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        let cmd = &load.commands[0].command;
        assert_eq!(
            cmd.arguments.len(),
            1,
            "lowercase ## arguments should parse"
        );
        assert_eq!(cmd.flags.len(), 1, "uppercase ## FLAGS should parse");
        assert_eq!(cmd.examples.len(), 1);
    }

    #[test]
    fn duplicate_flag_names_abort_file_with_source_error() {
        // Pin the documented hard-failure: Command::build rejects duplicate
        // flag names, which propagates as a SourceError and the whole file
        // is dropped from the registry. This is intentional (a half-loaded
        // command with a missing flag is worse than no command at all).
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            "---\nname: x\n---\n\
             ## Flags\n- `--port, -p` <NUM>: First\n- `--port`: Second\n",
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        assert!(
            load.commands.is_empty(),
            "duplicate flag names should drop the whole file"
        );
        assert!(
            load.diagnostics.iter().any(|d| matches!(
                d,
                LoadDiagnostic::SourceError { message, .. } if message.contains("duplicate flag")
            )),
            "expected SourceError mentioning 'duplicate flag', got {:?}",
            load.diagnostics
        );
    }

    #[test]
    fn unterminated_flag_value_keeps_takes_value_and_warns() {
        // Author wrote `<NUM` (forgot closing `>`). Their intent is
        // unambiguous, so takes_value stays true and a warning fires.
        // Subsequent modifiers should still be picked up.
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            "---\nname: x\n---\n\
             ## Flags\n- `--port, -p` <NUM (default: 8080): Listen port\n",
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        let cmd = &load.commands[0].command;
        let port = cmd.flags.iter().find(|f| f.name == "port").unwrap();
        assert!(
            port.takes_value,
            "takes_value should remain true on unterminated <VALUE>"
        );
        assert_eq!(
            port.default.as_deref(),
            Some("8080"),
            "modifiers after the malformed <VALUE> should still be parsed"
        );
        assert!(load.diagnostics.iter().any(|d| matches!(
            d,
            LoadDiagnostic::SchemaWarning { message, .. } if message.contains("unterminated `<VALUE>`")
        )));
    }

    #[test]
    fn unterminated_modifier_paren_warns_with_content() {
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            "---\nname: x\n---\n\
             ## Arguments\n- `env` (required: prod environment\n",
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        // The argument still loads (with no modifiers), but a warning is emitted.
        let cmd = &load.commands[0].command;
        assert_eq!(cmd.arguments.len(), 1);
        assert!(load.diagnostics.iter().any(|d| matches!(
            d,
            LoadDiagnostic::SchemaWarning { message, .. } if message.contains("unterminated `(`")
        )));
    }

    #[test]
    fn multi_char_short_flag_warns_and_drops_short() {
        // `-vv` is structurally invalid as a short alias (must be one char).
        // Without an explicit warning the user is left wondering why the
        // short form silently disappeared from the loaded Command.
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            "---\nname: x\n---\n## Flags\n- `--verbose, -vv`: noisy\n",
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        let cmd = &load.commands[0].command;
        let verbose = cmd.flags.iter().find(|f| f.name == "verbose").unwrap();
        assert!(
            verbose.short.is_none(),
            "multi-char short alias must not be applied"
        );
        assert!(load.diagnostics.iter().any(|d| matches!(
            d,
            LoadDiagnostic::SchemaWarning { message, .. } if message.contains("must be a single character")
        )));
    }

    #[test]
    fn empty_choices_modifier_warns() {
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            "---\nname: x\n---\n\
             ## Flags\n- `--format` <FMT> (choices: |||): Output format\n",
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        let cmd = &load.commands[0].command;
        let format = cmd.flags.iter().find(|f| f.name == "format").unwrap();
        // Empty choices are not applied; a warning explains why.
        assert!(format.choices.is_none() || format.choices.as_ref().unwrap().is_empty());
        assert!(load.diagnostics.iter().any(|d| matches!(
            d,
            LoadDiagnostic::SchemaWarning { message, .. } if message.contains("empty `choices:`")
        )));
    }

    #[test]
    fn example_title_with_internal_colons_splits_at_command_boundary() {
        // Title contains a colon (e.g. URL). The split must find the colon
        // followed by a backtick, not the first colon in the content.
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            "---\nname: x\n---\n\
             ## Examples\n- See https://example.com:8080: `myapp run`\n",
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        let cmd = &load.commands[0].command;
        assert_eq!(cmd.examples.len(), 1, "{:?}", load.diagnostics);
        assert_eq!(cmd.examples[0].command, "myapp run");
        assert_eq!(cmd.examples[0].description, "See https://example.com:8080");
    }

    #[test]
    fn unknown_h2_routes_content_into_description() {
        // Per documented contract, `## Notes` after `## Examples` switches
        // back to prelude mode so the prose lands in description rather
        // than being eaten by the Examples parser.
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            "---\nname: x\n---\n\
             prelude line\n\
             \n## Examples\n- `cmd one`\n\
             \n## Notes\n- See also: foo\nFree-form notes line.\n",
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        let cmd = &load.commands[0].command;
        assert_eq!(
            cmd.examples.len(),
            1,
            "## Notes content must not become an example"
        );
        assert!(
            cmd.description.contains("## Notes"),
            "## Notes heading should round-trip into description, got: {:?}",
            cmd.description
        );
        assert!(cmd.description.contains("See also: foo"));
        assert!(cmd.description.contains("Free-form notes line"));
    }

    #[test]
    fn unknown_h2_heading_kept_as_prose() {
        // `## Notes` after `## Examples` should not break parsing — its content
        // accumulates into the Examples section (treated as prose). This is
        // documented behaviour of split_body_sections.
        let dir = tempdir();
        write_md(
            dir.path(),
            "x.md",
            "---\nname: x\n---\n\
             prelude line\n\
             \n## Examples\n- `cmd one`\n\
             \n## Notes\nSome free-form notes that should not crash parsing.\n",
        );
        let load = MarkdownDirSource::new("t", dir.path(), Layer::Project).load();
        let cmd = &load.commands[0].command;
        assert_eq!(cmd.examples.len(), 1);
        // The prelude is preserved; trailing free-form headings after a
        // recognised section are absorbed but do not corrupt the prelude.
        assert!(cmd.description.contains("prelude line"));
    }

    #[test]
    fn priority_from_frontmatter_drives_merge() {
        // Author intends the high-priority project file to win against the
        // default-priority one in the same layer. Without the frontmatter
        // priority being respected at the merge boundary, this test fails.
        use crate::source::LayeredBuilder;

        let dir = tempdir();
        write_md(dir.path(), "a.md", "---\nname: deploy\nsummary: low\n---\n");
        write_md(
            dir.path(),
            "b.md",
            "---\nname: deploy\nsummary: high\npriority: 10\n---\n",
        );
        let (registry, _) = LayeredBuilder::new()
            .add(MarkdownDirSource::new("p", dir.path(), Layer::Project))
            .build();
        assert_eq!(registry.get_command("deploy").unwrap().summary, "high");
    }
}
