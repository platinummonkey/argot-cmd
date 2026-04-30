//! Layered command sources with priority-ordered merging.
//!
//! This module lets an application assemble a [`crate::Registry`] from several
//! [`CommandSource`] implementations rather than from a single hard-coded
//! `Vec<Command>`. Each source produces [`LoadedCommand`] values tagged with a
//! [`Layer`] (e.g. `Embedded`, `User`, `Project`, `Local`) and an optional
//! priority hint. When two sources contribute commands with the same canonical
//! name, the merger picks a winner using a deterministic precedence rule and
//! records every losing entry as a [`LoadDiagnostic::Shadowed`].
//!
//! ## Precedence
//!
//! For two `LoadedCommand` values with the same canonical name, the winner is
//! chosen by:
//!
//! 1. **Layer rank** (higher rank wins). Default order, low → high:
//!    `Embedded` < `User` < `Project` < `Local`. `Custom(n)` is ranked by `n`.
//! 2. **Priority** within the same layer (higher wins, default `0`).
//! 3. **Source insertion order** as a final tie-breaker (later source wins).
//!
//! Use [`LoadedCommand::overrides`] to declare an explicit shadow target — this
//! does not affect resolution order, but produces a
//! [`LoadDiagnostic::OverrideTargetMissing`] diagnostic if the named command
//! does not exist at a *strictly lower* [`Layer::rank`], catching typos.
//! "Lower layer" throughout this module always means lower **rank**, not
//! variant order — see [`Layer::rank`] for why.
//!
//! ## Why local files?
//!
//! Compile-time `Vec<Command>` definitions are great for typed, shippable
//! commands. But agent-first tooling often wants:
//!
//! - Per-project command tweaks without forking the binary.
//! - Authoring commands as Markdown so non-Rust contributors can edit them.
//! - Layering a user's personal aliases on top of the project's commands.
//!
//! The source layer makes all three possible while preserving argot's
//! "metadata is the source of truth" invariant.
//!
//! ## Example
//!
//! ```
//! use argot_cmd::{Command, Registry};
//! use argot_cmd::source::{EmbeddedSource, LayeredBuilder, Layer};
//!
//! let embedded = vec![
//!     Command::builder("deploy")
//!         .summary("Deploy (built-in)")
//!         .build()
//!         .unwrap(),
//! ];
//! let user_override = vec![
//!     Command::builder("deploy")
//!         .summary("Deploy (user override)")
//!         .build()
//!         .unwrap(),
//! ];
//!
//! let (registry, diagnostics) = LayeredBuilder::new()
//!     .add(EmbeddedSource::new("builtin", embedded))
//!     .add(EmbeddedSource::new("user", user_override).with_layer(Layer::User))
//!     .build();
//!
//! assert_eq!(registry.get_command("deploy").unwrap().summary, "Deploy (user override)");
//! assert_eq!(diagnostics.len(), 1); // the embedded "deploy" was shadowed
//! ```

use std::collections::HashMap;

use crate::model::Command;
use crate::query::Registry;

mod embedded;
#[cfg(feature = "markdown-source")]
pub mod markdown;

pub use embedded::EmbeddedSource;

/// Precedence layer for a [`CommandSource`].
///
/// Built-in variants are ranked low-to-high so that local project files can
/// override a binary's compiled-in commands by default. Use [`Layer::Custom`]
/// to insert a layer at an arbitrary rank when the four built-ins do not match
/// the application's mental model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    /// Compiled into the binary (lowest rank: 0).
    Embedded,
    /// User-wide overrides, e.g. `$XDG_CONFIG_HOME/<app>/commands` (rank: 100).
    User,
    /// Project-wide overrides, e.g. a `.<app>/commands/` directory committed to
    /// the repository (rank: 200).
    Project,
    /// Per-invocation / current-working-directory overrides — wins over all
    /// built-in layers (rank: 300).
    Local,
    /// Custom rank for applications that need finer-grained ordering.
    Custom(i32),
}

impl Layer {
    /// Numeric rank used for precedence comparisons. Higher wins.
    ///
    /// `rank()` is the only meaningful ordering on `Layer`. Argot does not
    /// impl `Ord` because rank is **not injective**: `Layer::User.rank() == 100`
    /// and `Layer::Custom(100).rank() == 100`, while `Layer::User !=
    /// Layer::Custom(100)`. There is no total order on `Layer` consistent with
    /// the derived `Eq`, so callers compare precedence by calling `rank()`
    /// directly. Throughout the public API, words like "lower layer" mean
    /// "strictly lower rank" — two layers whose ranks collide are siblings,
    /// not lower-than each other.
    pub fn rank(self) -> i32 {
        match self {
            Layer::Embedded => 0,
            Layer::User => 100,
            Layer::Project => 200,
            Layer::Local => 300,
            Layer::Custom(n) => n,
        }
    }

    /// Human-readable label for this layer.
    ///
    /// The format is **stable and machine-parseable**: built-in variants
    /// produce one of `"embedded"`, `"user"`, `"project"`, `"local"`, and
    /// `Custom(n)` produces `"custom(N)"` where `N` is the integer rank.
    /// Used in diagnostic output via [`Layer`]'s `Display` impl.
    pub fn label(self) -> String {
        // Delegate to Display so the format lives in exactly one place.
        self.to_string()
    }
}

impl std::fmt::Display for Layer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Layer::Embedded => f.write_str("embedded"),
            Layer::User => f.write_str("user"),
            Layer::Project => f.write_str("project"),
            Layer::Local => f.write_str("local"),
            Layer::Custom(n) => write!(f, "custom({})", n),
        }
    }
}

/// Provenance for a loaded command — where it came from on disk, who produced
/// it, and which layer it sits in.
#[derive(Debug, Clone)]
pub struct SourceOrigin {
    /// Logical name of the producing source (e.g. `"builtin"`, `"user-md"`).
    pub source: String,
    /// Layer this command was contributed under.
    pub layer: Layer,
    /// Optional file path or other locator. `None` for in-memory sources.
    pub path: Option<String>,
}

/// A `Command` paired with the metadata needed to layer it against other
/// loaded commands.
#[derive(Debug, Clone)]
pub struct LoadedCommand {
    /// The command itself. Handlers (set via [`crate::CommandBuilder::handler`])
    /// can be attached here after loading from a metadata-only source.
    pub command: Command,
    /// Higher-priority commands win against lower-priority commands within the
    /// same layer. Default `0`.
    pub priority: i32,
    /// Optional canonical name of a strictly-lower-rank command this entry
    /// is intended to shadow. Used only to produce
    /// [`LoadDiagnostic::OverrideTargetMissing`] when the named target is not
    /// present at a *strictly lower* [`Layer::rank`] than this entry.
    /// Same-rank siblings (including two `Layer::Custom(n)` values that share
    /// `n`, or `Layer::Custom(100)` vs `Layer::User`) do **not** satisfy the
    /// override — pick a rank below the target's if you need override
    /// semantics across custom layers.
    pub overrides: Option<String>,
    /// Provenance metadata.
    pub origin: SourceOrigin,
}

impl LoadedCommand {
    /// Construct a `LoadedCommand` with default priority `0` and no override
    /// target.
    pub fn new(command: Command, origin: SourceOrigin) -> Self {
        Self {
            command,
            priority: 0,
            overrides: None,
            origin,
        }
    }

    /// Set the priority hint (higher wins within a layer).
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Declare that this command is intended to override a strictly-lower-rank
    /// command with the given canonical name.
    ///
    /// This does not change resolution order (which is purely
    /// (layer rank, priority, insertion)). It is checked after the merge to
    /// emit a diagnostic if the named target was not present at any
    /// *strictly lower* [`Layer::rank`]. See [`LoadedCommand::overrides`] for
    /// the implications when using `Layer::Custom`.
    pub fn overriding(mut self, canonical: impl Into<String>) -> Self {
        self.overrides = Some(canonical.into());
        self
    }
}

/// Result of a single [`CommandSource::load`] call.
#[derive(Debug, Default)]
pub struct SourceLoad {
    /// Commands produced by the source.
    pub commands: Vec<LoadedCommand>,
    /// Diagnostics emitted while loading (parse errors, schema warnings, etc.).
    pub diagnostics: Vec<LoadDiagnostic>,
}

/// A producer of [`LoadedCommand`] values for the layered registry.
///
/// Implementations are free to read from disk, embed compile-time data, fetch
/// over the network, or anything else. Loading is invoked at most once per
/// build by [`LayeredBuilder::build`].
///
/// ## Object safety
///
/// `CommandSource` is object-safe so that [`LayeredBuilder`] can hold a
/// `Vec<Box<dyn CommandSource>>` and treat sources uniformly regardless of
/// their concrete type.
pub trait CommandSource {
    /// Logical name of the source (used in diagnostics and provenance).
    fn name(&self) -> &str;

    /// Load all commands this source contributes.
    ///
    /// Errors that prevent individual commands from being parsed should be
    /// reported via [`SourceLoad::diagnostics`] rather than panicking, so that
    /// a single bad file does not poison the whole build.
    fn load(&self) -> SourceLoad;
}

/// A non-fatal warning produced during source loading or merging.
#[derive(Debug, Clone)]
pub enum LoadDiagnostic {
    /// A loaded command was hidden by a higher-precedence entry with the same
    /// canonical name. The shadowed command's origin is preserved so the user
    /// can find it.
    Shadowed {
        /// Canonical name of the command that was shadowed.
        canonical: String,
        /// Where the shadowed (losing) command came from.
        shadowed: SourceOrigin,
        /// Where the winning command came from.
        winner: SourceOrigin,
    },
    /// A `LoadedCommand::overrides` field named a canonical command that was
    /// not present in any lower layer (likely a typo).
    OverrideTargetMissing {
        /// The override target that was not found.
        target: String,
        /// Where the overriding entry came from.
        origin: SourceOrigin,
    },
    /// A source-level error: a file failed to parse, a directory was missing,
    /// etc. `path` is the location of the failure if known.
    SourceError {
        /// Logical source name.
        source: String,
        /// Optional file or resource path.
        path: Option<String>,
        /// Human-readable error description.
        message: String,
    },
    /// A frontmatter or schema field was malformed — the source recovered, but
    /// some metadata may have been dropped.
    SchemaWarning {
        /// Where the warning originated.
        origin: SourceOrigin,
        /// Field name (e.g. `"priority"`).
        field: String,
        /// Description of the problem.
        message: String,
    },
}

impl LoadDiagnostic {
    /// Returns `true` if this diagnostic represents a hard error (not a
    /// warning that produced a usable command anyway).
    pub fn is_error(&self) -> bool {
        matches!(self, LoadDiagnostic::SourceError { .. })
    }
}

impl std::fmt::Display for LoadDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadDiagnostic::Shadowed { canonical, shadowed, winner } => write!(
                f,
                "command '{}' from {}[{}]:{} shadowed by {}[{}]:{}",
                canonical,
                shadowed.source,
                shadowed.layer,
                shadowed.path.as_deref().unwrap_or("<in-memory>"),
                winner.source,
                winner.layer,
                winner.path.as_deref().unwrap_or("<in-memory>"),
            ),
            LoadDiagnostic::OverrideTargetMissing { target, origin } => write!(
                f,
                "{}[{}]:{} declares overrides='{}' but no lower-layer command with that canonical name was loaded",
                origin.source,
                origin.layer,
                origin.path.as_deref().unwrap_or("<in-memory>"),
                target,
            ),
            LoadDiagnostic::SourceError { source, path, message } => write!(
                f,
                "source '{}' error{}: {}",
                source,
                path.as_ref().map(|p| format!(" at {}", p)).unwrap_or_default(),
                message,
            ),
            LoadDiagnostic::SchemaWarning { origin, field, message } => write!(
                f,
                "{}[{}]:{} field '{}': {}",
                origin.source,
                origin.layer,
                origin.path.as_deref().unwrap_or("<in-memory>"),
                field,
                message,
            ),
        }
    }
}

/// Assembles a [`Registry`] from one or more [`CommandSource`] implementations.
///
/// Sources are added in order via [`LayeredBuilder::add`]. The order matters
/// only as the final tie-breaker after layer rank and priority have been
/// considered.
///
/// See the [module-level docs](crate::source) for precedence rules.
pub struct LayeredBuilder {
    sources: Vec<Box<dyn CommandSource>>,
}

impl LayeredBuilder {
    /// Create an empty builder.
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    /// Append a source. Sources added later act as a tie-breaker when layer
    /// and priority are equal.
    // Named `add` rather than `push` to read fluently in chained builders
    // (`.add(a).add(b)`); it is intentionally not the `std::ops::Add::add`
    // associative operation, which makes no sense for a builder.
    #[allow(clippy::should_implement_trait)]
    pub fn add<S: CommandSource + 'static>(mut self, source: S) -> Self {
        self.sources.push(Box::new(source));
        self
    }

    /// Append a boxed source (useful when sources are constructed dynamically).
    pub fn add_boxed(mut self, source: Box<dyn CommandSource>) -> Self {
        self.sources.push(source);
        self
    }

    /// Drain every source, merge by canonical name with precedence
    /// `(layer rank desc, priority desc, insertion order desc)`, and return
    /// the resulting [`Registry`] alongside any diagnostics produced.
    ///
    /// The merge currently operates only on top-level commands. Subcommands
    /// inside a winning command are preserved exactly as they appeared in the
    /// source — there is no cross-source subcommand merging in v1.
    ///
    /// The `Vec<LoadDiagnostic>` half of the return is `#[must_use]`-flagged
    /// because dropping it silently can hide [`LoadDiagnostic::SourceError`]
    /// entries — i.e. configured sources that failed to contribute any
    /// commands. Inspect with [`LoadDiagnostic::is_error`] to decide whether
    /// to abort startup, or print all diagnostics in dev mode.
    #[must_use = "load diagnostics may include SourceError entries indicating sources that failed to load — inspect them or pattern-match to filter"]
    pub fn build(self) -> (Registry, Vec<LoadDiagnostic>) {
        let mut diagnostics: Vec<LoadDiagnostic> = Vec::new();
        let mut all_loaded: Vec<(usize, LoadedCommand)> = Vec::new();

        for (idx, source) in self.sources.into_iter().enumerate() {
            let SourceLoad {
                commands,
                diagnostics: src_diags,
            } = source.load();
            diagnostics.extend(src_diags);
            for c in commands {
                all_loaded.push((idx, c));
            }
        }

        // Group by canonical (case-sensitive — canonical names are normalized
        // by the user). Track every entry so we can emit shadow diagnostics.
        let mut groups: HashMap<String, Vec<(usize, LoadedCommand)>> = HashMap::new();
        for (idx, lc) in all_loaded.into_iter() {
            groups
                .entry(lc.command.canonical.clone())
                .or_default()
                .push((idx, lc));
        }

        // For each canonical, pick the winner.
        let mut winners: Vec<(usize, LoadedCommand)> = Vec::new();
        let mut overrides_to_check: Vec<(String, SourceOrigin, i32)> = Vec::new();
        let mut all_origins_by_canonical: HashMap<String, Vec<SourceOrigin>> = HashMap::new();

        // Iterate groups in canonical-name order so diagnostics emerge in a
        // deterministic order regardless of HashMap hashing.
        let mut group_keys: Vec<String> = groups.keys().cloned().collect();
        group_keys.sort();

        for canonical in group_keys {
            let mut entries = groups.remove(&canonical).unwrap_or_default();
            // Sort with strongest entry last so we can pop the winner.
            entries.sort_by(|a, b| {
                let a_rank = a.1.origin.layer.rank();
                let b_rank = b.1.origin.layer.rank();
                a_rank
                    .cmp(&b_rank)
                    .then_with(|| a.1.priority.cmp(&b.1.priority))
                    .then_with(|| a.0.cmp(&b.0))
            });

            // Record provenance of every entry (used for OverrideTargetMissing).
            all_origins_by_canonical
                .entry(canonical.clone())
                .or_default()
                .extend(entries.iter().map(|(_, lc)| lc.origin.clone()));

            // Soft-fail rather than panic if the invariant ever breaks. The
            // group is built by `entry().or_default().push()`, so every group
            // should have ≥ 1 entry — but propagating a diagnostic instead of
            // panicking protects the caller from a future refactor that drains
            // a group early.
            let Some(winner) = entries.pop() else {
                diagnostics.push(LoadDiagnostic::SourceError {
                    source: "<merger>".to_string(),
                    path: None,
                    message: format!(
                        "internal error: empty group for canonical '{}' (please report)",
                        canonical
                    ),
                });
                continue;
            };
            // Everything left in `entries` is shadowed.
            for (_, loser) in entries.iter() {
                diagnostics.push(LoadDiagnostic::Shadowed {
                    canonical: canonical.clone(),
                    shadowed: loser.origin.clone(),
                    winner: winner.1.origin.clone(),
                });
            }

            // Queue override-target check.
            if let Some(ref target) = winner.1.overrides {
                overrides_to_check.push((
                    target.clone(),
                    winner.1.origin.clone(),
                    winner.1.origin.layer.rank(),
                ));
            }

            winners.push(winner);
        }

        // Validate override targets after we know the full set of canonicals.
        // Per the doc on `LoadedCommand::overrides`, the target must exist in a
        // *strictly lower* layer than the declarer — a same- or higher-layer
        // entry does not satisfy the declared shadow intent.
        for (target, origin, declarer_rank) in overrides_to_check {
            let satisfied = all_origins_by_canonical
                .get(&target)
                .map(|origins| origins.iter().any(|o| o.layer.rank() < declarer_rank))
                .unwrap_or(false);
            if !satisfied {
                diagnostics.push(LoadDiagnostic::OverrideTargetMissing { target, origin });
            }
        }

        // Stable, deterministic command order: sort winners by canonical name.
        winners.sort_by(|a, b| a.1.command.canonical.cmp(&b.1.command.canonical));
        let commands = winners.into_iter().map(|(_, lc)| lc.command).collect();

        (Registry::new(commands), diagnostics)
    }
}

impl Default for LayeredBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cmd(canonical: &str, summary: &str) -> Command {
        Command::builder(canonical)
            .summary(summary)
            .build()
            .unwrap()
    }

    fn loaded(canonical: &str, summary: &str, layer: Layer, source: &str) -> LoadedCommand {
        LoadedCommand::new(
            make_cmd(canonical, summary),
            SourceOrigin {
                source: source.to_string(),
                layer,
                path: None,
            },
        )
    }

    struct StubSource {
        name: String,
        commands: Vec<LoadedCommand>,
    }

    impl CommandSource for StubSource {
        fn name(&self) -> &str {
            &self.name
        }
        fn load(&self) -> SourceLoad {
            SourceLoad {
                commands: self.commands.clone(),
                diagnostics: Vec::new(),
            }
        }
    }

    #[test]
    fn higher_layer_wins() {
        let embedded = StubSource {
            name: "embedded".into(),
            commands: vec![loaded(
                "deploy",
                "embedded ver",
                Layer::Embedded,
                "embedded",
            )],
        };
        let project = StubSource {
            name: "project".into(),
            commands: vec![loaded("deploy", "project ver", Layer::Project, "project")],
        };

        let (registry, diags) = LayeredBuilder::new().add(embedded).add(project).build();
        assert_eq!(
            registry.get_command("deploy").unwrap().summary,
            "project ver"
        );
        // exactly one Shadowed diagnostic for the embedded entry
        assert_eq!(diags.len(), 1);
        assert!(
            matches!(&diags[0], LoadDiagnostic::Shadowed { canonical, .. } if canonical == "deploy")
        );
    }

    #[test]
    fn priority_breaks_within_layer() {
        // Order [hi, lo]: insertion order alone would pick `lo` (later wins);
        // priority must be the actual discriminator.
        let hi = loaded("x", "hi", Layer::Project, "b").with_priority(10);
        let lo = loaded("x", "lo", Layer::Project, "a").with_priority(1);
        let src = StubSource {
            name: "s".into(),
            commands: vec![hi, lo],
        };
        let (registry, _diags) = LayeredBuilder::new().add(src).build();
        assert_eq!(registry.get_command("x").unwrap().summary, "hi");
    }

    #[test]
    fn layer_rank_dominates_insertion_order() {
        // Local source added first; Embedded source added second.
        // Insertion-order rule alone would pick Embedded (later wins);
        // layer rank must dominate.
        let local_first = StubSource {
            name: "local".into(),
            commands: vec![loaded("x", "local", Layer::Local, "local")],
        };
        let embedded_second = StubSource {
            name: "embedded".into(),
            commands: vec![loaded("x", "embedded", Layer::Embedded, "embedded")],
        };
        let (registry, _) = LayeredBuilder::new()
            .add(local_first)
            .add(embedded_second)
            .build();
        assert_eq!(registry.get_command("x").unwrap().summary, "local");
    }

    #[test]
    fn shadowed_diagnostic_carries_correct_winner_and_loser() {
        let embedded = StubSource {
            name: "e".into(),
            commands: vec![loaded("d", "lo", Layer::Embedded, "e-src")],
        };
        let project = StubSource {
            name: "p".into(),
            commands: vec![loaded("d", "hi", Layer::Project, "p-src")],
        };
        let (_, diags) = LayeredBuilder::new().add(embedded).add(project).build();
        let LoadDiagnostic::Shadowed {
            shadowed, winner, ..
        } = &diags[0]
        else {
            panic!("expected Shadowed, got {:?}", diags);
        };
        assert!(matches!(shadowed.layer, Layer::Embedded));
        assert!(matches!(winner.layer, Layer::Project));
        assert_eq!(shadowed.source, "e-src");
        assert_eq!(winner.source, "p-src");
    }

    #[test]
    fn add_boxed_accepts_dyn_command_source() {
        // Round-trip a boxed source through the builder. This both exercises
        // add_boxed and pins object-safety of the trait.
        let cmds = vec![Command::builder("ping").build().unwrap()];
        let boxed: Box<dyn CommandSource> = Box::new(EmbeddedSource::new("b", cmds));
        let (registry, _) = LayeredBuilder::new().add_boxed(boxed).build();
        assert!(registry.get_command("ping").is_some());
    }

    #[test]
    fn empty_builder_yields_empty_registry() {
        let (registry, diags) = LayeredBuilder::new().build();
        assert!(registry.list_commands().is_empty());
        assert!(diags.is_empty());
    }

    #[test]
    fn override_self_alone_emits_target_missing() {
        // A command declares overrides=self, but no other entry exists with
        // that canonical. The doc says "lower-layer command must exist" —
        // a same-layer self-reference does not satisfy that.
        let lc = loaded("deploy", "alone", Layer::Project, "p").overriding("deploy");
        let src = StubSource {
            name: "s".into(),
            commands: vec![lc],
        };
        let (_, diags) = LayeredBuilder::new().add(src).build();
        assert!(
            diags
                .iter()
                .any(|d| matches!(d, LoadDiagnostic::OverrideTargetMissing { target, .. } if target == "deploy")),
            "expected OverrideTargetMissing for self-override-alone, got {:?}",
            diags
        );
    }

    #[test]
    fn override_target_must_be_strictly_lower_layer() {
        // A Project entry overrides "foo" — but "foo" only exists at the same
        // (Project) layer. Per docstring, this should NOT satisfy the override.
        let same_layer_target = loaded("foo", "same", Layer::Project, "p1");
        let declarer = loaded("bar", "wins", Layer::Project, "p2").overriding("foo");
        let src = StubSource {
            name: "s".into(),
            commands: vec![same_layer_target, declarer],
        };
        let (_, diags) = LayeredBuilder::new().add(src).build();
        assert!(
            diags
                .iter()
                .any(|d| matches!(d, LoadDiagnostic::OverrideTargetMissing { target, .. } if target == "foo")),
            "expected OverrideTargetMissing because target was at same layer, got {:?}",
            diags
        );
    }

    #[test]
    fn source_diagnostics_propagate_to_build() {
        struct WarningSource;
        impl CommandSource for WarningSource {
            fn name(&self) -> &str {
                "warning-source"
            }
            fn load(&self) -> SourceLoad {
                SourceLoad {
                    commands: vec![],
                    diagnostics: vec![LoadDiagnostic::SchemaWarning {
                        origin: SourceOrigin {
                            source: "warning-source".into(),
                            layer: Layer::Project,
                            path: None,
                        },
                        field: "test".into(),
                        message: "synthetic warning".into(),
                    }],
                }
            }
        }
        let (_, diags) = LayeredBuilder::new().add(WarningSource).build();
        assert!(diags
            .iter()
            .any(|d| matches!(d, LoadDiagnostic::SchemaWarning { field, .. } if field == "test")));
    }

    #[test]
    fn is_error_distinguishes_fatal_diagnostics() {
        let fatal = LoadDiagnostic::SourceError {
            source: "s".into(),
            path: None,
            message: "boom".into(),
        };
        assert!(fatal.is_error());

        let warn = LoadDiagnostic::SchemaWarning {
            origin: SourceOrigin {
                source: "s".into(),
                layer: Layer::Embedded,
                path: None,
            },
            field: "f".into(),
            message: "m".into(),
        };
        assert!(!warn.is_error());
    }

    #[test]
    fn display_includes_layer_label() {
        let d = LoadDiagnostic::Shadowed {
            canonical: "x".into(),
            shadowed: SourceOrigin {
                source: "a".into(),
                layer: Layer::Embedded,
                path: None,
            },
            winner: SourceOrigin {
                source: "b".into(),
                layer: Layer::Local,
                path: None,
            },
        };
        let s = format!("{}", d);
        assert!(s.contains("[embedded]"), "{}", s);
        assert!(s.contains("[local]"), "{}", s);
    }

    #[test]
    fn insertion_order_breaks_ties() {
        let first = StubSource {
            name: "first".into(),
            commands: vec![loaded("x", "first", Layer::Project, "first")],
        };
        let second = StubSource {
            name: "second".into(),
            commands: vec![loaded("x", "second", Layer::Project, "second")],
        };
        let (registry, _) = LayeredBuilder::new().add(first).add(second).build();
        // Same layer + same priority → later source wins.
        assert_eq!(registry.get_command("x").unwrap().summary, "second");
    }

    #[test]
    fn no_collision_no_diagnostics() {
        let src = StubSource {
            name: "s".into(),
            commands: vec![
                loaded("a", "alpha", Layer::Embedded, "s"),
                loaded("b", "beta", Layer::Embedded, "s"),
            ],
        };
        let (registry, diags) = LayeredBuilder::new().add(src).build();
        assert_eq!(registry.list_commands().len(), 2);
        assert!(diags.is_empty(), "no shadows means no diagnostics");
    }

    #[test]
    fn override_target_missing_diagnostic() {
        let mut lc = loaded("deploy", "wins", Layer::Project, "project");
        lc = lc.overriding("nonexistent-target");
        let src = StubSource {
            name: "s".into(),
            commands: vec![lc],
        };
        let (_registry, diags) = LayeredBuilder::new().add(src).build();
        assert!(
            diags
                .iter()
                .any(|d| matches!(d, LoadDiagnostic::OverrideTargetMissing { target, .. } if target == "nonexistent-target")),
            "expected OverrideTargetMissing, got {:?}",
            diags
        );
    }

    #[test]
    fn override_target_satisfied_when_lower_exists() {
        let lower = loaded("deploy", "lower", Layer::Embedded, "builtin");
        let upper = loaded("deploy", "upper", Layer::Project, "project").overriding("deploy");
        let src = StubSource {
            name: "s".into(),
            commands: vec![lower, upper],
        };
        let (registry, diags) = LayeredBuilder::new().add(src).build();
        assert_eq!(registry.get_command("deploy").unwrap().summary, "upper");
        // No OverrideTargetMissing — the embedded entry satisfied the override.
        assert!(
            !diags
                .iter()
                .any(|d| matches!(d, LoadDiagnostic::OverrideTargetMissing { .. })),
            "should not warn when override is satisfied: {:?}",
            diags
        );
    }

    #[test]
    fn deterministic_command_ordering() {
        let src = StubSource {
            name: "s".into(),
            commands: vec![
                loaded("c", "", Layer::Embedded, "s"),
                loaded("a", "", Layer::Embedded, "s"),
                loaded("b", "", Layer::Embedded, "s"),
            ],
        };
        let (registry, _) = LayeredBuilder::new().add(src).build();
        let names: Vec<&str> = registry
            .list_commands()
            .iter()
            .map(|c| c.canonical.as_str())
            .collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn layer_rank_ordering() {
        assert!(Layer::Embedded.rank() < Layer::User.rank());
        assert!(Layer::User.rank() < Layer::Project.rank());
        assert!(Layer::Project.rank() < Layer::Local.rank());
        assert_eq!(Layer::Custom(50).rank(), 50);
    }

    #[test]
    fn diagnostic_display() {
        let d = LoadDiagnostic::Shadowed {
            canonical: "deploy".into(),
            shadowed: SourceOrigin {
                source: "embedded".into(),
                layer: Layer::Embedded,
                path: None,
            },
            winner: SourceOrigin {
                source: "project".into(),
                layer: Layer::Project,
                path: Some("/tmp/deploy.md".into()),
            },
        };
        let s = format!("{}", d);
        assert!(s.contains("deploy"));
        assert!(s.contains("embedded"));
        assert!(s.contains("/tmp/deploy.md"));
    }

    #[test]
    fn layer_label_disambiguates_custom() {
        assert_eq!(Layer::Embedded.label(), "embedded");
        assert_eq!(Layer::Custom(7).label(), "custom(7)");
    }
}
