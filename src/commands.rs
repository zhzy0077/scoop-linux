use crate::{
    buckets::Buckets,
    cli::{Cli, Command},
    config::Config,
    manifest::Manifest,
    platform,
    resolve::Resolver,
};
use anyhow::{Result, bail};
use clap::CommandFactory;
use regex::RegexBuilder;
use std::fs;

pub fn run(command: Command) -> Result<()> {
    if let Command::Help { command } = command {
        let mut cli = Cli::command();
        if let Some(name) = command {
            let sub = cli
                .find_subcommand_mut(&name)
                .ok_or_else(|| anyhow::anyhow!("unknown command '{name}'"))?;
            *sub = sub.clone().bin_name(format!("scoop {name}"));
            sub.print_long_help()?;
        } else {
            cli.print_long_help()?;
        }
        println!();
        return Ok(());
    }
    let mut config = Config::load()?;
    crate::journal::recover(&config)?;
    if matches!(
        &command,
        Command::Search { .. }
            | Command::Info { .. }
            | Command::Cat { .. }
            | Command::Depends { .. }
            | Command::Install(_)
            | Command::Download(_)
            | Command::Update(_)
            | Command::Status { local: false }
            | Command::Bucket {
                command: crate::cli::BucketCommand::List
            }
    ) {
        Buckets::initialize(&config)?;
    }
    let resolver = Resolver::new(&config);
    match command {
        Command::Config { args } => config.command(&args),
        Command::Bucket { command } => match &command {
            crate::cli::BucketCommand::Add { name, repo } if name == "main" && repo.is_none() => {
                let configured = std::env::var("SCOOP_MAIN_BUCKET")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        config
                            .values
                            .get("main_bucket_url")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned)
                    });
                Buckets::new(&config.root).add(name, configured.as_deref())
            }
            crate::cli::BucketCommand::Known => {
                for (name, source) in Buckets::known() {
                    let configured = if name == "main" {
                        std::env::var("SCOOP_MAIN_BUCKET")
                            .ok()
                            .filter(|s| !s.is_empty())
                            .or_else(|| {
                                config
                                    .values
                                    .get("main_bucket_url")
                                    .and_then(|v| v.as_str())
                                    .map(str::to_owned)
                            })
                    } else {
                        None
                    };
                    println!(
                        "{name}\t{}",
                        configured
                            .as_deref()
                            .or(source.as_str())
                            .unwrap_or("<unconfigured>")
                    );
                }
                Ok(())
            }
            _ => Buckets::new(&config.root).command(&command),
        },
        Command::Cat { app } => {
            let raw = resolver.manifest_text(&app)?;
            if let Err(error) = Manifest::parse(raw.clone()) {
                eprintln!("warning: manifest validation: {error}");
            }
            print!("{raw}");
            Ok(())
        }
        Command::Info { app, verbose } => {
            let resolved = resolver.resolve(&app)?;
            let value = &resolved.manifest.value;
            println!(
                "Name:        {}\nVersion:     {}\nSource:      {}\nHomepage:    {}\nLicense:     {}",
                resolved.name,
                resolved.manifest.version(),
                resolved.source,
                value["homepage"].as_str().unwrap_or_default(),
                license(value.get("license"))
            );
            if let Some(description) = value.get("description").and_then(|v| v.as_str()) {
                println!("Description: {description}");
            }
            if !resolved.alternatives.is_empty() {
                println!("Also in:     {}", resolved.alternatives.join(", "));
            }
            if verbose {
                println!("Provenance:  {}", resolved.source.provenance());
                let arch =
                    platform::select(None, config.architecture(), &platform::host()?, false)?;
                let target = match resolved.manifest.target(&arch) {
                    Ok(target) => target,
                    Err(error) => {
                        println!("Availability: {error}");
                        return Ok(());
                    }
                };
                println!("Architecture: {}", target.architecture);
                for artifact in target.artifacts {
                    println!(
                        "URL:         {}\nSHA256:      {}",
                        artifact.url,
                        artifact.hash.unwrap_or_default()
                    );
                }
                println!(
                    "Binaries:    {}\nDependencies: {}",
                    target
                        .bins
                        .iter()
                        .map(|bin| bin.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    target.dependencies.join(", ")
                );
            }
            Ok(())
        }
        Command::Search { query } => search(&resolver.buckets, query.as_deref().unwrap_or("")),
        Command::Install(options) => {
            let arch = platform::select(
                options.arch.as_deref(),
                config.architecture(),
                &platform::host()?,
                true,
            )?;
            crate::maintenance::automatic(&config, options.no_update)?;
            crate::lifecycle::install(
                &config,
                &options.apps,
                &arch,
                options.independent,
                crate::download::Options {
                    no_cache: options.no_cache,
                    skip_hash: options.skip_hash_check,
                    force: false,
                    progress: true,
                },
            )
        }
        Command::Download(options) => {
            let arch = platform::select(
                options.arch.as_deref(),
                config.architecture(),
                &platform::host()?,
                false,
            )?;
            crate::maintenance::automatic(&config, options.no_update)?;
            crate::lifecycle::download(
                &config,
                &options.apps,
                &arch,
                crate::download::Options {
                    force: options.force,
                    skip_hash: options.skip_hash_check,
                    no_cache: false,
                    progress: true,
                },
            )
        }
        Command::Depends { app, arch } => {
            let arch = platform::select(
                arch.as_deref(),
                config.architecture(),
                &platform::host()?,
                false,
            )?;
            println!("Source\tName");
            for package in crate::lifecycle::plan(&resolver, &app, &arch, false)? {
                println!("{}\t{}", package.resolved.source, package.resolved.name);
            }
            Ok(())
        }
        Command::Uninstall { apps } => crate::lifecycle::uninstall(&config, &apps),
        Command::List { query } => {
            let filter = RegexBuilder::new(query.as_deref().unwrap_or(""))
                .case_insensitive(true)
                .build()?;
            let installed = crate::state::installed(&config)?;
            anyhow::ensure!(!installed.is_empty(), "There aren't any apps installed.");
            println!("Name\tVersion\tSource\tUpdated\tInfo");
            for (app, dir, record) in installed {
                if filter.is_match(&app) {
                    println!(
                        "{app}\t{}\t{}\t{}\t{}",
                        dir.file_name().unwrap_or_default().to_string_lossy(),
                        record
                            .bucket
                            .as_deref()
                            .or(record.url.as_deref())
                            .unwrap_or(""),
                        chrono::DateTime::<chrono::Utc>::from_timestamp(
                            i64::try_from(record.installed_at).unwrap_or(i64::MAX),
                            0
                        )
                        .map(|date| date.to_rfc3339())
                        .unwrap_or_else(|| "<invalid timestamp>".into()),
                        if record.hold { "Held package" } else { "" }
                    );
                }
            }
            Ok(())
        }
        Command::Prefix { app } => {
            let name = crate::state::operand_name(&app)?;
            let (dir, _) = crate::state::current(&config, name)?
                .ok_or_else(|| anyhow::anyhow!("package is not installed: {app}"))?;
            println!("{}", dir.parent().unwrap().join("current").display());
            Ok(())
        }
        Command::Which { command } => {
            crate::links::validate_basename(&command)?;
            anyhow::ensure!(
                fs::symlink_metadata(config.root.join("bin"))?
                    .file_type()
                    .is_dir(),
                "bin is not a managed directory"
            );
            let link = config.root.join("bin").join(&command);
            let app = crate::links::owner(&link)?
                .ok_or_else(|| anyhow::anyhow!("managed executable not found: {command}"))?;
            anyhow::ensure!(
                crate::state::current(&config, &app)?.is_some(),
                "executable package is not installed"
            );
            println!("{}", fs::canonicalize(link)?.display());
            Ok(())
        }
        Command::Update(options) => crate::maintenance::update(&config, &options),
        Command::Status { local } => crate::maintenance::status(&config, local),
        Command::Hold { apps } => crate::maintenance::hold(&config, &apps, true),
        Command::Unhold { apps } => crate::maintenance::hold(&config, &apps, false),
        Command::Reset { apps, all } => crate::maintenance::reset(&config, &apps, all),
        Command::Cleanup { apps, all, cache } => {
            crate::maintenance::cleanup(&config, &apps, all, cache)
        }
        Command::Cache { command } => crate::maintenance::cache(&config, command.as_ref()),
        Command::Export {
            config: include_config,
        } => {
            use std::io::Write;
            let exported = crate::scoopfile::export(&config, include_config)?;
            let json = serde_json::to_string_pretty(&exported.document)?;
            if let Err(error) = writeln!(std::io::stdout().lock(), "{json}") {
                if error.kind() == std::io::ErrorKind::BrokenPipe {
                    return Ok(());
                }
                return Err(error.into());
            }
            anyhow::ensure!(
                exported.issues.is_empty(),
                "export incomplete; omitted entries: {}",
                exported.issues.join("; ")
            );
            Ok(())
        }
        Command::Import { file } => crate::scoopfile::import(&config, &file),
        Command::Checkup => crate::diagnostics::checkup(&config),
        Command::Home { app } => crate::diagnostics::home(&config, &app),
        Command::Alias { .. } => unsupported("alias"),
        Command::Create { .. } => unsupported("create"),
        Command::Virustotal { .. } => unsupported("virustotal"),
        Command::Help { .. } => unreachable!(),
    }
}
fn license(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(identifier)) => identifier.clone(),
        Some(value) => {
            let identifier = value
                .get("identifier")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match value.get("url").and_then(|v| v.as_str()) {
                Some(url) => format!("{identifier} ({url})"),
                None => identifier.into(),
            }
        }
        None => String::new(),
    }
}
fn unsupported(command: &str) -> Result<()> {
    bail!("unsupported feature: {command} (deferred beyond MVP)")
}
pub fn search(buckets: &Buckets, query: &str) -> Result<()> {
    let query = RegexBuilder::new(query).case_insensitive(true).build()?;
    println!("Name\tVersion\tSource\tBinaries");
    for bucket in buckets.names()? {
        for path in buckets.manifests(&bucket)? {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            let manifest = match fs::read_to_string(&path)
                .map_err(Into::into)
                .and_then(Manifest::parse)
            {
                Ok(manifest) => manifest,
                Err(error) => {
                    eprintln!("warning: {}: {error}", path.display());
                    continue;
                }
            };
            let mut bins = crate::manifest::binaries(manifest.value.get("bin"))?;
            if let Some(arches) = manifest
                .value
                .get("architecture")
                .and_then(|v| v.as_object())
            {
                for entry in arches.values() {
                    bins.extend(crate::manifest::binaries(entry.get("bin"))?);
                }
            }
            let mut basenames: Vec<_> = bins.iter().map(|bin| bin.name.as_str()).collect();
            basenames.sort();
            basenames.dedup();
            if query.is_match(name) || basenames.iter().any(|b| query.is_match(b)) {
                println!(
                    "{name}\t{}\t{bucket}\t{}",
                    manifest.version(),
                    basenames.join(", ")
                );
            }
        }
    }
    Ok(())
}
