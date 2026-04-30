//! In-memory [`CommandSource`] backed by a `Vec<Command>`.

use crate::model::Command;

use super::{CommandSource, Layer, LoadedCommand, SourceLoad, SourceOrigin};

/// A [`CommandSource`] that wraps a pre-built `Vec<Command>`.
///
/// Use this to expose the application's compiled-in commands as a layer in a
/// [`crate::source::LayeredBuilder`]. The default layer is
/// [`Layer::Embedded`] so that any disk-loaded source naturally takes
/// precedence; call [`EmbeddedSource::with_layer`] to choose a different layer
/// (for example, when seeding test fixtures at the `Project` layer).
///
/// # Examples
///
/// ```
/// use argot_cmd::Command;
/// use argot_cmd::source::{EmbeddedSource, LayeredBuilder};
///
/// let commands = vec![
///     Command::builder("ping").summary("Ping").build().unwrap(),
/// ];
/// let (registry, _diags) = LayeredBuilder::new()
///     .add(EmbeddedSource::new("builtin", commands))
///     .build();
/// assert!(registry.get_command("ping").is_some());
/// ```
pub struct EmbeddedSource {
    name: String,
    layer: Layer,
    commands: Vec<Command>,
    priority: i32,
}

impl EmbeddedSource {
    /// Create a new `EmbeddedSource` at [`Layer::Embedded`] with priority `0`.
    pub fn new(name: impl Into<String>, commands: Vec<Command>) -> Self {
        Self {
            name: name.into(),
            layer: Layer::Embedded,
            commands,
            priority: 0,
        }
    }

    /// Override the layer this source contributes to.
    pub fn with_layer(mut self, layer: Layer) -> Self {
        self.layer = layer;
        self
    }

    /// Set the priority assigned to every command from this source.
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }
}

impl CommandSource for EmbeddedSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn load(&self) -> SourceLoad {
        let commands = self
            .commands
            .iter()
            .cloned()
            .map(|cmd| {
                LoadedCommand::new(
                    cmd,
                    SourceOrigin {
                        source: self.name.clone(),
                        layer: self.layer,
                        path: None,
                    },
                )
                .with_priority(self.priority)
            })
            .collect();
        SourceLoad {
            commands,
            diagnostics: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_source_emits_all_commands() {
        let cmds = vec![
            Command::builder("a").build().unwrap(),
            Command::builder("b").build().unwrap(),
        ];
        let src = EmbeddedSource::new("test", cmds);
        let load = src.load();
        assert_eq!(load.commands.len(), 2);
        assert!(load.diagnostics.is_empty());
        assert_eq!(load.commands[0].origin.source, "test");
        assert!(matches!(load.commands[0].origin.layer, Layer::Embedded));
    }

    #[test]
    fn with_layer_changes_origin() {
        let cmds = vec![Command::builder("x").build().unwrap()];
        let src = EmbeddedSource::new("t", cmds).with_layer(Layer::Local);
        let load = src.load();
        assert!(matches!(load.commands[0].origin.layer, Layer::Local));
    }

    #[test]
    fn with_priority_propagates() {
        let cmds = vec![Command::builder("x").build().unwrap()];
        let src = EmbeddedSource::new("t", cmds).with_priority(42);
        let load = src.load();
        assert_eq!(load.commands[0].priority, 42);
    }
}
