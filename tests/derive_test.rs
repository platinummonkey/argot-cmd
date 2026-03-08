#[cfg(feature = "derive")]
mod tests {
    use argot::ArgotCommand;

    #[derive(ArgotCommand)]
    #[argot(
        summary = "Deploy the application",
        alias = "d",
        best_practice = "always dry-run first"
    )]
    struct Deploy {
        #[argot(positional, required, description = "target environment")]
        env: String,

        #[argot(flag, short = 'n', description = "dry run mode")]
        dry_run: bool,

        #[argot(
            flag,
            short = 'o',
            takes_value,
            description = "output format",
            default = "text"
        )]
        output: String,
    }

    #[test]
    fn test_canonical_name_from_struct() {
        let cmd = Deploy::command();
        assert_eq!(cmd.canonical, "deploy");
    }

    #[test]
    fn test_summary_and_alias() {
        let cmd = Deploy::command();
        assert_eq!(cmd.summary, "Deploy the application");
        assert!(cmd.aliases.contains(&"d".to_string()));
    }

    #[test]
    fn test_positional_argument() {
        let cmd = Deploy::command();
        let arg = cmd.arguments.iter().find(|a| a.name == "env").unwrap();
        assert!(arg.required);
        assert_eq!(arg.description, "target environment");
    }

    #[test]
    fn test_flag_boolean() {
        let cmd = Deploy::command();
        let flag = cmd.flags.iter().find(|f| f.name == "dry-run").unwrap();
        assert_eq!(flag.short, Some('n'));
        assert!(!flag.takes_value);
    }

    #[test]
    fn test_flag_with_value_and_default() {
        let cmd = Deploy::command();
        let flag = cmd.flags.iter().find(|f| f.name == "output").unwrap();
        assert!(flag.takes_value);
        assert_eq!(flag.default.as_deref(), Some("text"));
    }

    #[test]
    fn test_best_practice() {
        let cmd = Deploy::command();
        assert!(cmd
            .best_practices
            .contains(&"always dry-run first".to_string()));
    }

    #[derive(ArgotCommand)]
    #[argot(canonical = "explicit-name")]
    struct SomeOtherCommand {}

    #[test]
    fn test_canonical_override() {
        let cmd = SomeOtherCommand::command();
        assert_eq!(cmd.canonical, "explicit-name");
    }

    #[test]
    fn test_derive_command_parses_via_parser() {
        use argot::{Parser, Registry};

        // Build registry from derived command
        let cmd = Deploy::command();
        let registry = Registry::new(vec![cmd]);
        let parser = Parser::new(registry.commands());

        // Parse a real argv slice
        let parsed = parser.parse(&["deploy", "production"]).unwrap();
        assert_eq!(parsed.command.canonical, "deploy");
        assert_eq!(parsed.args["env"], "production");
        assert_eq!(
            parsed.flags.get("dry-run").map(|s| s.as_str()),
            None, // not provided
        );
    }

    #[test]
    fn test_derive_command_flag_parsing() {
        use argot::{Parser, Registry};

        let cmd = Deploy::command();
        let registry = Registry::new(vec![cmd]);
        let parser = Parser::new(registry.commands());

        let parsed = parser
            .parse(&["deploy", "staging", "--dry-run", "--output=json"])
            .unwrap();
        assert_eq!(parsed.args["env"], "staging");
        assert_eq!(parsed.flags["dry-run"], "true");
        assert_eq!(parsed.flags["output"], "json");
    }

    #[test]
    fn test_derive_missing_required_argument() {
        use argot::{ParseError, Parser, Registry};

        let cmd = Deploy::command();
        let registry = Registry::new(vec![cmd]);
        let parser = Parser::new(registry.commands());

        // "env" is required — missing should error
        let result = parser.parse(&["deploy"]);
        assert!(
            matches!(result, Err(ParseError::MissingArgument(ref s)) if s == "env"),
            "expected MissingArgument(env), got {:?}",
            result
        );
    }

    // -----------------------------------------------------------------------
    // Edge-case tests
    // -----------------------------------------------------------------------

    #[derive(ArgotCommand)]
    #[argot(summary = "Print version", canonical = "version")]
    struct VersionCmd;

    #[test]
    fn test_derive_unit_struct() {
        let cmd = VersionCmd::command();
        assert_eq!(cmd.canonical, "version");
        assert_eq!(cmd.summary, "Print version");
        assert!(cmd.arguments.is_empty());
        assert!(cmd.flags.is_empty());
    }

    #[derive(ArgotCommand)]
    #[argot(
        canonical = "full-cmd",
        summary = "Full example",
        description = "Shows every struct-level attribute",
        alias = "fc",
        alias = "full",
        best_practice = "always dry-run first",
        anti_pattern = "skipping tests"
    )]
    struct FullCmd;

    #[test]
    fn test_derive_all_struct_attrs() {
        let cmd = FullCmd::command();
        assert_eq!(cmd.canonical, "full-cmd");
        assert_eq!(cmd.summary, "Full example");
        assert!(!cmd.description.is_empty());
        assert!(cmd.aliases.contains(&"fc".to_string()));
        assert!(cmd.aliases.contains(&"full".to_string()));
        assert!(cmd.best_practices.iter().any(|s| s.contains("dry-run")));
        assert!(cmd.anti_patterns.iter().any(|s| s.contains("skipping")));
    }

    #[derive(ArgotCommand)]
    #[argot(canonical = "skip-test")]
    struct SkipFields {
        _internal: String, // no #[argot] → skipped
        _count: u32,       // no #[argot] → skipped
    }

    #[test]
    fn test_derive_unannotated_fields_skipped() {
        let cmd = SkipFields::command();
        assert!(cmd.arguments.is_empty());
        assert!(cmd.flags.is_empty());
    }

    #[derive(ArgotCommand)]
    struct DeployToStaging;

    #[test]
    fn test_derive_camel_to_kebab_canonical() {
        assert_eq!(DeployToStaging::command().canonical, "deploy-to-staging");
    }

    #[derive(ArgotCommand)]
    struct NamingCheck {
        #[argot(positional)]
        source_path: String,
        #[argot(flag, takes_value)]
        target_dir: String,
    }

    #[test]
    fn test_derive_snake_to_kebab_field_names() {
        let cmd = NamingCheck::command();
        assert!(
            cmd.arguments.iter().any(|a| a.name == "source-path"),
            "expected source-path"
        );
        assert!(
            cmd.flags.iter().any(|f| f.name == "target-dir"),
            "expected target-dir"
        );
    }

    #[derive(ArgotCommand)]
    struct MultiPos {
        #[argot(positional, required)]
        first: String,
        #[argot(positional)]
        second: String,
    }

    #[test]
    fn test_derive_positional_order_preserved() {
        let cmd = MultiPos::command();
        assert_eq!(cmd.arguments.len(), 2);
        assert_eq!(cmd.arguments[0].name, "first");
        assert!(cmd.arguments[0].required);
        assert_eq!(cmd.arguments[1].name, "second");
        assert!(!cmd.arguments[1].required);
    }

    #[derive(ArgotCommand)]
    struct FlagTest {
        #[argot(
            flag,
            short = 'f',
            takes_value,
            description = "output format",
            default = "json"
        )]
        format: String,
        #[argot(flag, short = 'n', description = "dry run")]
        dry_run: bool,
    }

    #[test]
    fn test_derive_flag_all_attrs() {
        let cmd = FlagTest::command();
        let fmt = cmd.flags.iter().find(|f| f.name == "format").unwrap();
        assert_eq!(fmt.short, Some('f'));
        assert!(fmt.takes_value);
        assert_eq!(fmt.default.as_deref(), Some("json"));
        let dry = cmd.flags.iter().find(|f| f.name == "dry-run").unwrap();
        assert_eq!(dry.short, Some('n'));
        assert!(!dry.takes_value);
    }

    #[derive(ArgotCommand)]
    #[argot(summary = "Run a job", alias = "j")]
    struct RunJob {
        #[argot(positional, required, description = "job name")]
        name: String,
        #[argot(
            flag,
            short = 'p',
            takes_value,
            description = "parallelism",
            default = "1"
        )]
        parallel: String,
    }

    #[test]
    fn test_derive_e2e_registry_and_parser() {
        use argot::{Parser, Registry};
        let registry = Registry::new(vec![RunJob::command()]);
        let parser = Parser::new(registry.commands());

        // alias resolution + default flag
        let p = parser.parse(&["j", "nightly"]).unwrap();
        assert_eq!(p.command.canonical, "run-job");
        assert_eq!(p.args["name"], "nightly");
        assert_eq!(p.flags["parallel"], "1");

        // explicit flag overrides default
        let p2 = parser.parse(&["run-job", "daily", "--parallel=4"]).unwrap();
        assert_eq!(p2.args["name"], "daily");
        assert_eq!(p2.flags["parallel"], "4");
    }
}
