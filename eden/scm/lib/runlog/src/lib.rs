/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod filestore;

#[cfg(unix)]
use std::os::unix::prelude::MetadataExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
pub use filestore::FileStore;
use parking_lot::Mutex;
use rand::Rng;
use rand::distributions::Alphanumeric;
use rand::thread_rng;
use repo::repo::Repo;
use serde::Deserialize;
use serde::Serialize;

/// Logger logs runtime information for a single hg command invocation.
pub struct Logger {
    entry: Mutex<Entry>,
    storage: Option<Mutex<FileStore>>,
}

impl Logger {
    /// Initialize a new logger and write out initial runlog entry.
    /// Respects runlog.enable config field.
    pub fn from_repo(repo: Option<&Repo>, command: Vec<String>) -> Result<Arc<Self>> {
        if let Some(repo) = repo {
            Self::new(repo.config(), repo.shared_dot_hg_path(), command)
        } else {
            Ok(Self::empty(command))
        }
    }

    fn empty(command: Vec<String>) -> Arc<Self> {
        Arc::new(Logger {
            entry: Mutex::new(Entry::new(command)),
            storage: None,
        })
    }

    fn new(config: &dyn Config, shared_path: &Path, command: Vec<String>) -> Result<Arc<Self>> {
        if !config.get_or("runlog", "enable", || false)?
            || accidentally_running_as_root(shared_path)
        {
            return Ok(Self::empty(command));
        }

        // Probabilistically clean up old entries to avoid doing the work every time.
        let cleanup_chance = config.get_or("runlog", "cleanup-chance", || 0.01)?;
        if cleanup_chance > rand::thread_rng().r#gen::<f64>() {
            let threshold = config.get_or("runlog", "cleanup-threshold", || 3600.0)?;
            FileStore::cleanup(shared_path, Duration::from_secs_f64(threshold))?;
        }

        let boring_commands: Vec<String> = config.get_or_default("runlog", "boring-commands")?;

        // This command is boring if it is in boring-commands, or it
        // looks like the invoker is disabling the blackbox.
        let boring = command.is_empty()
            || boring_commands.contains(&command[0])
            || (config.get_or_default::<String>("extensions", "blackbox")? == "!")
            || (config.get("blackbox", "track") == Some("".into()));

        let entry = Entry::new(command);
        let storage = Some(Mutex::new(FileStore::new(shared_path, &entry.id, boring)?));

        let logger = Self {
            entry: Mutex::new(entry),
            storage,
        };
        logger.write(&logger.entry.lock(), false)?;

        Ok(Arc::new(logger))
    }

    pub fn close(&self, exit_code: i32) -> Result<()> {
        let mut entry = self.entry.lock();
        entry.exit_code = Some(exit_code);
        entry.end_time = Some(chrono::Utc::now());
        entry.progress = Vec::new();

        self.write(&entry, true)?;

        Ok(())
    }

    pub fn update_progress(&self, progress: Vec<Progress>) -> Result<()> {
        let mut entry = self.entry.lock();
        if entry.exit_code.is_none() && entry.update_status(progress) {
            self.write(&entry, false)?;
        }

        Ok(())
    }

    fn write(&self, e: &Entry, close: bool) -> Result<()> {
        if let Some(storage) = &self.storage {
            let storage = storage.lock();
            storage.save(e)?;

            if close {
                storage.close(e)?;
            }
        }

        Ok(())
    }
}

#[cfg(unix)]
fn accidentally_running_as_root(shared_path: &Path) -> bool {
    // Check if we are root and repo is not owned by root.

    if unsafe { libc::geteuid() } != 0 {
        return false;
    }

    match std::fs::metadata(shared_path) {
        Ok(m) => m.uid() != 0,
        // err on side of not writing files as root
        Err(_) => true,
    }
}

#[cfg(not(unix))]
fn accidentally_running_as_root(_: &Path) -> bool {
    false
}

/// Entry represents one runlog entry (i.e. a single hg command
/// execution).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: String,
    pub command: Vec<String>,
    pub pid: u32,
    pub download_bytes: usize,
    pub upload_bytes: usize,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
    pub exit_code: Option<i32>,
    pub progress: Vec<Progress>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Progress {
    pub topic: String,
    pub unit: String,
    pub total: u64,
    pub position: u64,
}

impl Entry {
    fn new(command: Vec<String>) -> Self {
        let rng_id: String = thread_rng()
            .sample_iter(Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();
        // Note: rng_id can be the same after fork.
        // So we append it with the pid.
        let id = format!("{}{}", rng_id, std::process::id());
        Self {
            id,
            command,
            pid: std::process::id(),
            download_bytes: 0,
            upload_bytes: 0,
            start_time: chrono::Utc::now(),
            end_time: None,
            exit_code: None,
            progress: Vec::new(),
        }
    }

    /// Return whether anything changed in the entry
    pub fn update_status(&mut self, progress: Vec<Progress>) -> bool {
        let (download_bytes, upload_bytes, _) = hg_http::current_progress();
        macro_rules! try_to_update {
            ($original_stat:expr,$new_stat:expr) => {{
                if $original_stat == $new_stat {
                    false
                } else {
                    $original_stat = $new_stat;
                    true
                }
            }};
        }
        let progress_updated = try_to_update!(self.progress, progress);
        let downloaded_bytes_updated = try_to_update!(self.download_bytes, download_bytes);
        let upload_bytes_updated = try_to_update!(self.upload_bytes, upload_bytes);
        progress_updated || downloaded_bytes_updated || upload_bytes_updated
    }
}

impl Progress {
    pub fn new(bar: Arc<progress_model::ProgressBar>) -> Progress {
        let (position, total) = bar.position_total();
        Progress {
            topic: bar.topic().to_string(),
            position,
            total,
            unit: bar.unit().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_close() -> Result<()> {
        let mut cfg = BTreeMap::new();
        cfg.insert("runlog.boring-commands", "boring");
        cfg.insert("runlog.enable", "1");

        let cleaned_up_files = |cfg, name: &str, exit: i32| -> Result<bool> {
            let td = tempdir()?;
            let logger = Logger::new(cfg, td.path(), vec![name.to_string()])?;
            logger.close(exit)?;

            let got: Vec<String> = std::fs::read_dir(td.path().join("runlog"))?
                .map(|d| d.unwrap().path().to_string_lossy().to_string())
                .collect();

            Ok(got.is_empty())
        };

        // Boring commands that exit cleanly are removed immediately.
        assert!(cleaned_up_files(&cfg, "boring", 0)?);

        // Boring commands that exit uncleanly are still recorded.
        assert!(!cleaned_up_files(&cfg, "boring", 1)?);

        // Non-boring commands aren't deleted immediately.
        assert!(!cleaned_up_files(&cfg, "interesting", 0)?);

        // Infer boringness from blackbox disablement.
        let mut no_bb_cfg = cfg.clone();
        no_bb_cfg.insert("extensions.blackbox", "!");
        assert!(cleaned_up_files(&no_bb_cfg, "blackbox_disabled", 0)?);

        // Infer boringness from empty blackbox.trace.
        let mut no_bb_trace = cfg.clone();
        no_bb_trace.insert("blackbox.track", "");
        assert!(cleaned_up_files(&no_bb_trace, "blackbox_disabled", 0)?);

        Ok(())
    }

    #[test]
    fn test_empty_command() -> Result<()> {
        let td = tempdir()?;
        let mut cfg = BTreeMap::new();
        cfg.insert("runlog.enable", "1");

        // Don't crash.
        Logger::new(&cfg, td.path(), vec![])?;

        Ok(())
    }
}
