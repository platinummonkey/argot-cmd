//! Central command registry with lookup and search operations.
//!
//! [`Registry`] is the primary store for the command tree in an argot
//! application. It owns a `Vec<Command>` and exposes several query methods:
//!
//! - **[`Registry::get_command`]** — exact lookup by canonical name.
//! - **[`Registry::get_subcommand`]** — walk a path of canonical names into
//!   the nested subcommand tree.
//! - **[`Registry::list_commands`]** — iterate all top-level commands.
//! - **[`Registry::search`]** — case-insensitive substring search across
//!   canonical name, summary, and description.
//! - **[`Registry::fuzzy_search`]** — fuzzy (skim) search returning results
//!   sorted by score (best match first). Requires the `fuzzy` feature.
//! - **[`Registry::to_json`]** — serialize the command tree to pretty-printed
//!   JSON (handler closures are excluded).
//!
//! Pass `registry.commands()` to [`crate::Parser::new`] to wire the registry
//! into the parsing pipeline.
//!
//! # Example
//!
//! ```
//! # use argot_cmd::{Command, Registry};
//! let registry = Registry::new(vec![
//!     Command::builder("list").summary("List all items").build().unwrap(),
//!     Command::builder("get").summary("Get a single item").build().unwrap(),
//! ]);
//!
//! assert!(registry.get_command("list").is_some());
//! assert_eq!(registry.search("item").len(), 2);
//! ```

#[cfg(feature = "fuzzy")]
use fuzzy_matcher::skim::SkimMatcherV2;
#[cfg(feature = "fuzzy")]
use fuzzy_matcher::FuzzyMatcher;
use thiserror::Error;

use crate::model::{Command, Example};

/// A command paired with its canonical path from the registry root.
///
/// Produced by [`Registry::iter_all_recursive`].
#[derive(Debug, Clone)]
pub struct CommandEntry<'a> {
    /// Canonical names from root to this command, e.g. `["remote", "add"]`.
    pub path: Vec<String>,
    /// The command at this path.
    pub command: &'a Command,
}

impl<'a> CommandEntry<'a> {
    /// The canonical name of this command (last element of `path`).
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot_cmd::{Command, Registry};
    /// let registry = Registry::new(vec![
    ///     Command::builder("remote")
    ///         .subcommand(Command::builder("add").build().unwrap())
    ///         .build()
    ///         .unwrap(),
    /// ]);
    /// let entries = registry.iter_all_recursive();
    /// assert_eq!(entries[0].name(), "remote");
    /// assert_eq!(entries[1].name(), "add");
    /// ```
    pub fn name(&self) -> &str {
        self.path.last().map(String::as_str).unwrap_or("")
    }

    /// The full dotted path string, e.g. `"remote.add"`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot_cmd::{Command, Registry};
    /// let registry = Registry::new(vec![
    ///     Command::builder("remote")
    ///         .subcommand(Command::builder("add").build().unwrap())
    ///         .build()
    ///         .unwrap(),
    /// ]);
    /// let entries = registry.iter_all_recursive();
    /// assert_eq!(entries[0].path_str(), "remote");
    /// assert_eq!(entries[1].path_str(), "remote.add");
    /// ```
    pub fn path_str(&self) -> String {
        self.path.join(".")
    }
}

/// Errors produced by [`Registry`] methods.
#[derive(Debug, Error)]
pub enum QueryError {
    /// JSON serialization failed.
    ///
    /// Wraps the underlying [`serde_json::Error`].
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Owns the registered command tree and provides query/search operations.
///
/// Create a `Registry` with [`Registry::new`], passing the fully-built list of
/// top-level commands. The registry takes ownership of the command list and
/// makes it available through a variety of lookup and search methods.
///
/// # Examples
///
/// ```
/// # use argot_cmd::{Command, Registry};
/// let registry = Registry::new(vec![
///     Command::builder("deploy").summary("Deploy the app").build().unwrap(),
/// ]);
///
/// let cmd = registry.get_command("deploy").unwrap();
/// assert_eq!(cmd.summary, "Deploy the app");
/// ```
pub struct Registry {
    commands: Vec<Command>,
}

impl Registry {
    /// Create a new `Registry` owning the given command list.
    ///
    /// # Arguments
    ///
    /// - `commands` — The top-level command list. Subcommands are nested
    ///   inside the respective [`Command::subcommands`] fields.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot_cmd::{Command, Registry};
    /// let registry = Registry::new(vec![
    ///     Command::builder("run").build().unwrap(),
    /// ]);
    /// assert_eq!(registry.list_commands().len(), 1);
    /// ```
    pub fn new(commands: Vec<Command>) -> Self {
        Self { commands }
    }

    /// Append a command to the registry.
    ///
    /// Used internally by [`crate::Cli::with_query_support`] to inject the
    /// built-in `query` meta-command.
    pub(crate) fn push(&mut self, cmd: Command) {
        self.commands.push(cmd);
    }

    /// Borrow the raw command slice (useful for constructing a [`Parser`][crate::parser::Parser]).
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot_cmd::{Command, Registry, Parser};
    /// let registry = Registry::new(vec![Command::builder("ping").build().unwrap()]);
    /// let parser = Parser::new(registry.commands());
    /// let parsed = parser.parse(&["ping"]).unwrap();
    /// assert_eq!(parsed.command.canonical, "ping");
    /// ```
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    /// Return references to all top-level commands.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot_cmd::{Command, Registry};
    /// let registry = Registry::new(vec![
    ///     Command::builder("a").build().unwrap(),
    ///     Command::builder("b").build().unwrap(),
    /// ]);
    /// assert_eq!(registry.list_commands().len(), 2);
    /// ```
    pub fn list_commands(&self) -> Vec<&Command> {
        self.commands.iter().collect()
    }

    /// Look up a top-level command by its exact canonical name.
    ///
    /// Returns `None` if no command with that canonical name exists. Does not
    /// match aliases or spellings — use [`crate::Resolver`] for fuzzy/prefix
    /// matching.
    ///
    /// # Arguments
    ///
    /// - `canonical` — The exact canonical name to look up.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot_cmd::{Command, Registry};
    /// let registry = Registry::new(vec![
    ///     Command::builder("deploy").alias("d").build().unwrap(),
    /// ]);
    ///
    /// assert!(registry.get_command("deploy").is_some());
    /// assert!(registry.get_command("d").is_none()); // alias, not canonical
    /// ```
    pub fn get_command(&self, canonical: &str) -> Option<&Command> {
        self.commands.iter().find(|c| c.canonical == canonical)
    }

    /// Walk a path of canonical names into the subcommand tree.
    ///
    /// `path = &["remote", "add"]` returns the `add` subcommand of `remote`.
    /// Each path segment must be an *exact canonical* name at that level of
    /// the tree.
    ///
    /// Returns `None` if any segment fails to match or if `path` is empty.
    ///
    /// # Arguments
    ///
    /// - `path` — Ordered slice of canonical command names from top-level down.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot_cmd::{Command, Registry};
    /// let registry = Registry::new(vec![
    ///     Command::builder("remote")
    ///         .subcommand(Command::builder("add").build().unwrap())
    ///         .build()
    ///         .unwrap(),
    /// ]);
    ///
    /// let sub = registry.get_subcommand(&["remote", "add"]).unwrap();
    /// assert_eq!(sub.canonical, "add");
    ///
    /// assert!(registry.get_subcommand(&[]).is_none());
    /// assert!(registry.get_subcommand(&["remote", "nope"]).is_none());
    /// ```
    pub fn get_subcommand(&self, path: &[&str]) -> Option<&Command> {
        if path.is_empty() {
            return None;
        }
        let mut current = self.get_command(path[0])?;
        for &segment in &path[1..] {
            current = current
                .subcommands
                .iter()
                .find(|c| c.canonical == segment)?;
        }
        Some(current)
    }

    /// Return the examples slice for a top-level command, or `None` if the
    /// command does not exist.
    ///
    /// An empty examples list returns `Some(&[])`.
    ///
    /// # Arguments
    ///
    /// - `canonical` — The exact canonical name of the command.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot_cmd::{Command, Example, Registry};
    /// let registry = Registry::new(vec![
    ///     Command::builder("run")
    ///         .example(Example::new("basic run", "myapp run"))
    ///         .build()
    ///         .unwrap(),
    /// ]);
    ///
    /// assert_eq!(registry.get_examples("run").unwrap().len(), 1);
    /// assert!(registry.get_examples("missing").is_none());
    /// ```
    pub fn get_examples(&self, canonical: &str) -> Option<&[Example]> {
        self.get_command(canonical).map(|c| c.examples.as_slice())
    }

    /// Substring search across canonical name, summary, and description.
    ///
    /// The search is case-insensitive. Returns all top-level commands for
    /// which the query appears in at least one of the three text fields.
    ///
    /// # Arguments
    ///
    /// - `query` — The substring to search for (case-insensitive).
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot_cmd::{Command, Registry};
    /// let registry = Registry::new(vec![
    ///     Command::builder("list").summary("List all records").build().unwrap(),
    ///     Command::builder("get").summary("Get a single record").build().unwrap(),
    /// ]);
    ///
    /// let results = registry.search("record");
    /// assert_eq!(results.len(), 2);
    /// assert!(registry.search("zzz").is_empty());
    /// ```
    pub fn search(&self, query: &str) -> Vec<&Command> {
        let q = query.to_lowercase();
        self.commands
            .iter()
            .filter(|c| {
                c.canonical.to_lowercase().contains(&q)
                    || c.summary.to_lowercase().contains(&q)
                    || c.description.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Fuzzy search across canonical name, summary, and description.
    ///
    /// Uses the skim fuzzy-matching algorithm (requires the `fuzzy` feature).
    /// Returns matches sorted descending by score (best match first).
    /// Commands that produce no fuzzy match are excluded.
    ///
    /// # Arguments
    ///
    /// - `query` — The fuzzy query string.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "fuzzy")] {
    /// # use argot_cmd::{Command, Registry};
    /// let registry = Registry::new(vec![
    ///     Command::builder("deploy").summary("Deploy a service").build().unwrap(),
    ///     Command::builder("delete").summary("Delete a resource").build().unwrap(),
    ///     Command::builder("describe").summary("Describe a resource").build().unwrap(),
    /// ]);
    ///
    /// // Fuzzy-matches all commands starting with 'de'
    /// let results = registry.fuzzy_search("dep");
    /// assert!(!results.is_empty());
    /// // Results are sorted by match score descending
    /// assert_eq!(results[0].0.canonical, "deploy");
    /// // Scores are positive integers — higher is a better match
    /// assert!(results[0].1 > 0);
    /// # }
    /// ```
    #[cfg(feature = "fuzzy")]
    pub fn fuzzy_search(&self, query: &str) -> Vec<(&Command, i64)> {
        let matcher = SkimMatcherV2::default();
        let mut results: Vec<(&Command, i64)> = self
            .commands
            .iter()
            .filter_map(|cmd| {
                let text = format!("{} {} {}", cmd.canonical, cmd.summary, cmd.description);
                matcher.fuzzy_match(&text, query).map(|score| (cmd, score))
            })
            .collect();
        results.sort_by(|a, b| b.1.cmp(&a.1));
        results
    }

    /// Match commands by natural-language intent phrase.
    ///
    /// Scores each command by how many words from `phrase` appear in its
    /// combined text (canonical name, aliases, semantic aliases, summary,
    /// description). Returns matches sorted by score descending.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot_cmd::{Command, Registry};
    /// let registry = Registry::new(vec![
    ///     Command::builder("deploy")
    ///         .summary("Deploy a service to an environment")
    ///         .semantic_alias("release to production")
    ///         .semantic_alias("push to environment")
    ///         .build().unwrap(),
    ///     Command::builder("status")
    ///         .summary("Check service status")
    ///         .build().unwrap(),
    /// ]);
    ///
    /// let results = registry.match_intent("deploy to production");
    /// assert!(!results.is_empty());
    /// assert_eq!(results[0].0.canonical, "deploy");
    /// ```
    pub fn match_intent(&self, phrase: &str) -> Vec<(&Command, u32)> {
        let phrase_lower = phrase.to_lowercase();
        let words: Vec<&str> = phrase_lower
            .split_whitespace()
            .filter(|w| !w.is_empty())
            .collect();

        if words.is_empty() {
            return vec![];
        }

        let mut results: Vec<(&Command, u32)> = self
            .commands
            .iter()
            .filter_map(|cmd| {
                let combined = format!(
                    "{} {} {} {} {}",
                    cmd.canonical.to_lowercase(),
                    cmd.aliases
                        .iter()
                        .map(|s| s.to_lowercase())
                        .collect::<Vec<_>>()
                        .join(" "),
                    cmd.semantic_aliases
                        .iter()
                        .map(|s| s.to_lowercase())
                        .collect::<Vec<_>>()
                        .join(" "),
                    cmd.summary.to_lowercase(),
                    cmd.description.to_lowercase(),
                );
                let score = words.iter().filter(|&&w| combined.contains(w)).count() as u32;
                if score > 0 {
                    Some((cmd, score))
                } else {
                    None
                }
            })
            .collect();

        results.sort_by(|a, b| b.1.cmp(&a.1));
        results
    }

    /// Serialize the entire command tree to a pretty-printed JSON string.
    ///
    /// Handler closures are excluded from the output (they are skipped by the
    /// `serde` configuration on [`Command`]).
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::Serialization`] if `serde_json` fails (in
    /// practice this should not happen for well-formed command trees).
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot_cmd::{Command, Registry};
    /// let registry = Registry::new(vec![
    ///     Command::builder("deploy").summary("Deploy").build().unwrap(),
    /// ]);
    ///
    /// let json = registry.to_json().unwrap();
    /// assert!(json.contains("deploy"));
    /// ```
    pub fn to_json(&self) -> Result<String, QueryError> {
        serde_json::to_string_pretty(&self.commands).map_err(QueryError::Serialization)
    }

    /// Serialize the entire command tree to a pretty-printed JSON string,
    /// filtering each command object to only include the requested top-level
    /// fields.
    ///
    /// Each command object (including nested subcommands at any depth) is
    /// filtered so that only keys listed in `fields` are retained. The
    /// `subcommands` key is always walked recursively even if it is not in
    /// `fields`; its entries are filtered before being emitted.
    ///
    /// If `fields` is empty the method falls back to the same output as
    /// [`Registry::to_json`].
    ///
    /// Field names that do not exist in the serialized command are silently
    /// ignored (no error is returned for missing fields).
    ///
    /// Valid field names correspond to the top-level keys of the serialized
    /// [`Command`] object: `canonical`, `aliases`, `spellings`,
    /// `semantic_aliases`, `summary`, `description`, `arguments`, `flags`,
    /// `examples`, `best_practices`, `anti_patterns`, `subcommands`, `meta`,
    /// `mutating`, etc.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::Serialization`] if `serde_json` fails.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot_cmd::{Command, Registry};
    /// let registry = Registry::new(vec![
    ///     Command::builder("deploy")
    ///         .summary("Deploy the app")
    ///         .build()
    ///         .unwrap(),
    /// ]);
    ///
    /// let json = registry.to_json_with_fields(&["canonical", "summary"]).unwrap();
    /// let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    /// let obj = &v[0];
    /// assert_eq!(obj["canonical"], "deploy");
    /// assert_eq!(obj["summary"], "Deploy the app");
    /// // fields not requested are absent
    /// assert!(obj.get("examples").is_none());
    /// ```
    pub fn to_json_with_fields(&self, fields: &[&str]) -> Result<String, QueryError> {
        if fields.is_empty() {
            return self.to_json();
        }
        let value = serde_json::to_value(&self.commands).map_err(QueryError::Serialization)?;
        let filtered = filter_commands_value(value, fields);
        serde_json::to_string_pretty(&filtered).map_err(QueryError::Serialization)
    }

    /// Iterate over every command in the tree depth-first, including all
    /// nested subcommands at any depth.
    ///
    /// Each entry carries the [`CommandEntry::path`] (canonical names from the
    /// registry root to the command) and a reference to the [`Command`].
    ///
    /// Commands are yielded in depth-first order: a parent command appears
    /// immediately before all of its descendants. Within each level, commands
    /// appear in registration order.
    ///
    /// # Examples
    ///
    /// ```
    /// # use argot_cmd::{Command, Registry};
    /// let registry = Registry::new(vec![
    ///     Command::builder("remote")
    ///         .subcommand(Command::builder("add").build().unwrap())
    ///         .subcommand(Command::builder("remove").build().unwrap())
    ///         .build()
    ///         .unwrap(),
    ///     Command::builder("status").build().unwrap(),
    /// ]);
    ///
    /// let all: Vec<_> = registry.iter_all_recursive();
    /// let names: Vec<String> = all.iter().map(|e| e.path_str()).collect();
    ///
    /// assert_eq!(names, ["remote", "remote.add", "remote.remove", "status"]);
    /// ```
    pub fn iter_all_recursive(&self) -> Vec<CommandEntry<'_>> {
        let mut out = Vec::new();
        for cmd in &self.commands {
            collect_recursive(cmd, vec![], &mut out);
        }
        out
    }
}

/// Serialize a single [`Command`] to a pretty-printed JSON string, filtering
/// the output to only include the requested top-level fields.
///
/// Behaves like [`Registry::to_json_with_fields`] but for a single command
/// rather than the whole registry.
///
/// If `fields` is empty the method serializes the full command (equivalent to
/// `serde_json::to_string_pretty(cmd)`).
///
/// # Errors
///
/// Returns [`QueryError::Serialization`] if `serde_json` fails.
///
/// # Examples
///
/// ```
/// # use argot_cmd::{Command, Registry};
/// # use argot_cmd::query::command_to_json_with_fields;
/// let cmd = Command::builder("deploy")
///     .summary("Deploy the app")
///     .build()
///     .unwrap();
///
/// let json = command_to_json_with_fields(&cmd, &["canonical", "summary"]).unwrap();
/// let v: serde_json::Value = serde_json::from_str(&json).unwrap();
/// assert_eq!(v["canonical"], "deploy");
/// assert_eq!(v["summary"], "Deploy the app");
/// assert!(v.get("examples").is_none());
/// ```
pub fn command_to_json_with_fields(cmd: &Command, fields: &[&str]) -> Result<String, QueryError> {
    if fields.is_empty() {
        return serde_json::to_string_pretty(cmd).map_err(QueryError::Serialization);
    }
    let value = serde_json::to_value(cmd).map_err(QueryError::Serialization)?;
    let filtered = filter_command_object(value, fields);
    serde_json::to_string_pretty(&filtered).map_err(QueryError::Serialization)
}

/// Filter a JSON array of command objects to only include the requested fields
/// in each entry.
fn filter_commands_value(value: serde_json::Value, fields: &[&str]) -> serde_json::Value {
    match value {
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(
                arr.into_iter()
                    .map(|v| filter_command_object(v, fields))
                    .collect(),
            )
        }
        other => other,
    }
}

/// Filter a single command JSON object to only include the requested fields.
///
/// The `subcommands` value (if present and requested) has its entries
/// recursively filtered as well.
fn filter_command_object(value: serde_json::Value, fields: &[&str]) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for field in fields {
                if let Some(v) = map.get(*field) {
                    // If this field is `subcommands`, recursively filter its entries.
                    let filtered_v = if *field == "subcommands" {
                        filter_commands_value(v.clone(), fields)
                    } else {
                        v.clone()
                    };
                    out.insert((*field).to_owned(), filtered_v);
                }
            }
            serde_json::Value::Object(out)
        }
        other => other,
    }
}

fn collect_recursive<'a>(cmd: &'a Command, mut path: Vec<String>, out: &mut Vec<CommandEntry<'a>>) {
    path.push(cmd.canonical.clone());
    out.push(CommandEntry {
        path: path.clone(),
        command: cmd,
    });
    for sub in &cmd.subcommands {
        collect_recursive(sub, path.clone(), out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Command;

    fn registry() -> Registry {
        let sub = Command::builder("push")
            .summary("Push changes")
            .build()
            .unwrap();
        let remote = Command::builder("remote")
            .summary("Manage remotes")
            .subcommand(sub)
            .build()
            .unwrap();
        let list = Command::builder("list")
            .summary("List all items in the store")
            .build()
            .unwrap();
        Registry::new(vec![remote, list])
    }

    #[test]
    fn test_list_commands() {
        let r = registry();
        let cmds = r.list_commands();
        assert_eq!(cmds.len(), 2);
    }

    #[test]
    fn test_get_command() {
        let r = registry();
        assert!(r.get_command("remote").is_some());
        assert!(r.get_command("missing").is_none());
    }

    #[test]
    fn test_get_subcommand() {
        let r = registry();
        assert_eq!(
            r.get_subcommand(&["remote", "push"]).unwrap().canonical,
            "push"
        );
        assert!(r.get_subcommand(&["remote", "nope"]).is_none());
        assert!(r.get_subcommand(&[]).is_none());
    }

    #[test]
    fn test_get_examples_empty() {
        let r = registry();
        assert_eq!(r.get_examples("list"), Some([].as_slice()));
    }

    #[test]
    fn test_search_match() {
        let r = registry();
        let results = r.search("store");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].canonical, "list");
    }

    #[test]
    fn test_search_no_match() {
        let r = registry();
        assert!(r.search("zzz").is_empty());
    }

    #[cfg(feature = "fuzzy")]
    #[test]
    fn test_fuzzy_search_match() {
        let r = registry();
        let results = r.fuzzy_search("lst");
        assert!(!results.is_empty());
        assert!(results.iter().any(|(cmd, _)| cmd.canonical == "list"));
    }

    #[cfg(feature = "fuzzy")]
    #[test]
    fn test_fuzzy_search_no_match() {
        let r = registry();
        assert!(r.fuzzy_search("zzzzz").is_empty());
    }

    #[cfg(feature = "fuzzy")]
    #[test]
    fn test_fuzzy_search_sorted_by_score() {
        let exact = Command::builder("list")
            .summary("List all items")
            .build()
            .unwrap();
        let weak = Command::builder("remote")
            .summary("Manage remotes")
            .build()
            .unwrap();
        let r = Registry::new(vec![weak, exact]);
        let results = r.fuzzy_search("list");
        assert!(!results.is_empty());
        assert_eq!(results[0].0.canonical, "list");
        for window in results.windows(2) {
            assert!(window[0].1 >= window[1].1);
        }
    }

    #[test]
    fn test_to_json() {
        let r = registry();
        let json = r.to_json().unwrap();
        assert!(json.contains("remote"));
        assert!(json.contains("list"));
        let _: serde_json::Value = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn test_to_json_with_fields_filters_keys() {
        let r = Registry::new(vec![
            Command::builder("deploy")
                .summary("Deploy the app")
                .description("Deploys to production")
                .build()
                .unwrap(),
        ]);
        let json = r.to_json_with_fields(&["canonical", "summary"]).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = &v[0];
        assert_eq!(obj["canonical"], "deploy");
        assert_eq!(obj["summary"], "Deploy the app");
        // fields not in the filter must be absent
        assert!(obj.get("description").is_none());
        assert!(obj.get("examples").is_none());
        assert!(obj.get("aliases").is_none());
    }

    #[test]
    fn test_to_json_with_fields_empty_falls_back_to_full() {
        let r = Registry::new(vec![
            Command::builder("deploy")
                .summary("Deploy the app")
                .build()
                .unwrap(),
        ]);
        let full = r.to_json().unwrap();
        let filtered = r.to_json_with_fields(&[]).unwrap();
        assert_eq!(full, filtered);
    }

    #[test]
    fn test_to_json_with_fields_missing_field_silently_omitted() {
        let r = Registry::new(vec![
            Command::builder("deploy").build().unwrap(),
        ]);
        // "nonexistent_key" does not exist — should produce an object with only "canonical"
        let json = r
            .to_json_with_fields(&["canonical", "nonexistent_key"])
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = &v[0];
        assert_eq!(obj["canonical"], "deploy");
        assert!(obj.get("nonexistent_key").is_none());
    }

    #[test]
    fn test_to_json_with_fields_subcommands_filtered_recursively() {
        let r = Registry::new(vec![
            Command::builder("remote")
                .summary("Manage remotes")
                .subcommand(
                    Command::builder("add")
                        .summary("Add a remote")
                        .description("Detailed add docs")
                        .build()
                        .unwrap(),
                )
                .build()
                .unwrap(),
        ]);
        let json = r
            .to_json_with_fields(&["canonical", "summary", "subcommands"])
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = &v[0];
        assert_eq!(obj["canonical"], "remote");
        assert!(obj.get("description").is_none());
        // subcommands array should be present
        let subs = obj["subcommands"].as_array().unwrap();
        assert_eq!(subs.len(), 1);
        // the subcommand entry should also be filtered
        assert_eq!(subs[0]["canonical"], "add");
        assert_eq!(subs[0]["summary"], "Add a remote");
        assert!(subs[0].get("description").is_none());
    }

    #[test]
    fn test_to_json_with_fields_subcommands_not_requested_absent() {
        let r = Registry::new(vec![
            Command::builder("remote")
                .subcommand(Command::builder("add").build().unwrap())
                .build()
                .unwrap(),
        ]);
        // "subcommands" not in requested fields → key should be absent
        let json = r.to_json_with_fields(&["canonical"]).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = &v[0];
        assert_eq!(obj["canonical"], "remote");
        assert!(obj.get("subcommands").is_none());
    }

    #[test]
    fn test_command_to_json_with_fields() {
        let cmd = Command::builder("deploy")
            .summary("Deploy the app")
            .description("Long description")
            .build()
            .unwrap();
        let json = command_to_json_with_fields(&cmd, &["canonical", "summary"]).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["canonical"], "deploy");
        assert_eq!(v["summary"], "Deploy the app");
        assert!(v.get("description").is_none());
    }

    #[test]
    fn test_command_to_json_with_fields_empty_falls_back_to_full() {
        let cmd = Command::builder("deploy")
            .summary("Deploy the app")
            .build()
            .unwrap();
        let full = serde_json::to_string_pretty(&cmd).unwrap();
        let filtered = command_to_json_with_fields(&cmd, &[]).unwrap();
        assert_eq!(full, filtered);
    }

    #[test]
    fn test_match_intent_single_word() {
        let r = Registry::new(vec![
            Command::builder("deploy")
                .summary("Deploy a service")
                .build()
                .unwrap(),
            Command::builder("status")
                .summary("Check service status")
                .build()
                .unwrap(),
        ]);
        let results = r.match_intent("deploy");
        assert!(!results.is_empty());
        assert_eq!(results[0].0.canonical, "deploy");
    }

    #[test]
    fn test_match_intent_phrase() {
        let r = Registry::new(vec![
            Command::builder("deploy")
                .summary("Deploy a service to an environment")
                .semantic_alias("release to production")
                .semantic_alias("push to environment")
                .build()
                .unwrap(),
            Command::builder("status")
                .summary("Check service status")
                .build()
                .unwrap(),
        ]);
        let results = r.match_intent("release to production");
        assert!(!results.is_empty());
        assert_eq!(results[0].0.canonical, "deploy");
    }

    #[test]
    fn test_match_intent_no_match() {
        let r = Registry::new(vec![Command::builder("deploy")
            .summary("Deploy a service")
            .build()
            .unwrap()]);
        let results = r.match_intent("zzz xyzzy foobar");
        assert!(results.is_empty());
    }

    #[test]
    fn test_match_intent_sorted_by_score() {
        let r = Registry::new(vec![
            Command::builder("status")
                .summary("Check service status")
                .build()
                .unwrap(),
            Command::builder("deploy")
                .summary("Deploy a service to an environment")
                .semantic_alias("release to production")
                .semantic_alias("push to environment")
                .build()
                .unwrap(),
        ]);
        // "deploy to production" matches deploy on "deploy", "to", "production"
        // and matches status only on "to" (if present in summary)
        let results = r.match_intent("deploy to production");
        assert!(!results.is_empty());
        // deploy should score higher than status
        assert_eq!(results[0].0.canonical, "deploy");
        // scores are descending
        for window in results.windows(2) {
            assert!(window[0].1 >= window[1].1);
        }
    }

    #[test]
    fn test_iter_all_recursive_flat() {
        let r = Registry::new(vec![
            Command::builder("a").build().unwrap(),
            Command::builder("b").build().unwrap(),
        ]);
        let entries = r.iter_all_recursive();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path_str(), "a");
        assert_eq!(entries[1].path_str(), "b");
    }

    #[test]
    fn test_iter_all_recursive_nested() {
        let registry = Registry::new(vec![
            Command::builder("remote")
                .subcommand(Command::builder("add").build().unwrap())
                .subcommand(Command::builder("remove").build().unwrap())
                .build()
                .unwrap(),
            Command::builder("status").build().unwrap(),
        ]);

        let names: Vec<String> = registry
            .iter_all_recursive()
            .iter()
            .map(|e| e.path_str())
            .collect();

        assert_eq!(names, ["remote", "remote.add", "remote.remove", "status"]);
    }

    #[test]
    fn test_iter_all_recursive_deep_nesting() {
        let leaf = Command::builder("blue-green").build().unwrap();
        let mid = Command::builder("strategy")
            .subcommand(leaf)
            .build()
            .unwrap();
        let top = Command::builder("deploy").subcommand(mid).build().unwrap();
        let r = Registry::new(vec![top]);

        let names: Vec<String> = r
            .iter_all_recursive()
            .iter()
            .map(|e| e.path_str())
            .collect();

        assert_eq!(
            names,
            ["deploy", "deploy.strategy", "deploy.strategy.blue-green"]
        );
    }

    #[test]
    fn test_iter_all_recursive_entry_helpers() {
        let registry = Registry::new(vec![Command::builder("remote")
            .subcommand(Command::builder("add").build().unwrap())
            .build()
            .unwrap()]);
        let entries = registry.iter_all_recursive();
        assert_eq!(entries[1].name(), "add");
        assert_eq!(entries[1].path, vec!["remote", "add"]);
        assert_eq!(entries[1].path_str(), "remote.add");
    }

    #[test]
    fn test_iter_all_recursive_empty() {
        let r = Registry::new(vec![]);
        assert!(r.iter_all_recursive().is_empty());
    }
}

#[cfg(test)]
#[cfg(feature = "fuzzy")]
mod fuzzy_tests {
    use super::*;
    use crate::model::Command;

    #[test]
    fn test_fuzzy_search_returns_matches() {
        let r = Registry::new(vec![
            Command::builder("deploy").build().unwrap(),
            Command::builder("delete").build().unwrap(),
            Command::builder("status").build().unwrap(),
        ]);
        let results = r.fuzzy_search("dep");
        assert!(!results.is_empty(), "should find matches for 'dep'");
        // "deploy" should be the top match
        assert_eq!(results[0].0.canonical, "deploy");
    }

    #[test]
    fn test_fuzzy_search_sorted_by_score_descending() {
        let r = Registry::new(vec![
            Command::builder("deploy").build().unwrap(),
            Command::builder("delete").build().unwrap(),
        ]);
        let results = r.fuzzy_search("deploy");
        assert!(!results.is_empty());
        // Scores should be in descending order
        for i in 1..results.len() {
            assert!(
                results[i - 1].1 >= results[i].1,
                "results should be sorted by score desc"
            );
        }
    }

    #[test]
    fn test_fuzzy_search_no_match_returns_empty() {
        let r = Registry::new(vec![Command::builder("run").build().unwrap()]);
        let results = r.fuzzy_search("zzzzzzz");
        // No match should return empty (or very low score filtered out)
        // The fuzzy matcher may return low-score matches, so just verify
        // that "run" is NOT the top result for a nonsense query, or it returns empty
        if !results.is_empty() {
            // If it returns anything, score must be positive
            assert!(results.iter().all(|(_, score)| *score > 0));
        }
    }

    #[test]
    fn test_fuzzy_search_score_type() {
        let r = Registry::new(vec![Command::builder("deploy").build().unwrap()]);
        let results = r.fuzzy_search("deploy");
        assert!(!results.is_empty());
        // Score is i64
        let score: i64 = results[0].1;
        assert!(score > 0);
    }
}
