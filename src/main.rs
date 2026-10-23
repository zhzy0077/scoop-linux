fn main() {
    if let Ok(config) = scoop_linux::config::Config::load()
        && let Err(error) = scoop_linux::journal::recover(&config)
    {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
    let cli = scoop_linux::cli::parse();
    let missing_command_exit = matches!(&cli.command, scoop_linux::cli::Command::Which { .. });
    if let Err(error) = scoop_linux::commands::run(cli.command) {
        eprintln!("error: {error:#}");
        std::process::exit(if missing_command_exit { 2 } else { 1 });
    }
}
