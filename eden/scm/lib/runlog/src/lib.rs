/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod filestore;

#[cfg(unix)]
use std::os::unix::prelude::MetadataExt;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono;
use clidispatch::repo::Repo;
pub use filestore::FileStore;
use parking_lot::Mutex;
use rand::distributions::Alphanumeric;
use rand::thread_rng;
use rand::Rng;
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
    pub fn new(repo: Option<&Repo>, command: Vec<String>) -> Result<Arc<Self>> {
        let entry = Entry::new(command);
        let mut storage: Option<Mutex<FileStore>> = None;

        if let Some(repo) = repo {
            if repo.config().get_or("runlog", "enable", || false)?
                && !accidentally_running_as_root(repo)
            {
                let dir = repo.shared_dot_hg_path().join("runlog");

                // Probabilistically clean up old entries to avoid doing the work every time.
                let cleanup_chance = repo.config().get_or("runlog", "cleanup_chance", || 0.01)?;
                if cleanup_chance > rand::thread_rng().gen::<f64>() {
                    let threshold = repo
                        .config()
                        .get_or("runlog", "cleanup_threshold", || 3600.0)?;
                    FileStore::cleanup(&dir, Duration::from_secs_f64(threshold))?;
                }

                storage = Some(Mutex::new(FileStore::new(dir, &entry.id)?))
            }
        }

        let logger = Self {
            entry: Mutex::new(entry),
            storage,
        };
        logger.write(&logger.entry.lock())?;

        return Ok(Arc::new(logger));
    }

    pub fn close(&self, exit_code: i32) -> Result<()> {
        let mut entry = self.entry.lock();
        entry.exit_code = Some(exit_code);
        entry.end_time = Some(chrono::Utc::now());
        entry.progress = Vec::new();

        self.write(&entry)?;

        Ok(())
    }

    pub fn update_progress(&self, progress: Vec<Progress>) -> Result<()> {
        let mut entry = self.entry.lock();
        if entry.exit_code.is_none() && entry.set_progress(progress) {
            self.write(&entry)?;
        }

        Ok(())
    }

    fn write(&self, e: &Entry) -> Result<()> {
        if let Some(storage) = &self.storage {
            let storage = storage.lock();
            storage.save(e)?;
        }

        Ok(())
    }
}

#[cfg(unix)]
fn accidentally_running_as_root(repo: &Repo) -> bool {
    // Check if we are root and repo is not owned by root.

    if unsafe { libc::geteuid() } != 0 {
        return false;
    }

    match std::fs::metadata(repo.shared_dot_hg_path()) {
        Ok(m) => m.uid() != 0,
        // err on side of not writing files as root
        Err(_) => true,
    }
}

#[cfg(not(unix))]
fn accidentally_running_as_root(repo: &Repo) -> bool {
    false
}

/// Entry represents one runlog entry (i.e. a single hg command
/// execution).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Entry {
    id: String,
    command: Vec<String>,
    pid: u64,
    start_time: chrono::DateTime<chrono::Utc>,
    end_time: Option<chrono::DateTime<chrono::Utc>>,
    exit_code: Option<i32>,
    progress: Vec<Progress>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Progress {
    topic: String,
    unit: String,
    total: u64,
    position: u64,
}

impl Entry {
    fn new(command: Vec<String>) -> Self {
        Self {
            id: thread_rng()
                .sample_iter(Alphanumeric)
                .take(16)
                .map(char::from)
                .collect(),
            command,
            pid: unsafe { libc::getpid() } as u64,
            start_time: chrono::Utc::now(),
            end_time: None,
            exit_code: None,
            progress: Vec::new(),
        }
    }

    /// Return whether passed progress differs from current progress.
    pub fn set_progress(&mut self, progress: Vec<Progress>) -> bool {
        if self.progress == progress {
            false
        } else {
            self.progress = progress;
            true
        }
    }
}

impl Progress {
    pub fn new(bar: Arc<progress_model::ProgressBar>) -> Progress {
        let (position, total) = bar.position_total();
        return Progress {
            topic: bar.topic().to_string(),
            position,
            total,
            unit: bar.unit().to_string(),
        };
    }
}
