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
//! - **[`render_docs`]** — produces a full Markdown reference document for all
//!   commands in a [`crate::query::Registry`], with a table of contents and
//!   per-command sections separated by `---`.
//!
//! None of the functions print to stdout/stderr directly; all return a
//! `String` that the caller can write wherever appropriate.

use crate::model::Command;

/// A supported shell for completion script generation.
///
/// Used with [`render_completion`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shell {
    /// Bash (Bourne Again Shell)
    Bash,
    /// Zsh (Z Shell)
    Zsh,
    /// Fish shell
    Fish,
}

/// A pluggable renderer for command help, Markdown docs, and disambiguation messages.
///
/// Implement this trait to fully customize how argot formats its output.
/// Use [`crate::Cli::with_renderer`] to inject your implementation.
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
    /// Render a full Markdown reference document for all commands in a registry.
    ///
    /// Produces a `# Commands` heading, a table of contents with depth-based
    /// indentation, and per-command Markdown sections separated by `---`.
    fn render_docs(&self, registry: &crate::query::Registry) -> String {
        render_docs(registry)
    }
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
    fn render_docs(&self, registry: &crate::query::Registry) -> String {
        render_docs(registry)
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

/// Generate a shell completion script for a registry of commands.
///
/// The generated script hooks into the shell's native completion mechanism.
/// Source it in your shell profile to enable tab-completion for your tool.
///
/// # Arguments
///
/// - `shell` — the target shell
/// - `program` — the program name as it appears in `PATH` (e.g. `"mytool"`)
/// - `registry` — the [`crate::query::Registry`] containing all commands
///
/// # Examples
///
/// ```
/// # use argot::{Command, Flag, Registry};
/// # use argot::render::{Shell, render_completion};
/// let registry = Registry::new(vec![
///     Command::builder("deploy")
///         .flag(Flag::builder("env").takes_value().choices(["prod", "staging"]).build().unwrap())
///         .build().unwrap(),
///     Command::builder("status").build().unwrap(),
/// ]);
///
/// let script = render_completion(Shell::Bash, "mytool", &registry);
/// assert!(script.contains("mytool"));
/// assert!(script.contains("deploy"));
/// assert!(script.contains("status"));
/// ```
pub fn render_completion(shell: Shell, program: &str, registry: &crate::query::Registry) -> String {
    match shell {
        Shell::Bash => render_completion_bash(program, registry),
        Shell::Zsh => render_completion_zsh(program, registry),
        Shell::Fish => render_completion_fish(program, registry),
    }
}

fn render_completion_bash(program: &str, registry: &crate::query::Registry) -> String {
    let func_name = format!("_{}_completions", program.replace('-', "_"));

    // Collect: top-level command names
    let top_level: Vec<&str> = registry
        .commands()
        .iter()
        .map(|c| c.canonical.as_str())
        .collect();

    // Build per-command flag completions
    let mut cmd_cases = String::new();
    for entry in registry.iter_all_recursive() {
        let cmd = entry.command;
        let flags: Vec<String> = cmd.flags.iter().map(|f| format!("--{}", f.name)).collect();
        if !flags.is_empty() {
            let path_str = entry.path_str();
            cmd_cases.push_str(&format!(
                "        {})\n            COMPREPLY=($(compgen -W \"{}\" -- \"$cur\"))\n            return\n            ;;\n",
                path_str,
                flags.join(" ")
            ));
        }
    }

    format!(
        r#"# {program} bash completion
# Source this file or add to ~/.bashrc:
#   source <({program} completion bash)

{func_name}() {{
    local cur prev words cword
    _init_completion 2>/dev/null || {{
        cur="${{COMP_WORDS[COMP_CWORD]}}"
        prev="${{COMP_WORDS[COMP_CWORD-1]}}"
    }}

    local cmd="${{COMP_WORDS[1]}}"

    case "$cmd" in
{cmd_cases}        *)
            COMPREPLY=($(compgen -W "{top}" -- "$cur"))
            ;;
    esac
}}

complete -F {func_name} {program}
"#,
        program = program,
        func_name = func_name,
        cmd_cases = cmd_cases,
        top = top_level.join(" "),
    )
}

fn render_completion_zsh(program: &str, registry: &crate::query::Registry) -> String {
    let mut commands_block = String::new();
    for cmd in registry.commands() {
        let desc = if cmd.summary.is_empty() {
            &cmd.canonical
        } else {
            &cmd.summary
        };
        commands_block.push_str(&format!("    '{}:{}'\n", cmd.canonical, desc));
    }

    let mut subcommand_cases = String::new();
    for entry in registry.iter_all_recursive() {
        let cmd = entry.command;
        if cmd.flags.is_empty() && cmd.arguments.is_empty() {
            continue;
        }
        let mut args_spec = String::new();
        for flag in &cmd.flags {
            let desc = if flag.description.is_empty() {
                flag.name.as_str()
            } else {
                flag.description.as_str()
            };
            if flag.takes_value {
                args_spec.push_str(&format!("    '--{}[{}]:value:_default'\n", flag.name, desc));
            } else {
                args_spec.push_str(&format!("    '--{}[{}]'\n", flag.name, desc));
            }
        }
        let path_str = entry.path_str().replace('.', "-");
        subcommand_cases.push_str(&format!(
            "  ({path})\n    _arguments \\\n{args}  ;;\n",
            path = path_str,
            args = args_spec,
        ));
    }

    format!(
        r#"#compdef {program}
# {program} zsh completion

_{program}() {{
  local state

  _arguments \
    '1: :{program}_commands' \
    '*:: :->subcommand'

  case $state in
    subcommand)
      case $words[1] in
{subcases}      esac
  esac
}}

_{program}_commands() {{
  local -a commands
  commands=(
{cmds}  )
  _describe 'command' commands
}}

_{program}
"#,
        program = program,
        subcases = subcommand_cases,
        cmds = commands_block,
    )
}

fn render_completion_fish(program: &str, registry: &crate::query::Registry) -> String {
    let mut lines = format!(
        "# {program} fish completion\n# Add to ~/.config/fish/completions/{program}.fish\n\n"
    );

    // Top-level commands
    for cmd in registry.commands() {
        let desc = if cmd.summary.is_empty() {
            String::new()
        } else {
            format!(" -d '{}'", cmd.summary.replace('\'', "\\'"))
        };
        lines.push_str(&format!(
            "complete -c {program} -f -n '__fish_use_subcommand' -a '{}'{}\n",
            cmd.canonical, desc
        ));
    }

    lines.push('\n');

    // Per-command flags
    for entry in registry.iter_all_recursive() {
        let cmd = entry.command;
        let subcmd = &cmd.canonical;
        for flag in &cmd.flags {
            let desc = if flag.description.is_empty() {
                String::new()
            } else {
                format!(" -d '{}'", flag.description.replace('\'', "\\'"))
            };
            let req = if flag.takes_value { " -r" } else { "" };
            lines.push_str(&format!(
                "complete -c {program} -n '__fish_seen_subcommand_from {subcmd}' -l '{name}'{req}{desc}\n",
                program = program,
                subcmd = subcmd,
                name = flag.name,
                req = req,
                desc = desc,
            ));
        }
    }

    lines
}

/// Generate a JSON Schema (draft-07) describing the inputs for a command.
///
/// The schema object is suitable for use in agent tool definitions (e.g.
/// OpenAI function calling, Anthropic tool use, MCP tool input schemas).
///
/// Arguments appear as required string properties (with `"required"` if marked
/// so). Flags with [`crate::model::Flag::takes_value`] become string properties;
/// boolean flags become boolean properties.
///
/// # Examples
///
/// ```
/// # use argot::{Argument, Command, Flag};
/// # use argot::render::render_json_schema;
/// let cmd = Command::builder("deploy")
///     .summary("Deploy a service")
///     .argument(Argument::builder("env").required().description("Target environment").build().unwrap())
///     .flag(Flag::builder("dry-run").description("Simulate only").build().unwrap())
///     .flag(Flag::builder("strategy")
///         .takes_value()
///         .choices(["rolling", "blue-green"])
///         .description("Rollout strategy")
///         .build().unwrap())
///     .build().unwrap();
///
/// let schema = render_json_schema(&cmd).unwrap();
/// let v: serde_json::Value = serde_json::from_str(&schema).unwrap();
/// assert_eq!(v["title"], "deploy");
/// assert_eq!(v["properties"]["env"]["type"], "string");
/// assert_eq!(v["properties"]["dry-run"]["type"], "boolean");
/// let strats = v["properties"]["strategy"]["enum"].as_array().unwrap();
/// assert_eq!(strats.len(), 2);
/// ```
pub fn render_json_schema(command: &Command) -> Result<String, serde_json::Error> {
    use serde_json::{json, Map, Value};

    let mut properties: Map<String, Value> = Map::new();
    let mut required: Vec<Value> = Vec::new();

    // Positional arguments → string properties
    for arg in &command.arguments {
        let mut prop = json!({
            "type": "string",
        });
        if !arg.description.is_empty() {
            prop["description"] = json!(arg.description);
        }
        if arg.variadic {
            prop = json!({
                "type": "array",
                "items": { "type": "string" },
            });
            if !arg.description.is_empty() {
                prop["description"] = json!(arg.description);
            }
        }
        if arg.required {
            required.push(json!(arg.name));
        }
        if let Some(ref default) = arg.default {
            prop["default"] = json!(default);
        }
        properties.insert(arg.name.clone(), prop);
    }

    // Flags → typed properties
    for flag in &command.flags {
        let mut prop: Map<String, Value> = Map::new();

        if !flag.description.is_empty() {
            prop.insert("description".into(), json!(flag.description));
        }

        if flag.takes_value {
            if let Some(ref choices) = flag.choices {
                prop.insert("type".into(), json!("string"));
                prop.insert(
                    "enum".into(),
                    Value::Array(choices.iter().map(|c| json!(c)).collect()),
                );
            } else {
                prop.insert("type".into(), json!("string"));
            }
            if let Some(ref default) = flag.default {
                prop.insert("default".into(), json!(default));
            }
        } else {
            // Boolean flag
            prop.insert("type".into(), json!("boolean"));
            prop.insert("default".into(), json!(false));
        }

        if flag.required {
            required.push(json!(flag.name));
        }

        properties.insert(flag.name.clone(), Value::Object(prop));
    }

    let mut schema = json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": command.canonical,
        "type": "object",
        "properties": properties,
    });

    if !command.summary.is_empty() {
        schema["description"] = json!(command.summary);
    }

    if !required.is_empty() {
        schema["required"] = Value::Array(required);
    }

    serde_json::to_string_pretty(&schema)
}

/// Render a full Markdown reference document for all commands in a registry.
///
/// The output contains:
/// - A `# Commands` top-level heading.
/// - A table of contents: a bulleted list of anchor links to each command in
///   depth-first order. Subcommands are indented by two spaces per level beyond
///   the first.
/// - Each command rendered with [`render_markdown`], separated by `---` lines.
///
/// # Arguments
///
/// - `registry` — The registry whose commands should be documented.
///
/// # Examples
///
/// ```
/// # use argot::{Command, Registry, render_docs};
/// let registry = Registry::new(vec![
///     Command::builder("deploy")
///         .summary("Deploy the application")
///         .subcommand(Command::builder("rollback").summary("Roll back").build().unwrap())
///         .build()
///         .unwrap(),
///     Command::builder("status").summary("Show status").build().unwrap(),
/// ]);
///
/// let docs = render_docs(&registry);
/// assert!(docs.contains("# Commands"));
/// assert!(docs.contains("# deploy"));
/// assert!(docs.contains("# rollback"));
/// assert!(docs.contains("# status"));
/// assert!(docs.contains("---"));
/// ```
pub fn render_docs(registry: &crate::query::Registry) -> String {
    let entries = registry.iter_all_recursive();

    let mut out = String::from("# Commands\n\n");

    // Table of contents
    for entry in &entries {
        let depth = entry.path.len();
        let indent = "  ".repeat(depth.saturating_sub(1));
        let anchor = entry.path_str().replace('.', "-").to_lowercase();
        let label = entry.path_str().replace('.', " ");
        out.push_str(&format!("{}- [{}](#{})\n", indent, label, anchor));
    }

    // Per-command sections
    for (i, entry) in entries.iter().enumerate() {
        out.push_str("\n---\n\n");
        out.push_str(&render_markdown(entry.command));
        let _ = i; // suppress unused variable warning
    }

    out
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

    #[test]
    fn test_render_completion_bash_contains_program() {
        use crate::query::Registry;
        let reg = Registry::new(vec![
            Command::builder("deploy").build().unwrap(),
            Command::builder("status").build().unwrap(),
        ]);
        let script = render_completion(Shell::Bash, "mytool", &reg);
        assert!(script.contains("mytool"));
        assert!(script.contains("deploy"));
        assert!(script.contains("status"));
    }

    #[test]
    fn test_render_completion_zsh_contains_program() {
        use crate::query::Registry;
        let reg = Registry::new(vec![Command::builder("run").build().unwrap()]);
        let script = render_completion(Shell::Zsh, "mytool", &reg);
        assert!(script.contains("mytool") && script.contains("run"));
    }

    #[test]
    fn test_render_completion_fish_contains_program() {
        use crate::query::Registry;
        let reg = Registry::new(vec![Command::builder("run").build().unwrap()]);
        let script = render_completion(Shell::Fish, "mytool", &reg);
        assert!(script.contains("mytool") && script.contains("run"));
    }

    #[test]
    fn test_render_completion_bash_includes_flags() {
        use crate::query::Registry;
        let reg = Registry::new(vec![Command::builder("deploy")
            .flag(Flag::builder("env").takes_value().build().unwrap())
            .flag(Flag::builder("dry-run").build().unwrap())
            .build()
            .unwrap()]);
        let script = render_completion(Shell::Bash, "t", &reg);
        assert!(script.contains("--env"));
        assert!(script.contains("--dry-run"));
    }

    #[test]
    fn test_render_json_schema_properties() {
        let cmd = Command::builder("deploy")
            .summary("Deploy a service")
            .argument(
                Argument::builder("env")
                    .required()
                    .description("Target env")
                    .build()
                    .unwrap(),
            )
            .flag(
                Flag::builder("dry-run")
                    .description("Simulate")
                    .build()
                    .unwrap(),
            )
            .flag(
                Flag::builder("strategy")
                    .takes_value()
                    .choices(["rolling", "canary"])
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();

        let schema = render_json_schema(&cmd).unwrap();
        let v: serde_json::Value = serde_json::from_str(&schema).unwrap();

        assert_eq!(v["title"], "deploy");
        assert_eq!(v["description"], "Deploy a service");
        assert_eq!(v["properties"]["env"]["type"], "string");
        assert_eq!(v["properties"]["dry-run"]["type"], "boolean");
        assert_eq!(v["properties"]["strategy"]["type"], "string");
        assert_eq!(v["properties"]["strategy"]["enum"][0], "rolling");
        let req = v["required"].as_array().unwrap();
        assert!(req.contains(&serde_json::json!("env")));
    }

    #[test]
    fn test_render_json_schema_empty_command() {
        let cmd = Command::builder("ping").build().unwrap();
        let schema = render_json_schema(&cmd).unwrap();
        let v: serde_json::Value = serde_json::from_str(&schema).unwrap();
        assert_eq!(v["title"], "ping");
        assert!(
            v["required"].is_null()
                || v["required"]
                    .as_array()
                    .map(|a| a.is_empty())
                    .unwrap_or(true)
        );
    }

    #[test]
    fn test_render_json_schema_returns_result() {
        let cmd = Command::builder("ping").build().unwrap();
        // Must return Ok, not panic.
        let result = render_json_schema(&cmd);
        assert!(result.is_ok());
        let _: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
    }

    #[test]
    fn test_spellings_not_in_help_output() {
        let cmd = Command::builder("deploy")
            .alias("release")
            .spelling("deply")
            .build()
            .unwrap();

        let help = render_help(&cmd);
        assert!(help.contains("release"), "alias should appear in help");
        assert!(!help.contains("deply"), "spelling must not appear in help");
    }

    #[test]
    fn test_semantic_aliases_not_in_help_output() {
        let cmd = Command::builder("deploy")
            .alias("d")
            .semantic_alias("release to production")
            .semantic_alias("push to environment")
            .summary("Deploy a service")
            .build()
            .unwrap();

        let help = render_help(&cmd);
        assert!(help.contains("d"), "alias should appear in help");
        assert!(
            !help.contains("release to production"),
            "semantic alias must not appear in help"
        );
        assert!(
            !help.contains("push to environment"),
            "semantic alias must not appear in help"
        );
    }

    fn docs_registry() -> crate::query::Registry {
        use crate::query::Registry;
        Registry::new(vec![
            Command::builder("deploy")
                .summary("Deploy the application")
                .subcommand(
                    Command::builder("rollback")
                        .summary("Roll back a deployment")
                        .build()
                        .unwrap(),
                )
                .build()
                .unwrap(),
            Command::builder("status")
                .summary("Show status")
                .build()
                .unwrap(),
        ])
    }

    #[test]
    fn test_render_docs_contains_all_commands() {
        let reg = docs_registry();
        let docs = render_docs(&reg);
        assert!(docs.contains("# Commands"), "missing top-level heading");
        assert!(docs.contains("deploy"), "missing deploy");
        assert!(docs.contains("rollback"), "missing rollback");
        assert!(docs.contains("status"), "missing status");
        assert!(docs.contains("---"), "missing separator");
    }

    #[test]
    fn test_render_docs_table_of_contents_indents_subcommands() {
        let reg = docs_registry();
        let docs = render_docs(&reg);
        // "deploy" at top level — no leading spaces before the bullet
        assert!(
            docs.contains("\n- [deploy](#deploy)"),
            "deploy should be at root indent"
        );
        // "deploy rollback" at depth 2 — two leading spaces
        assert!(
            docs.contains("\n  - [deploy rollback](#deploy-rollback)"),
            "deploy rollback should be indented"
        );
        // "status" at top level
        assert!(
            docs.contains("\n- [status](#status)"),
            "status should be at root indent"
        );
    }

    #[test]
    fn test_render_docs_empty_registry() {
        use crate::query::Registry;
        let reg = Registry::new(vec![]);
        let docs = render_docs(&reg);
        assert!(docs.starts_with("# Commands\n\n"));
        // Should not panic and should not contain any separator (no commands)
        assert!(!docs.contains("---"));
    }

    #[test]
    fn test_default_renderer_render_docs() {
        let reg = docs_registry();
        let renderer = DefaultRenderer;
        let docs = renderer.render_docs(&reg);
        assert!(docs.contains("# Commands"));
        assert!(docs.contains("deploy"));
        assert!(docs.contains("status"));
    }

    #[test]
    fn test_render_completion_zsh_with_flags_and_args() {
        use crate::query::Registry;
        let reg = Registry::new(vec![
            Command::builder("deploy")
                .summary("Deploy")
                .flag(
                    Flag::builder("env")
                        .takes_value()
                        .description("target env")
                        .build()
                        .unwrap(),
                )
                .flag(Flag::builder("dry-run").description("simulate").build().unwrap())
                .argument(Argument::builder("service").required().build().unwrap())
                .build()
                .unwrap(),
            // A command with no flags/args (should be skipped in subcommand_cases)
            Command::builder("status").build().unwrap(),
        ]);
        let script = render_completion(Shell::Zsh, "mytool", &reg);
        assert!(script.contains("mytool"));
        assert!(script.contains("deploy"));
        assert!(script.contains("--env"));
        assert!(script.contains("--dry-run"));
    }

    #[test]
    fn test_render_completion_zsh_empty_summary_uses_canonical() {
        use crate::query::Registry;
        // A command with no summary should use the canonical name in the description
        let reg = Registry::new(vec![Command::builder("run").build().unwrap()]);
        let script = render_completion(Shell::Zsh, "mytool", &reg);
        // canonical name used since summary is empty
        assert!(script.contains("run:run"));
    }

    #[test]
    fn test_render_completion_fish_with_flags() {
        use crate::query::Registry;
        let reg = Registry::new(vec![Command::builder("deploy")
            .summary("Deploy the app")
            .flag(
                Flag::builder("env")
                    .takes_value()
                    .description("target environment")
                    .build()
                    .unwrap(),
            )
            .flag(Flag::builder("dry-run").description("simulate").build().unwrap())
            .build()
            .unwrap()]);
        let script = render_completion(Shell::Fish, "mytool", &reg);
        assert!(script.contains("mytool"));
        assert!(script.contains("deploy"));
        assert!(script.contains("--env") || script.contains("'env'"));
        // Flag with takes_value should have -r
        assert!(script.contains("-r"));
        // Summary should be in description
        assert!(script.contains("Deploy the app"));
    }

    #[test]
    fn test_render_completion_fish_empty_summary() {
        use crate::query::Registry;
        let reg = Registry::new(vec![Command::builder("run").build().unwrap()]);
        let script = render_completion(Shell::Fish, "mytool", &reg);
        // Empty summary → no -d '...' in the line for the command
        assert!(script.contains("run"));
    }

    #[test]
    fn test_render_completion_bash_no_flags_cmd() {
        use crate::query::Registry;
        // Command without flags should still appear in the top-level list
        let reg = Registry::new(vec![Command::builder("status").build().unwrap()]);
        let script = render_completion(Shell::Bash, "app", &reg);
        assert!(script.contains("status"));
    }

    #[test]
    fn test_render_json_schema_variadic_arg() {
        let cmd = Command::builder("run")
            .argument(
                Argument::builder("files")
                    .variadic()
                    .description("Files to process")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        let schema = render_json_schema(&cmd).unwrap();
        let v: serde_json::Value = serde_json::from_str(&schema).unwrap();
        assert_eq!(v["properties"]["files"]["type"], "array");
        assert_eq!(v["properties"]["files"]["items"]["type"], "string");
        assert!(v["properties"]["files"]["description"].as_str().is_some());
    }

    #[test]
    fn test_render_json_schema_flag_with_default() {
        let cmd = Command::builder("run")
            .flag(
                Flag::builder("output")
                    .takes_value()
                    .default_value("text")
                    .description("Output format")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        let schema = render_json_schema(&cmd).unwrap();
        let v: serde_json::Value = serde_json::from_str(&schema).unwrap();
        assert_eq!(v["properties"]["output"]["default"], "text");
        assert_eq!(v["properties"]["output"]["type"], "string");
    }

    #[test]
    fn test_render_json_schema_required_flag() {
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
        let schema = render_json_schema(&cmd).unwrap();
        let v: serde_json::Value = serde_json::from_str(&schema).unwrap();
        let req = v["required"].as_array().unwrap();
        assert!(req.contains(&serde_json::json!("env")));
    }

    #[test]
    fn test_render_json_schema_arg_with_default() {
        let cmd = Command::builder("run")
            .argument(
                Argument::builder("target")
                    .default_value("prod")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        let schema = render_json_schema(&cmd).unwrap();
        let v: serde_json::Value = serde_json::from_str(&schema).unwrap();
        assert_eq!(v["properties"]["target"]["default"], "prod");
    }

    #[test]
    fn test_render_help_output_in_example() {
        // Example with output should show "# Output:" line
        let cmd = Command::builder("run")
            .example(
                Example::new("Run example", "myapp run")
                    .with_output("OK"),
            )
            .build()
            .unwrap();
        let help = render_help(&cmd);
        assert!(help.contains("# Output: OK"));
    }

    #[test]
    fn test_render_markdown_with_best_practices_and_anti_patterns() {
        let cmd = Command::builder("deploy")
            .best_practice("Always dry-run first")
            .anti_pattern("Deploy on Fridays")
            .build()
            .unwrap();
        let md = render_markdown(&cmd);
        assert!(md.contains("## Best Practices"));
        assert!(md.contains("Always dry-run first"));
        assert!(md.contains("## Anti-Patterns"));
        assert!(md.contains("Deploy on Fridays"));
    }

    #[test]
    fn test_render_markdown_with_subcommands() {
        let cmd = Command::builder("remote")
            .subcommand(Command::builder("add").summary("Add remote").build().unwrap())
            .build()
            .unwrap();
        let md = render_markdown(&cmd);
        assert!(md.contains("## Subcommands"));
        assert!(md.contains("**add**"));
    }
}
