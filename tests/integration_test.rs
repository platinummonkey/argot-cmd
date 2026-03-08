use std::sync::Arc;

use argot::{
    render::{render_help, render_markdown},
    Argument, Command, Example, Flag, Parser, Registry,
};

fn build_registry() -> Registry {
    let list = Command::builder("list")
        .alias("ls")
        .summary("List all items")
        .description("Lists items, optionally filtered.")
        .argument(
            Argument::builder("filter")
                .description("optional filter string")
                .build()
                .unwrap(),
        )
        .flag(
            Flag::builder("verbose")
                .short('v')
                .description("verbose output")
                .build()
                .unwrap(),
        )
        .example(Example::new("list everything", "myapp list"))
        .best_practice("pipe output through less for large lists")
        .anti_pattern("list without a filter on huge datasets")
        .build()
        .unwrap();

    let remote_add = Command::builder("add")
        .summary("Add a remote")
        .argument(
            Argument::builder("name")
                .description("remote name")
                .required()
                .build()
                .unwrap(),
        )
        .argument(
            Argument::builder("url")
                .description("remote URL")
                .required()
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();

    let remote_remove = Command::builder("remove")
        .alias("rm")
        .summary("Remove a remote")
        .argument(
            Argument::builder("name")
                .description("remote name")
                .required()
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();

    let remote = Command::builder("remote")
        .summary("Manage remotes")
        .subcommand(remote_add)
        .subcommand(remote_remove)
        .build()
        .unwrap();

    let run = Command::builder("run")
        .summary("Run a script")
        .handler(Arc::new(|_parsed| {
            println!("run handler called");
            Ok(())
        }))
        .build()
        .unwrap();

    Registry::new(vec![list, remote, run])
}

#[test]
fn test_registry_list_and_get() {
    let r = build_registry();
    assert_eq!(r.list_commands().len(), 3);
    assert!(r.get_command("list").is_some());
    assert!(r.get_command("missing").is_none());
}

#[test]
fn test_registry_get_subcommand() {
    let r = build_registry();
    assert_eq!(
        r.get_subcommand(&["remote", "add"]).unwrap().canonical,
        "add"
    );
    assert_eq!(
        r.get_subcommand(&["remote", "remove"]).unwrap().canonical,
        "remove"
    );
    assert!(r.get_subcommand(&["remote", "nope"]).is_none());
}

#[test]
fn test_registry_search() {
    let r = build_registry();
    let results = r.search("remote");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].canonical, "remote");

    assert!(r.search("zzz").is_empty());
}

#[test]
fn test_registry_to_json() {
    let r = build_registry();
    let json = r.to_json().unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v.is_array());
    // handler field should be absent (serde skip)
    assert!(json.contains("\"canonical\""));
    assert!(!json.contains("\"handler\""));
}

#[test]
fn test_parse_flat_command_with_alias() {
    let r = build_registry();
    let parser = Parser::new(r.commands());

    let parsed = parser.parse(&["ls"]).unwrap();
    assert_eq!(parsed.command.canonical, "list");
}

#[test]
fn test_parse_flag_boolean() {
    let r = build_registry();
    let parser = Parser::new(r.commands());

    let parsed = parser.parse(&["list", "-v"]).unwrap();
    assert_eq!(parsed.flags["verbose"], "true");
}

#[test]
fn test_parse_subcommand_two_levels() {
    let r = build_registry();
    let parser = Parser::new(r.commands());

    let parsed = parser
        .parse(&["remote", "add", "origin", "https://example.com"])
        .unwrap();
    assert_eq!(parsed.command.canonical, "add");
    assert_eq!(parsed.args["name"], "origin");
    assert_eq!(parsed.args["url"], "https://example.com");
}

#[test]
fn test_parse_subcommand_alias() {
    let r = build_registry();
    let parser = Parser::new(r.commands());

    let parsed = parser.parse(&["remote", "rm", "origin"]).unwrap();
    assert_eq!(parsed.command.canonical, "remove");
    assert_eq!(parsed.args["name"], "origin");
}

#[test]
fn test_parse_missing_required_arg() {
    let r = build_registry();
    let parser = Parser::new(r.commands());

    // "remote add" requires both name and url
    let err = parser.parse(&["remote", "add"]).unwrap_err();
    assert!(
        matches!(err, argot::ParseError::MissingArgument(_)),
        "expected MissingArgument, got {:?}",
        err
    );
}

#[test]
fn test_render_help_pipeline() {
    let r = build_registry();
    let cmd = r.get_command("list").unwrap();
    let help = render_help(cmd);

    assert!(help.contains("NAME"));
    assert!(help.contains("list"));
    assert!(help.contains("SUMMARY"));
    assert!(help.contains("EXAMPLES"));
    assert!(help.contains("BEST PRACTICES"));
    assert!(help.contains("ANTI-PATTERNS"));
}

#[test]
fn test_render_markdown_pipeline() {
    let r = build_registry();
    let cmd = r.get_command("list").unwrap();
    let md = render_markdown(cmd);
    assert!(md.starts_with("# list"));
}

#[test]
fn test_handler_is_callable() {
    let r = build_registry();
    let cmd = r.get_command("run").unwrap();
    assert!(cmd.handler.is_some());
    // Invoke the handler with a minimal ParsedCommand
    use argot::ParsedCommand;
    use std::collections::HashMap;
    let parsed = ParsedCommand {
        command: cmd,
        args: HashMap::new(),
        flags: HashMap::new(),
    };
    let result = (cmd.handler.as_ref().unwrap())(&parsed);
    assert!(result.is_ok());
}

#[test]
fn test_full_pipeline() {
    // Build → Register → Parse → Render
    let r = build_registry();
    let parser = Parser::new(r.commands());

    let parsed = parser.parse(&["list", "needle"]).unwrap();
    assert_eq!(parsed.command.canonical, "list");
    assert_eq!(
        parsed.args.get("filter").map(String::as_str),
        Some("needle")
    );

    let help = render_help(parsed.command);
    assert!(!help.is_empty());

    let md = render_markdown(parsed.command);
    assert!(md.starts_with("# list"));
}

#[test]
fn test_serde_round_trip_with_subcommands() {
    let r = build_registry(); // uses the existing helper
    let json = r.to_json().unwrap();

    // Re-parse the JSON into a Vec<Command>
    let commands: Vec<argot::Command> = serde_json::from_str(&json).unwrap();

    // Verify structure survived round-trip
    let remote = commands
        .iter()
        .find(|c| c.canonical == "remote")
        .expect("remote not found");
    assert!(
        !remote.subcommands.is_empty(),
        "subcommands should survive serde"
    );

    let add_sub = remote.subcommands.iter().find(|c| c.canonical == "add");
    assert!(
        add_sub.is_some(),
        "remote.add subcommand should survive serde"
    );

    // Handlers are skipped — verify they are None after deserialization
    let run = commands
        .iter()
        .find(|c| c.canonical == "run")
        .expect("run not found");
    assert!(
        run.handler.is_none(),
        "handler must be None after deserialization"
    );

    // Re-build a registry from the deserialized commands and verify parsing still works
    let new_registry = argot::Registry::new(commands);
    let parser = argot::Parser::new(new_registry.commands());
    let parsed = parser.parse(&["list"]).unwrap();
    assert_eq!(parsed.command.canonical, "list");
}

#[test]
fn test_command_named_help_parses_correctly() {
    // A user-defined "help" command should be parseable; it only conflicts
    // with Cli's --help flag, not with direct Parser use.
    let help_cmd = argot::Command::builder("help")
        .summary("Show help information")
        .build()
        .unwrap();
    let registry = argot::Registry::new(vec![help_cmd]);
    let parser = argot::Parser::new(registry.commands());
    let parsed = parser.parse(&["help"]).unwrap();
    assert_eq!(parsed.command.canonical, "help");
}

#[test]
fn test_command_named_version_parses_correctly() {
    let version_cmd = argot::Command::builder("version")
        .summary("Print version information")
        .build()
        .unwrap();
    let registry = argot::Registry::new(vec![version_cmd]);
    let parser = argot::Parser::new(registry.commands());
    let parsed = parser.parse(&["version"]).unwrap();
    assert_eq!(parsed.command.canonical, "version");
}

// ================================================================
// ParseError variant coverage
// ================================================================

#[test]
fn test_parse_error_no_command() {
    let cmds = vec![Command::builder("run").build().unwrap()];
    assert!(matches!(
        Parser::new(&cmds).parse(&[]),
        Err(argot::ParseError::NoCommand)
    ));
}

#[test]
fn test_parse_error_unknown_command() {
    let cmds = vec![Command::builder("run").build().unwrap()];
    assert!(matches!(
        Parser::new(&cmds).parse(&["nope"]),
        Err(argot::ParseError::Resolve(
            argot::ResolveError::Unknown { .. }
        ))
    ));
}

#[test]
fn test_parse_error_ambiguous_command() {
    let cmds = vec![
        Command::builder("fetch").build().unwrap(),
        Command::builder("format").build().unwrap(),
    ];
    assert!(matches!(
        Parser::new(&cmds).parse(&["f"]),
        Err(argot::ParseError::Resolve(
            argot::ResolveError::Ambiguous { .. }
        ))
    ));
}

#[test]
fn test_parse_error_missing_argument() {
    let cmds = vec![Command::builder("get")
        .argument(Argument::builder("id").required().build().unwrap())
        .build()
        .unwrap()];
    match Parser::new(&cmds).parse(&["get"]) {
        Err(argot::ParseError::MissingArgument(n)) => assert_eq!(n, "id"),
        other => panic!("expected MissingArgument(id), got {:?}", other),
    }
}

#[test]
fn test_parse_error_unexpected_argument() {
    let cmds = vec![Command::builder("run").build().unwrap()];
    match Parser::new(&cmds).parse(&["run", "extra"]) {
        Err(argot::ParseError::UnexpectedArgument(v)) => assert_eq!(v, "extra"),
        other => panic!("expected UnexpectedArgument, got {:?}", other),
    }
}

#[test]
fn test_parse_error_missing_required_flag() {
    let cmds = vec![Command::builder("deploy")
        .flag(
            Flag::builder("env")
                .takes_value()
                .required()
                .build()
                .unwrap(),
        )
        .build()
        .unwrap()];
    match Parser::new(&cmds).parse(&["deploy"]) {
        Err(argot::ParseError::MissingFlag(n)) => assert_eq!(n, "env"),
        other => panic!("expected MissingFlag(env), got {:?}", other),
    }
}

#[test]
fn test_parse_error_flag_missing_value() {
    let cmds = vec![Command::builder("build")
        .flag(Flag::builder("target").takes_value().build().unwrap())
        .build()
        .unwrap()];
    match Parser::new(&cmds).parse(&["build", "--target"]) {
        Err(argot::ParseError::FlagMissingValue { name }) => assert_eq!(name, "target"),
        other => panic!("expected FlagMissingValue, got {:?}", other),
    }
}

#[test]
fn test_parse_error_unknown_flag() {
    let cmds = vec![Command::builder("run").build().unwrap()];
    match Parser::new(&cmds).parse(&["run", "--ghost"]) {
        Err(argot::ParseError::UnknownFlag(n)) => assert!(n.contains("ghost")),
        other => panic!("expected UnknownFlag, got {:?}", other),
    }
}

#[test]
fn test_parse_error_unknown_subcommand() {
    let cmds = vec![Command::builder("remote")
        .subcommand(Command::builder("add").build().unwrap())
        .build()
        .unwrap()];
    match Parser::new(&cmds).parse(&["remote", "bogus"]) {
        Err(argot::ParseError::UnknownSubcommand { parent, got }) => {
            assert_eq!(parent, "remote");
            assert_eq!(got, "bogus");
        }
        other => panic!("expected UnknownSubcommand, got {:?}", other),
    }
}

#[test]
fn test_parse_error_invalid_choice() {
    let cmds = vec![Command::builder("build")
        .flag(
            Flag::builder("format")
                .takes_value()
                .choices(["json", "yaml"])
                .build()
                .unwrap(),
        )
        .build()
        .unwrap()];
    match Parser::new(&cmds).parse(&["build", "--format=xml"]) {
        Err(argot::ParseError::InvalidChoice {
            flag,
            value,
            choices,
        }) => {
            assert_eq!(flag, "format");
            assert_eq!(value, "xml");
            assert!(choices.contains(&"json".to_string()));
        }
        other => panic!("expected InvalidChoice, got {:?}", other),
    }
}

// ================================================================
// Positive paths for recently added features
// ================================================================

#[test]
fn test_choices_valid_value_accepted() {
    let cmds = vec![Command::builder("build")
        .flag(
            Flag::builder("fmt")
                .takes_value()
                .choices(["json", "yaml"])
                .build()
                .unwrap(),
        )
        .build()
        .unwrap()];
    let parsed = Parser::new(&cmds).parse(&["build", "--fmt=yaml"]).unwrap();
    assert_eq!(parsed.flags["fmt"], "yaml");
}

#[test]
fn test_repeatable_boolean_flag_count() {
    let cmds = vec![Command::builder("run")
        .flag(
            Flag::builder("verbose")
                .short('v')
                .repeatable()
                .build()
                .unwrap(),
        )
        .build()
        .unwrap()];
    let parsed = Parser::new(&cmds)
        .parse(&["run", "-v", "-v", "-v"])
        .unwrap();
    assert_eq!(parsed.flags["verbose"], "3");
    assert_eq!(parsed.flag_count("verbose"), 3);
}

#[test]
fn test_repeatable_value_flag_collects() {
    let cmds = vec![Command::builder("run")
        .flag(
            Flag::builder("tag")
                .takes_value()
                .repeatable()
                .build()
                .unwrap(),
        )
        .build()
        .unwrap()];
    let parsed = Parser::new(&cmds)
        .parse(&["run", "--tag=alpha", "--tag=beta"])
        .unwrap();
    let tags: Vec<String> = serde_json::from_str(&parsed.flags["tag"]).unwrap();
    assert_eq!(tags, ["alpha", "beta"]);
}

#[test]
fn test_flag_present_and_absent() {
    let cmds = vec![Command::builder("x")
        .flag(Flag::builder("v").build().unwrap())
        .flag(
            Flag::builder("out")
                .takes_value()
                .default_value("text")
                .build()
                .unwrap(),
        )
        .build()
        .unwrap()];
    let parsed = Parser::new(&cmds).parse(&["x", "--v"]).unwrap();
    assert!(parsed.flag("v").is_some());
    assert!(parsed.flag("out").is_some()); // default applied
    assert!(parsed.flag("other").is_none());
}
