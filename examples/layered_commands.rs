//! Demonstrates layered command sources: built-in commands plus on-disk
//! Markdown overrides with priority/precedence hints.
//!
//! Run with:
//! ```sh
//! cargo run --features markdown-source --example layered_commands
//! ```
//!
//! The example writes two Markdown command files to a temp directory and
//! shows how a project-layer `deploy.md` shadows the embedded `deploy`,
//! while a brand-new `release` command from disk is added without any code
//! change.

use std::fs;
use std::path::Path;

use argot_cmd::source::{EmbeddedSource, Layer, LayeredBuilder};
use argot_cmd::{Cli, Command};

#[cfg(feature = "markdown-source")]
use argot_cmd::source::markdown::MarkdownDirSource;

fn write(path: &Path, name: &str, body: &str) {
    fs::write(path.join(name), body).unwrap();
}

#[cfg(feature = "markdown-source")]
fn main() {
    let tmp = tempdir_in_target();

    // 1. Embedded (compile-time) commands — the binary's defaults.
    let embedded = vec![
        Command::builder("deploy")
            .summary("Deploy (built-in)")
            .build()
            .unwrap(),
        Command::builder("status")
            .summary("Show status")
            .build()
            .unwrap(),
    ];

    // 2. Project-layer Markdown commands — typically committed to a repo's
    //    `.<app>/commands/` directory.
    write(
        &tmp,
        "deploy.md",
        r#"---
name: deploy
summary: Deploy (project override)
priority: 10
overrides: deploy
mutating: true
best_practices:
  - "Always run with --dry-run first"
anti_patterns:
  - "Deploy on Fridays"
---

Deploys the application to a target environment using the project's custom
configuration. Overrides the built-in `deploy` command.
"#,
    );
    write(
        &tmp,
        "release.md",
        r#"---
name: release
summary: Cut a versioned release
semantic_aliases:
  - tag a release
  - publish a version
---

Tags a new semantic version, builds release artifacts, and publishes them.
"#,
    );

    // 3. Build a Cli directly from the layered sources. Cli::from_layered
    //    returns the diagnostics so the application can choose how to surface
    //    them (we just print to stderr here).
    let (cli, diagnostics) = Cli::from_layered(
        LayeredBuilder::new()
            .add(EmbeddedSource::new("builtin", embedded))
            .add(MarkdownDirSource::new("project", &tmp, Layer::Project)),
    );

    println!("Loaded {} commands:", cli.commands().len());
    for cmd in cli.commands() {
        println!("  {:<10}  {}", cmd.canonical, cmd.summary);
    }

    // Partition diagnostics into errors (sources that failed to contribute
    // commands at all) and warnings (shadows / schema warnings / missing
    // override targets). A real application typically aborts startup on errors
    // and prints warnings only in dev mode. The example shows both halves so
    // downstream copy-paste callers don't conflate them.
    let (errors, warnings): (Vec<_>, Vec<_>) = diagnostics.iter().partition(|d| d.is_error());
    if !errors.is_empty() {
        eprintln!("\nLoad errors (these would abort startup in a real app):");
        for d in &errors {
            eprintln!("  ! {}", d);
        }
    }
    if !warnings.is_empty() {
        eprintln!("\nWarnings:");
        for d in &warnings {
            eprintln!("  - {}", d);
        }
    }

    // The project-layer deploy shadows the built-in one.
    let deploy = cli
        .commands()
        .iter()
        .find(|c| c.canonical == "deploy")
        .unwrap();
    assert_eq!(deploy.summary, "Deploy (project override)");
    assert!(deploy.mutating);

    // The release command is brand-new, contributed entirely from disk.
    assert!(cli.commands().iter().any(|c| c.canonical == "release"));

    let _ = fs::remove_dir_all(&tmp);
}

#[cfg(not(feature = "markdown-source"))]
fn main() {
    eprintln!(
        "This example requires the `markdown-source` feature.\n\
         Run with: cargo run --features markdown-source --example layered_commands"
    );
}

#[cfg(feature = "markdown-source")]
fn tempdir_in_target() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("argot-layered-example-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}
