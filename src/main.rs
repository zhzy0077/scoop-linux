fn main() {
    let arguments: Vec<_> = std::env::args().collect();
    if let Ok(config) = scoop_linux::config::Config::load()
        && let Err(error) = scoop_linux::journal::recover(&config)
    {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
    if arguments.get(1).is_some_and(|arg| arg == "--bootstrap") {
        let result = (|| -> anyhow::Result<()> {
            anyhow::ensure!(
                arguments.len() == 3,
                "usage: scoop --bootstrap <release-catalog>"
            );
            let config = scoop_linux::config::Config::load()?;
            scoop_linux::journal::recover(&config)?;
            scoop_linux::self_update::bootstrap(&config, &arguments[2])
        })();
        if let Err(error) = result {
            eprintln!("error: {error:#}");
            std::process::exit(1);
        }
        return;
    }
    let cli = scoop_linux::cli::parse();
    let missing_command_exit = matches!(&cli.command, scoop_linux::cli::Command::Which { .. });
    if let Err(error) = scoop_linux::commands::run(cli.command) {
        eprintln!("error: {error:#}");
        std::process::exit(if missing_command_exit { 2 } else { 1 });
    }
}
