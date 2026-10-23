use crate::{
    config::{self, Config},
    links, manifest, state,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Link {
    name: String,
    old: Option<PathBuf>,
    new: Option<PathBuf>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transaction {
    format_version: u32,
    app: String,
    version: String,
    old_current: Option<PathBuf>,
    stage: Option<String>,
    backup: Option<String>,
    links: Vec<Link>,
    committed: bool,
}
pub fn sync_dir(path: &Path) -> Result<()> {
    fs::File::open(path)?.sync_all()?;
    Ok(())
}
pub fn sync_tree(path: &Path) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            sync_tree(&entry.path())?;
        } else if kind.is_file() {
            fs::File::open(entry.path())?.sync_all()?;
        }
    }
    sync_dir(path)
}
pub fn failpoint(name: &str) -> Result<()> {
    #[cfg(feature = "test-hooks")]
    if std::env::var("SCOOP_TEST_FAILPOINT").ok().as_deref() == Some(name) {
        if std::env::var("SCOOP_TEST_CRASH").ok().as_deref() == Some("1") {
            std::process::exit(86);
        }
        anyhow::bail!("injected failure: {name}");
    }
    let _ = name;
    Ok(())
}
fn journal_path(config: &Config, app: &str) -> PathBuf {
    config
        .root
        .join(".transactions")
        .join(format!("{app}.json"))
}
fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}
fn remove_tree(path: &Path) -> Result<()> {
    if exists(path) {
        ensure!(
            fs::symlink_metadata(path)?.file_type().is_dir(),
            "transaction directory was replaced: {}",
            path.display()
        );
        state::Record::load(path).context("refusing to remove an unrecognized transaction tree")?;
        fs::remove_dir_all(path)?;
    }
    Ok(())
}
impl Transaction {
    fn validate(&self) -> Result<()> {
        ensure!(self.format_version == 1, "unsupported transaction format");
        manifest::name(&self.app)?;
        state::version_name(&self.version)?;
        if let Some(old) = &self.old_current {
            state::version_name(old.to_str().context("invalid transaction current")?)?;
        }
        for name in self.stage.iter().chain(self.backup.iter()) {
            manifest::name(name)?;
        }
        if let Some(stage) = &self.stage {
            ensure!(
                stage.starts_with(&format!("{}-", self.app)),
                "invalid staging ownership"
            );
        }
        if let Some(backup) = &self.backup {
            ensure!(
                backup.starts_with(&format!("backup-{}-", self.app)),
                "invalid backup ownership"
            );
        }
        for link in &self.links {
            links::validate_basename(&link.name)?;
            for target in link.old.iter().chain(link.new.iter()) {
                let parent = Path::new("../apps");
                ensure!(
                    target.starts_with(parent),
                    "invalid transaction executable target"
                );
                let tail = target
                    .strip_prefix(parent)?
                    .to_str()
                    .context("invalid link target")?;
                manifest::relative(tail)?;
            }
        }
        Ok(())
    }
    fn finish(&self, config: &Config) -> Result<()> {
        for name in self.backup.iter().chain(self.stage.iter()) {
            remove_tree(&config.root.join(".staging").join(name))?;
        }
        fs::remove_file(journal_path(config, &self.app))?;
        sync_dir(&config.root.join(".transactions"))?;
        sync_dir(&config.root.join(".staging"))
    }
    fn rollback(&self, config: &Config) -> Result<()> {
        let app = config.root.join("apps").join(&self.app);
        ensure!(
            fs::symlink_metadata(&app)?.file_type().is_dir(),
            "recovery app directory is not managed"
        );
        let final_dir = app.join(&self.version);
        let current = app.join("current");
        if exists(&current) {
            let actual =
                fs::read_link(&current).context("unmanaged current conflict during recovery")?;
            ensure!(
                Some(&actual) == self.old_current.as_ref() || actual == Path::new(&self.version),
                "current changed outside the transaction"
            );
        }
        // If a replacement's backup exists, the original final directory was moved.
        if let Some(backup) = &self.backup {
            let backup = config.root.join(".staging").join(backup);
            if exists(&backup) {
                remove_tree(&final_dir)?;
                fs::rename(&backup, &final_dir)?;
            }
        } else if let Some(stage) = &self.stage
            && !exists(&config.root.join(".staging").join(stage))
        {
            remove_tree(&final_dir)?;
        }
        match &self.old_current {
            Some(target) => links::atomic_symlink(target, &app.join("current"))?,
            None => {
                if exists(&app.join("current")) {
                    ensure!(
                        fs::symlink_metadata(app.join("current"))?
                            .file_type()
                            .is_symlink(),
                        "unmanaged current conflict during recovery"
                    );
                    fs::remove_file(app.join("current"))?;
                }
            }
        }
        for link in &self.links {
            let path = config.root.join("bin").join(&link.name);
            let actual = fs::read_link(&path).ok();
            // A user replacement after interruption is not transaction-owned.
            if exists(&path)
                && (!fs::symlink_metadata(&path)?.file_type().is_symlink()
                    || (actual != link.new && actual != link.old))
            {
                anyhow::bail!(
                    "unmanaged executable changed during recovery: {}",
                    path.display()
                );
            }
            match &link.old {
                Some(target) => links::atomic_symlink(target, &path)?,
                None => {
                    if exists(&path) {
                        fs::remove_file(path)?;
                    }
                }
            }
        }
        sync_dir(&app)?;
        sync_dir(&config.root.join("bin"))?;
        self.finish(config)
    }
}
pub fn recover(config: &Config) -> Result<()> {
    let directory = config.root.join(".transactions");
    if !directory.exists() {
        return Ok(());
    }
    ensure!(
        fs::symlink_metadata(&directory)?.file_type().is_dir(),
        "transactions is not a managed directory"
    );
    if fs::read_dir(&directory)?.next().is_none() {
        return Ok(());
    }
    let _lock = config::mutation_lock(&config.root)?;
    recover_locked(config)
}
pub fn recover_locked(config: &Config) -> Result<()> {
    let dir = config.root.join(".transactions");
    if !dir.exists() {
        return Ok(());
    }
    ensure!(
        fs::symlink_metadata(&dir)?.file_type().is_dir(),
        "transactions is not a managed directory"
    );
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let filename = entry.file_name();
        let Some(app) = filename
            .to_str()
            .and_then(|name| name.strip_suffix(".json"))
        else {
            // Atomic metadata writes can leave unpublished .tmp files after a crash.
            continue;
        };
        if manifest::name(app).is_err() {
            continue;
        }
        let result = (|| -> Result<()> {
            ensure!(entry.file_type()?.is_file(), "unmanaged transaction entry");
            for part in ["apps", "bin", ".staging"] {
                ensure!(
                    fs::symlink_metadata(config.root.join(part))?
                        .file_type()
                        .is_dir(),
                    "recovery directory is not managed: {part}"
                );
            }
            let transaction: Transaction = serde_json::from_slice(&fs::read(entry.path())?)?;
            transaction.validate()?;
            ensure!(transaction.app == app, "transaction filename mismatch");
            if transaction.committed {
                transaction.finish(config)?;
            } else {
                transaction.rollback(config)?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => eprintln!("Recovered transaction for {app}."),
            Err(error) => eprintln!(
                "warning: recovery for {app} at {} failed: {error:#}. {}",
                entry.path().display(),
                remedy(app)
            ),
        }
    }
    Ok(())
}
fn remedy(app: &str) -> String {
    format!(
        "Mutations of {app} are blocked. Preserve the journal and package/staging trees; move any reported conflicting user replacement to a safe location, then rerun scoop. For corrupt JSON, restore this journal from backup or inspect its recorded paths and reconcile that app before moving the damaged journal aside."
    )
}
/// Wait for live mutations before inspecting an app from an unlocked planner.
pub fn check(config: &Config, app: &str) -> Result<()> {
    let _lock = config::mutation_lock(&config.root)?;
    recover_locked(config)?;
    ensure_clear(config, app)
}
// Call under the root lock after recovery, before any mutation of this package.
pub fn ensure_clear(config: &Config, app: &str) -> Result<()> {
    manifest::name(app)?;
    let path = journal_path(config, app);
    ensure!(
        !exists(&path),
        "unresolved transaction for {app}: {}. {}",
        path.display(),
        remedy(app)
    );
    Ok(())
}
// Caller holds the root lock. A persisted prepared journal always rolls back; committed journals finish cleanup.
pub fn activate(
    config: &Config,
    app: &str,
    version: &str,
    stage: Option<&Path>,
    replace: bool,
    old_bins: &[manifest::Binary],
    bins: &[manifest::Binary],
) -> Result<()> {
    manifest::name(app)?;
    ensure_clear(config, app)?;
    state::version_name(version)?;
    state::managed_dir(&config.root.join(".transactions"))?;
    let app_dir = config.root.join("apps").join(app);
    state::managed_dir(&app_dir)?;
    links::preflight(config, bins)?;
    let mut names = std::collections::BTreeSet::new();
    for bin in old_bins.iter().chain(bins) {
        names.insert(bin.name.clone());
    }
    let mut changes = Vec::new();
    for name in names {
        let path = config.root.join("bin").join(&name);
        let old = fs::read_link(&path).ok();
        let new = if let Some(bin) = bins.iter().find(|bin| bin.name == name) {
            Some(links::target(app, bin))
        } else if links::owner(&path).ok().flatten().as_deref() == Some(app) {
            None
        } else {
            old.clone()
        };
        if old != new {
            changes.push(Link { name, old, new });
        }
    }
    let stage_name = stage
        .map(|path| {
            path.file_name()
                .context("stage filename")
                .and_then(|name| name.to_str().context("stage UTF-8"))
                .map(str::to_owned)
        })
        .transpose()?;
    if let Some(stage) = stage {
        ensure!(
            stage.parent() == Some(config.root.join(".staging").as_path()),
            "stage must be inside managed staging"
        );
        sync_tree(stage)?;
    }
    let backup = if replace {
        Some(format!(
            "backup-{}-{}",
            app,
            tempfile::NamedTempFile::new_in(config.root.join(".staging"))?
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .trim_start_matches('.')
        ))
    } else {
        None
    };
    let mut transaction = Transaction {
        format_version: 1,
        app: app.into(),
        version: version.into(),
        old_current: fs::read_link(app_dir.join("current")).ok(),
        stage: stage_name,
        backup,
        links: changes,
        committed: false,
    };
    transaction.validate()?;
    config::atomic_json(&journal_path(config, app), &transaction)?;
    let result = (|| -> Result<()> {
        failpoint("prepared")?;
        if let Some(stage) = stage {
            let final_dir = app_dir.join(version);
            if let Some(backup) = &transaction.backup {
                fs::rename(&final_dir, config.root.join(".staging").join(backup))?;
                sync_dir(&app_dir)?;
                sync_dir(&config.root.join(".staging"))?;
                failpoint("backup")?;
            }
            fs::rename(stage, &final_dir)?;
            sync_dir(&app_dir)?;
            sync_dir(&config.root.join(".staging"))?;
            failpoint("published")?;
        }
        links::atomic_symlink(Path::new(version), &app_dir.join("current"))?;
        sync_dir(&app_dir)?;
        failpoint("current")?;
        for change in &transaction.links {
            let path = config.root.join("bin").join(&change.name);
            if let Some(target) = &change.new {
                if change.old != change.new
                    && let Some(owner) = links::owner(&path)?
                    && owner != app
                {
                    println!("Replacing '{}' from {owner} with {app}.", change.name);
                }
                links::atomic_symlink(target, &path)?;
            } else if exists(&path) {
                fs::remove_file(&path)?;
            }
            sync_dir(&config.root.join("bin"))?;
            failpoint("links")?;
        }
        // A stable Scoop executable link may require no individual link changes.
        failpoint("links")?;
        transaction.committed = true;
        config::atomic_json(&journal_path(config, app), &transaction)?;
        failpoint("committed")?;
        transaction.finish(config)
    })();
    if let Err(error) = result {
        if transaction.committed {
            return Err(error);
        }
        transaction
            .rollback(config)
            .context("activation rollback failed; journal retained")?;
        return Err(error);
    }
    Ok(())
}
