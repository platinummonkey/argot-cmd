//! Human-readable and Markdown renderers for commands.
//!
//! This module exposes three rendering functions and one disambiguation helper:
//!
//! - **[`render_help`]** — produces a multi-section plain-text help page
//!   (NAME, SUMMARY, DESCRIPTION, USAGE, ARGUMENTS, FLAGS, SUBCOMMANDS,
//!   EXAMPLES, BEST PRACTICES, ANTI-PATTERNS). Sections that have no content
//!   are omitted.
//!
//! - **[`render_subcommand_list`]** — produces a compact two-column listing of
//!   `canonical  summary` lines, suitable for a top-level `--help` display.
//!
//! - **[`render_markdown`]** — produces a GitHub-flavored Markdown page with
//!   `##` headings, argument/flag tables, and fenced code blocks for examples.
//!
//! - **[`render_ambiguity`]** — formats a human-readable message when a
//!   command token is ambiguous.
//!
//! None of the functions print to stdout/stderr directly; all return a
//! `String` that the caller can write wherever appropriate.

use crate::model::Command;

/// A pluggable renderer for command help, Markdown docs, and disambiguation messages.
///
/// Implement this trait to fully customize how argot formats its output.
/// Use [`Cli::with_renderer`] to inject your implementation.
///
/// A [`DefaultRenderer`] is provided that delegates to the module-level free
/// functions ([`render_help`], [`render_markdown`], etc.).
///
/// # Examples
///
/// ```
/// # use argot::{Command, render::Renderer};
/// struct UppercaseRenderer;
///
/// impl Renderer for UppercaseRenderer {
///     fn render_help(&self, command: &Command) -> String {
///         argot::render_help(command).to_uppercase()
///     }
///     fn render_markdown(&self, command: &Command) -> String {
///         argot::render_markdown(command)
///     }
///     fn render_subcommand_list(&self, commands: &[Command]) -> String {
///         argot::render_subcommand_list(commands)
///     }
///     fn render_ambiguity(&self, input: &str, candidates: &[String]) -> String {
///         argot::render_ambiguity(input, candidates)
///     }
/// }
/// ```
pub trait Renderer: Send + Sync {
    /// Render a plain-text help page for a command.
    fn render_help(&self, command: &crate::model::Command) -> String;
    /// Render a Markdown documentation page for a command.
    fn render_markdown(&self, command: &crate::model::Command) -> String;
    /// Render a compact listing of multiple commands.
    fn render_subcommand_list(&self, commands: &[crate::model::Command]) -> String;
    /// Render a disambiguation message for an ambiguous command token.
    fn render_ambiguity(&self, input: &str, candidates: &[String]) -> String;
}

/// The default renderer. Delegates to the module-level free functions.
///
/// This is used by [`crate::Cli`] unless overridden with [`crate::Cli::with_renderer`].
#[derive(Debug, Default, Clone)]
pub struct DefaultRenderer;

impl Renderer for DefaultRenderer {
    fn render_help(&self, command: &crate::model::Command) -> String {
        render_help(command)
    }
    fn render_markdown(&self, command: &crate::model::Command) -> String {
        render_markdown(command)
    }
    fn render_subcommand_list(&self, commands: &[crate::model::Command]) -> String {
        render_subcommand_list(commands)
    }
    fn render_ambiguity(&self, input: &str, candidates: &[String]) -> String {
        render_ambiguity(input, candidates)
    }
}

/// Render a human-readable help page for a command.
///
/// The output contains the following sections (each omitted when empty):
/// NAME, SUMMARY, DESCRIPTION, USAGE, ARGUMENTS, FLAGS, SUBCOMMANDS,
/// EXAMPLES, BEST PRACTICES, ANTI-PATTERNS.
///
/// # Arguments
///
/// - `command` — The command to render help for.
///
/// # Examples
///
/// ```
/// # use argot::{Command, render_help};
/// let cmd = Command::builder("greet")
///     .summary("Say hello")
///     .build()
///     .unwrap();
///
/// let help = render_help(&cmd);
/// assert!(help.contains("NAME"));
/// assert!(help.contains("greet"));
/// assert!(help.contains("SUMMARY"));
/// ```
pub fn render_help(command: &Command) -> String {
    let mut out = String::new();

    // NAME
    let name_line = if command.aliases.is_empty() {
        command.canonical.clone()
    } else {
        format!("{} ({})", command.canonical, command.aliases.join(", "))
    };
    out.push_str(&format!("NAME\n    {}\n\n", name_line));

    if !command.summary.is_empty() {
        out.push_str(&format!("SUMMARY\n    {}\n\n", command.summary));
    }

    if !command.description.is_empty() {
        out.push_str(&format!("DESCRIPTION\n    {}\n\n", command.description));
    }

    out.push_str(&format!("USAGE\n    {}\n\n", build_usage(command)));

    if !command.arguments.is_empty() {
        out.push_str("ARGUMENTS\n");
        for arg in &command.arguments {
            let req = if arg.required { " (required)" } else { "" };
            out.push_str(&format!("    <{}>  {}{}\n", arg.name, arg.description, req));
        }
        out.push('\n');
    }

    if !command.flags.is_empty() {
        out.push_str("FLAGS\n");
        for flag in &command.flags {
            let short_part = flag.short.map(|c| format!("-{}, ", c)).unwrap_or_default();
            let req = if flag.required { " (required)" } else { "" };
            out.push_str(&format!(
                "    {}--{}  {}{}\n",
                short_part, flag.name, flag.description, req
            ));
        }
        out.push('\n');
    }

    if !command.subcommands.is_empty() {
        out.push_str("SUBCOMMANDS\n");
        for sub in &command.subcommands {
            out.push_str(&format!("    {}  {}\n", sub.canonical, sub.summary));
        }
        out.push('\n');
    }

    if !command.examples.is_empty() {
        out.push_str("EXAMPLES\n");
        for ex in &command.examples {
            out.push_str(&format!("    # {}\n    {}\n", ex.description, ex.command));
            if let Some(output) = &ex.output {
                out.push_str(&format!("    # Output: {}\n", output));
            }
            out.push('\n');
        }
    }

    if !command.best_practices.is_empty() {
        out.push_str("BEST PRACTICES\n");
        for bp in &command.best_practices {
            out.push_str(&format!("    - {}\n", bp));
        }
        out.push('\n');
    }

    if !command.anti_patterns.is_empty() {
        out.push_str("ANTI-PATTERNS\n");
        for ap in &command.anti_patterns {
            out.push_str(&format!("    - {}\n", ap));
        }
        out.push('\n');
    }

    out
}

/// Render a compact listing of multiple commands (e.g. for a top-level help).
///
/// Each line has the format `  canonical  summary`. This is suitable for
/// displaying all registered commands when no specific command is requested.
///
/// # Arguments
///
/// - `commands` — The commands to list.
///
/// # Examples
///
/// ```
/// # use argot::{Command, render_subcommand_list};
/// let cmds = vec![
///     Command::builder("list").summary("List items").build().unwrap(),
///     Command::builder("get").summary("Get an item").build().unwrap(),
/// ];
///
/// let listing = render_subcommand_list(&cmds);
/// assert!(listing.contains("list"));
/// assert!(listing.contains("List items"));
/// ```
pub fn render_subcommand_list(commands: &[Command]) -> String {
    let mut out = String::new();
    for cmd in commands {
        out.push_str(&format!("  {}  {}\n", cmd.canonical, cmd.summary));
    }
    out
}

/// Render a Markdown documentation page for a command.
///
/// The output is GitHub-flavored Markdown with:
/// - A `# canonical` top-level heading.
/// - `##` headings for Description, Usage, Arguments, Flags, Subcommands,
///   Examples, Best Practices, and Anti-Patterns.
/// - Arguments and flags rendered as Markdown tables.
/// - Usage and examples rendered as fenced code blocks.
///
/// Empty sections are omitted.
///
/// # Arguments
///
/// - `command` — The command to render documentation for.
///
/// # Examples
///
/// ```
/// # use argot::{Command, render_markdown};
/// let cmd = Command::builder("deploy")
///     .summary("Deploy the app")
///     .build()
///     .unwrap();
///
/// let md = render_markdown(&cmd);
/// assert!(md.starts_with("# deploy"));
/// assert!(md.contains("Deploy the app"));
/// ```
pub fn render_markdown(command: &Command) -> String {
    let mut out = String::new();

    out.push_str(&format!("# {}\n\n", command.canonical));

    if !command.summary.is_empty() {
        out.push_str(&format!("{}\n\n", command.summary));
    }

    if !command.description.is_empty() {
        out.push_str(&format!("## Description\n\n{}\n\n", command.description));
    }

    out.push_str(&format!(
        "## Usage\n\n```\n{}\n```\n\n",
        build_usage(command)
    ));

    if !command.arguments.is_empty() {
        out.push_str("## Arguments\n\n");
        out.push_str("| Name | Description | Required |\n");
        out.push_str("|------|-------------|----------|\n");
        for arg in &command.arguments {
            out.push_str(&format!(
                "| `{}` | {} | {} |\n",
                arg.name, arg.description, arg.required
            ));
        }
        out.push('\n');
    }

    if !command.flags.is_empty() {
        out.push_str("## Flags\n\n");
        out.push_str("| Flag | Short | Description | Required |\n");
        out.push_str("|------|-------|-------------|----------|\n");
        for flag in &command.flags {
            let short = flag.short.map(|c| format!("`-{}`", c)).unwrap_or_default();
            out.push_str(&format!(
                "| `--{}` | {} | {} | {} |\n",
                flag.name, short, flag.description, flag.required
            ));
        }
        out.push('\n');
    }

    if !command.subcommands.is_empty() {
        out.push_str("## Subcommands\n\n");
        for sub in &command.subcommands {
            out.push_str(&format!("- **{}**: {}\n", sub.canonical, sub.summary));
        }
        out.push('\n');
    }

    if !command.examples.is_empty() {
        out.push_str("## Examples\n\n");
        for ex in &command.examples {
            out.push_str(&format!(
                "### {}\n\n```\n{}\n```\n\n",
                ex.description, ex.command
            ));
        }
    }

    if !command.best_practices.is_empty() {
        out.push_str("## Best Practices\n\n");
        for bp in &command.best_practices {
            out.push_str(&format!("- {}\n", bp));
        }
        out.push('\n');
    }

    if !command.anti_patterns.is_empty() {
        out.push_str("## Anti-Patterns\n\n");
        for ap in &command.anti_patterns {
            out.push_str(&format!("- {}\n", ap));
        }
        out.push('\n');
    }

    out
}

/// Render a human-readable disambiguation message.
///
/// Used when a command token matches more than one candidate as a prefix.
/// The message lists all candidate canonical names so the user or agent can
/// choose the intended command.
///
/// # Arguments
///
/// - `input` — The ambiguous token as typed by the user.
/// - `candidates` — Canonical names of all matching commands.
///
/// # Examples
///
/// ```
/// # use argot::render_ambiguity;
/// let msg = render_ambiguity("l", &["list".to_string(), "log".to_string()]);
/// assert!(msg.contains("Ambiguous command"));
/// assert!(msg.contains("list"));
/// assert!(msg.contains("log"));
/// ```
pub fn render_ambiguity(input: &str, candidates: &[String]) -> String {
    let list = candidates
        .iter()
        .map(|c| format!("  - {}", c))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Ambiguous command \"{}\". Did you mean one of:\n{}",
        input, list
    )
}

/// Render any [`crate::ResolveError`] as a human-readable string.
///
/// - [`crate::ResolveError::Ambiguous`] — delegates to [`render_ambiguity`].
/// - [`crate::ResolveError::Unknown`] with suggestions — formats a
///   "Did you mean?" message.
/// - [`crate::ResolveError::Unknown`] without suggestions — formats a
///   plain "Unknown command" message.
///
/// # Examples
///
/// ```
/// # use argot::{Command, Resolver};
/// # use argot::render::render_resolve_error;
/// let cmds = vec![
///     Command::builder("list").build().unwrap(),
///     Command::builder("log").build().unwrap(),
/// ];
/// let resolver = Resolver::new(&cmds);
///
/// let err = resolver.resolve("xyz").unwrap_err();
/// let msg = render_resolve_error(&err);
/// assert!(msg.contains("xyz"));
///
/// let err2 = resolver.resolve("l").unwrap_err();
/// let msg2 = render_resolve_error(&err2);
/// assert!(msg2.contains("list"));
/// ```
pub fn render_resolve_error(error: &crate::resolver::ResolveError) -> String {
    use crate::resolver::ResolveError;
    match error {
        ResolveError::Ambiguous { input, candidates } => render_ambiguity(input, candidates),
        ResolveError::Unknown { input, suggestions } if !suggestions.is_empty() => format!(
            "Unknown command: `{}`. Did you mean: {}?",
            input,
            suggestions.join(", ")
        ),
        ResolveError::Unknown { input, .. } => format!("Unknown command: `{}`", input),
    }
}

fn build_usage(command: &Command) -> String {
    let mut parts = vec![command.canonical.clone()];
    if !command.subcommands.is_empty() {
        parts.push("<subcommand>".to_string());
    }
    for arg in &command.arguments {
        if arg.required {
            parts.push(format!("<{}>", arg.name));
        } else {
            parts.push(format!("[{}]", arg.name));
        }
    }
    if !command.flags.is_empty() {
        parts.push("[flags]".to_string());
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Argument, Command, Example, Flag};

    fn full_command() -> Command {
        Command::builder("deploy")
            .alias("d")
            .summary("Deploy the application")
            .description("Deploys the app to the target environment.")
            .argument(
                Argument::builder("env")
                    .description("target environment")
                    .required()
                    .build()
                    .unwrap(),
            )
            .flag(
                Flag::builder("dry-run")
                    .short('n')
                    .description("simulate only")
                    .build()
                    .unwrap(),
            )
            .subcommand(
                Command::builder("rollback")
                    .summary("Roll back")
                    .build()
                    .unwrap(),
            )
            .example(Example::new("deploy to prod", "deploy prod").with_output("deployed"))
            .best_practice("always dry-run first")
            .anti_pattern("deploy on Friday")
            .build()
            .unwrap()
    }

    #[test]
    fn test_render_help_contains_all_sections() {
        let cmd = full_command();
        let help = render_help(&cmd);
        assert!(help.contains("NAME"), "missing NAME");
        assert!(help.contains("SUMMARY"), "missing SUMMARY");
        assert!(help.contains("DESCRIPTION"), "missing DESCRIPTION");
        assert!(help.contains("USAGE"), "missing USAGE");
        assert!(help.contains("ARGUMENTS"), "missing ARGUMENTS");
        assert!(help.contains("FLAGS"), "missing FLAGS");
        assert!(help.contains("SUBCOMMANDS"), "missing SUBCOMMANDS");
        assert!(help.contains("EXAMPLES"), "missing EXAMPLES");
        assert!(help.contains("BEST PRACTICES"), "missing BEST PRACTICES");
        assert!(help.contains("ANTI-PATTERNS"), "missing ANTI-PATTERNS");
    }

    #[test]
    fn test_render_help_omits_empty_sections() {
        let cmd = Command::builder("simple")
            .summary("Simple")
            .build()
            .unwrap();
        let help = render_help(&cmd);
        assert!(!help.contains("ARGUMENTS"));
        assert!(!help.contains("FLAGS"));
        assert!(!help.contains("SUBCOMMANDS"));
        assert!(!help.contains("EXAMPLES"));
        assert!(!help.contains("BEST PRACTICES"));
        assert!(!help.contains("ANTI-PATTERNS"));
    }

    #[test]
    fn test_render_help_shows_alias() {
        let cmd = full_command();
        let help = render_help(&cmd);
        assert!(help.contains('d')); // alias
    }

    #[test]
    fn test_render_markdown_starts_with_heading() {
        let cmd = full_command();
        let md = render_markdown(&cmd);
        assert!(md.starts_with("# deploy"));
    }

    #[test]
    fn test_render_markdown_contains_table() {
        let cmd = full_command();
        let md = render_markdown(&cmd);
        assert!(md.contains("| `env`"));
        assert!(md.contains("| `--dry-run`"));
    }

    #[test]
    fn test_render_ambiguity() {
        let candidates = vec!["list".to_string(), "log".to_string()];
        let msg = render_ambiguity("l", &candidates);
        assert!(msg.contains("Did you mean"));
        assert!(msg.contains("list"));
        assert!(msg.contains("log"));
    }

    #[test]
    fn test_render_subcommand_list() {
        let cmds = vec![
            Command::builder("a").summary("alpha").build().unwrap(),
            Command::builder("b").summary("beta").build().unwrap(),
        ];
        let out = render_subcommand_list(&cmds);
        assert!(out.contains("alpha"));
        assert!(out.contains("beta"));
    }

    #[test]
    fn test_render_resolve_error_unknown_no_suggestions() {
        use crate::resolver::ResolveError;
        let err = ResolveError::Unknown {
            input: "xyz".into(),
            suggestions: vec![],
        };
        let msg = render_resolve_error(&err);
        assert!(msg.contains("xyz"));
        assert!(!msg.contains("Did you mean"));
    }

    #[test]
    fn test_render_resolve_error_unknown_with_suggestions() {
        use crate::resolver::ResolveError;
        let err = ResolveError::Unknown {
            input: "lst".into(),
            suggestions: vec!["list".into()],
        };
        let msg = render_resolve_error(&err);
        assert!(msg.contains("lst") && msg.contains("list") && msg.contains("Did you mean"));
    }

    #[test]
    fn test_render_resolve_error_ambiguous() {
        use crate::resolver::ResolveError;
        let err = ResolveError::Ambiguous {
            input: "l".into(),
            candidates: vec!["list".into(), "log".into()],
        };
        let msg = render_resolve_error(&err);
        assert!(msg.contains("list") && msg.contains("log"));
    }

    #[test]
    fn test_default_renderer_delegates() {
        let cmd = Command::builder("test")
            .summary("A test command")
            .build()
            .unwrap();
        let r = DefaultRenderer;
        let help = r.render_help(&cmd);
        assert!(help.contains("test"));
        let md = r.render_markdown(&cmd);
        assert!(md.starts_with("# test"));
    }

    #[test]
    fn test_custom_renderer_via_cli() {
        struct Upper;
        impl Renderer for Upper {
            fn render_help(&self, c: &Command) -> String {
                render_help(c).to_uppercase()
            }
            fn render_markdown(&self, c: &Command) -> String {
                render_markdown(c)
            }
            fn render_subcommand_list(&self, cs: &[Command]) -> String {
                render_subcommand_list(cs)
            }
            fn render_ambiguity(&self, i: &str, cs: &[String]) -> String {
                render_ambiguity(i, cs)
            }
        }
        let cli = crate::cli::Cli::new(vec![Command::builder("ping").build().unwrap()])
            .with_renderer(Upper);
        // run with --help; output should be uppercase
        let _ = cli.run(["--help"]);
    }
}
