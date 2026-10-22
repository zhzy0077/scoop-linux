use clap::{CommandFactory, Parser};
use scoop_linux::cli::Cli;
use std::process::Command;
#[test]
fn every_mvp_command_has_documented_syntax() {
    let commands = [
        "help",
        "config",
        "bucket",
        "search",
        "info",
        "cat",
        "install",
        "uninstall",
        "list",
        "prefix",
        "which",
        "depends",
        "update",
        "status",
        "hold",
        "unhold",
        "reset",
        "cleanup",
        "cache",
        "download",
        "export",
        "import",
        "checkup",
        "home",
    ];
    let mut cli = Cli::command();
    cli.debug_assert();
    cli = Cli::command();
    for name in commands {
        let command = cli.find_subcommand_mut(name).unwrap();
        assert!(command.get_about().is_some());
        let help = command.render_long_help().to_string();
        assert!(help.contains("Usage:"), "{name}");
    }
    assert!(cli.find_subcommand("shim").is_none());
    assert_eq!(cli.get_subcommands().count(), 27);
}
#[test]
fn supported_options_and_multiple_operands() {
    for args in [
        vec!["install", "a", "b", "-iksu", "-a", "aarch64"],
        vec![
            "install",
            "a",
            "--independent",
            "--no-cache",
            "--skip-hash-check",
            "--no-update-scoop",
            "--arch",
            "x86_64",
        ],
        vec!["update", "*", "-fiksqa"],
        vec![
            "update",
            "--force",
            "--independent",
            "--no-cache",
            "--skip-hash-check",
            "--quiet",
            "--all",
        ],
        vec!["download", "a", "b", "-fsu", "-a", "riscv64"],
        vec![
            "download",
            "a",
            "--force",
            "--skip-hash-check",
            "--no-update-scoop",
            "--arch",
            "arm64",
        ],
        vec!["depends", "a", "--arch", "64bit"],
        vec!["depends", "a", "-a", "64bit"],
        vec!["cleanup", "-ak"],
        vec!["cleanup", "--all", "--cache"],
        vec!["reset", "-a"],
        vec!["reset", "--all"],
        vec!["cache", "rm", "-a"],
        vec!["cache", "rm", "--all"],
        vec!["status", "-l"],
        vec!["status", "--local"],
        vec!["info", "a", "-v"],
        vec!["info", "a", "--verbose"],
        vec!["export", "-c"],
        vec!["export", "--config"],
        vec!["uninstall", "a", "b"],
        vec!["hold", "a", "b"],
        vec!["unhold", "a", "b"],
        vec!["reset", "a@1", "b"],
        vec!["cache", "show"],
        vec!["update"],
    ] {
        assert!(
            Cli::try_parse_from(std::iter::once("scoop").chain(args.clone())).is_ok(),
            "{args:?}"
        );
    }
    for args in [
        vec!["install"],
        vec!["cat"],
        vec!["reset"],
        vec!["cleanup"],
        vec!["cache", "rm"],
        vec!["shim"],
        vec!["install", "a", "--global"],
        vec!["uninstall", "a", "--purge"],
    ] {
        assert!(
            Cli::try_parse_from(std::iter::once("scoop").chain(args.clone())).is_err(),
            "{args:?}"
        );
    }
}
#[test]
fn explicit_errors_and_deferred_handlers() {
    let temp = tempfile::tempdir().unwrap();
    for (args, expected) in [
        (vec!["alias"], "unsupported feature: alias"),
        (vec!["create"], "unsupported feature: create"),
        (vec!["virustotal"], "unsupported feature: virustotal"),
        (
            vec!["install", "a", "--global"],
            "unsupported feature: --global",
        ),
        (
            vec!["uninstall", "a", "--purge"],
            "unsupported feature: --purge",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_scoop"))
            .args(args)
            .env("HOME", temp.path())
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
            .env("SCOOP", temp.path().join("root"))
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(!temp.path().join("root").exists());
}
#[test]
fn foreign_download_resolves_and_foreign_install_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tool.json");
    let artifact = temp.path().join("artifact");
    std::fs::write(&artifact, b"foreign content for download").unwrap();
    std::fs::write(&path, serde_json::json!({"version":"1","homepage":"https://example.org","license":"MIT","url":reqwest::Url::from_file_path(&artifact).unwrap().as_str(),"hash":scoop_linux::download::digest(&artifact).unwrap(),"architecture":{"riscv64":{},"arm":{}}}).to_string()).unwrap();
    for (command, arch, expected) in [
        ("install", "riscv64", "foreign architecture"),
        ("download", "riscv64", ""),
        ("depends", "riscv64", ""),
        ("install", "armv7l", "foreign architecture"),
        ("download", "armv7l", ""),
        ("depends", "armv7l", ""),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_scoop"))
            .args([command, path.to_str().unwrap(), "--arch", arch])
            .env("HOME", temp.path())
            .env("SCOOP", temp.path().join("root"))
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            expected.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(!temp.path().join("root/apps").exists());
}
