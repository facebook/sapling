/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use slog::info;
use slog::Logger;
use std::ffi::OsStr;
use std::fs::read_to_string;
use std::io::Write;
use std::path::PathBuf;
use tempfile::NamedTempFile;

#[derive(Clone, Debug)]
pub struct FileCheckpoint {
    pub file_name: PathBuf,
}

impl FileCheckpoint {
    pub fn new(file_name: &OsStr) -> Self {
        let mut buf = PathBuf::new();
        buf.push(file_name);
        Self { file_name: buf }
    }

    pub fn read(&self) -> Result<Option<String>, Error> {
        if self.file_name.exists() {
            return read_to_string(&self.file_name)
                .map(Some)
                .context("couldn't read checkpoint");
        }
        Ok(None)
    }

    pub fn update(&self, logger: &Logger, key: &str) -> Result<(), Error> {
        let tempfile = NamedTempFile::new_in(
            &self
                .file_name
                .parent()
                .context("no parent dir for checkpoint file")?,
        )?;
        tempfile.as_file().write_all(key.as_bytes())?;
        let file = tempfile.persist(&self.file_name)?;
        // This is expensive, but we only call it every PROGRESS_INTERVAL_SECS seconds
        file.sync_all()?;
        info!(logger, "checkpointed {}", key);
        Ok(())
    }
}
