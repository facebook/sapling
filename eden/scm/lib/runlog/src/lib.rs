/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod filestore;

pub use filestore::FileStore;

use anyhow::Result;
use chrono;
use parking_lot::Mutex;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};

use clidispatch::repo::Repo;

use std::sync::Arc;

/// Logger logs runtime information for a single hg command invocation.
pub struct Logger {
    entry: Mutex<Entry>,
    storage: Option<Mutex<FileStore>>,
}

impl Logger {
    /// Initialize a new logger and write out initial runlog entry.
    /// Respects runlog.enable config field.
    pub fn new(repo: Option<&Repo>, command: Vec<String>) -> Result<Arc<Self>> {
        let mut logger = Self {
            entry: Mutex::new(Entry::new(command)),
            storage: None,
        };

        if let Some(repo) = repo {
            if repo.config().get_or("runlog", "enable", || false)? {
                logger.storage = Some(Mutex::new(FileStore::new(
                    repo.shared_dot_hg_path().join("runlog"),
                )?))
            }
        }

        logger.write(&logger.entry.lock())?;

        return Ok(Arc::new(logger));
    }

    pub fn close(&self, exit_code: i32) -> Result<()> {
        let mut entry = self.entry.lock();
        entry.exit_code = Some(exit_code);
        entry.end_time = Some(chrono::Utc::now());

        self.write(&entry)?;

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

/// Entry represents one runlog entry (i.e. a single hg command
/// execution).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct Entry {
    id: String,
    command: Vec<String>,
    pid: u64,
    start_time: chrono::DateTime<chrono::Utc>,
    end_time: Option<chrono::DateTime<chrono::Utc>>,
    exit_code: Option<i32>,
}

impl Entry {
    fn new(command: Vec<String>) -> Self {
        Self {
            id: thread_rng().sample_iter(Alphanumeric).take(16).collect(),
            command,
            pid: unsafe { libc::getpid() } as u64,
            start_time: chrono::Utc::now(),
            end_time: None,
            exit_code: None,
        }
    }
}
