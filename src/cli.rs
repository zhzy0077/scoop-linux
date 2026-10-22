use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "scoop",
    version,
    about = "User-scoped Linux package manager",
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Show command syntax
    Help { command: Option<String> },
    /// Read, set, or remove configuration: config [rm] [name] [value]
    Config {
        #[arg(num_args=0..=2)]
        args: Vec<String>,
    },
    /// Manage Git manifest buckets
    Bucket {
        #[command(subcommand)]
        command: BucketCommand,
    },
    /// Search local package and executable names using a regular expression
    Search { query: Option<String> },
    /// Display package metadata and selected source
    Info {
        app: String,
        #[arg(short, long)]
        verbose: bool,
    },
    /// Print the exact selected JSON manifest
    Cat { app: String },
    /// Install native packages and their dependencies
    Install(Install),
    /// Uninstall managed packages, preserving application data
    Uninstall {
        #[arg(required = true)]
        apps: Vec<String>,
    },
    /// List installed packages, optionally filtering names
    List { query: Option<String> },
    /// Print the active installation directory
    Prefix { app: String },
    /// Locate a managed executable target
    Which { command: String },
    /// Display dependency installation order for a target architecture
    Depends {
        app: String,
        #[arg(short, long)]
        arch: Option<String>,
    },
    /// Refresh Scoop and buckets, or update selected apps
    Update(Update),
    /// Report outdated installed packages
    Status {
        #[arg(short, long)]
        local: bool,
    },
    /// Hold packages at their installed version
    Hold {
        #[arg(required = true)]
        apps: Vec<String>,
    },
    /// Remove package holds
    Unhold {
        #[arg(required = true)]
        apps: Vec<String>,
    },
    /// Activate a retained app@version or restore links
    Reset {
        #[arg(required_unless_present = "all")]
        apps: Vec<String>,
        #[arg(short, long)]
        all: bool,
    },
    /// Remove inactive managed versions
    Cleanup {
        #[arg(required_unless_present = "all")]
        apps: Vec<String>,
        #[arg(short, long)]
        all: bool,
        #[arg(short = 'k', long)]
        cache: bool,
    },
    /// Inspect or remove cached downloads
    Cache {
        #[command(subcommand)]
        command: Option<CacheCommand>,
    },
    /// Download verified artifacts for a selected architecture
    Download(Download),
    /// Print a Scoopfile JSON document (phase 4)
    Export {
        #[arg(short, long)]
        config: bool,
    },
    /// Import a Scoopfile from a path or URL (phase 4)
    Import { file: String },
    /// Diagnose Git, PATH, permissions and installations (phase 4)
    Checkup,
    /// Open a package homepage using xdg-open (phase 4)
    Home { app: String },
    /// Deferred custom subcommands
    Alias {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Deferred manifest authoring
    Create {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Deferred VirusTotal integration
    Virustotal {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}
#[derive(Subcommand, Debug)]
pub enum BucketCommand {
    Add { name: String, repo: Option<String> },
    List,
    Known,
    Rm { name: String },
}
#[derive(Subcommand, Debug)]
pub enum CacheCommand {
    Show {
        apps: Vec<String>,
    },
    Rm {
        #[arg(required_unless_present = "all")]
        apps: Vec<String>,
        #[arg(short, long)]
        all: bool,
    },
}
#[derive(Args, Debug)]
pub struct Install {
    #[arg(required = true)]
    pub apps: Vec<String>,
    #[arg(short, long)]
    pub independent: bool,
    #[arg(short = 'k', long)]
    pub no_cache: bool,
    #[arg(short = 's', long)]
    pub skip_hash_check: bool,
    #[arg(short = 'u', long)]
    pub no_update_scoop: bool,
    #[arg(short, long)]
    pub arch: Option<String>,
}
#[derive(Args, Debug)]
pub struct Download {
    #[arg(required = true)]
    pub apps: Vec<String>,
    #[arg(short, long)]
    pub force: bool,
    #[arg(short = 's', long)]
    pub skip_hash_check: bool,
    #[arg(short = 'u', long)]
    pub no_update_scoop: bool,
    #[arg(short, long)]
    pub arch: Option<String>,
}
#[derive(Args, Debug)]
pub struct Update {
    pub apps: Vec<String>,
    #[arg(short, long)]
    pub force: bool,
    #[arg(short, long)]
    pub independent: bool,
    #[arg(short = 'k', long)]
    pub no_cache: bool,
    #[arg(short = 's', long)]
    pub skip_hash_check: bool,
    #[arg(short, long)]
    pub quiet: bool,
    #[arg(short, long)]
    pub all: bool,
}

pub fn parse() -> Cli {
    // Give feature-specific diagnostics for intentionally excluded syntax.
    for argument in std::env::args().skip(1) {
        if argument == "--" {
            break;
        }
        if matches!(argument.as_str(), "--global" | "-g" | "--purge" | "-p")
            || argument.starts_with("--global=")
            || argument.starts_with("--purge=")
        {
            eprintln!("error: unsupported feature: {argument}");
            std::process::exit(2);
        }
    }
    Cli::parse()
}
