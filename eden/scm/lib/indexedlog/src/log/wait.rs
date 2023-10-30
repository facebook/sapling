/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use super::meta::LogMetadata;
use super::Log;
use super::META_FILE;
use crate::errors::IoResultExt;

/// State to detect on-disk updates.
pub struct Wait {
    // The "meta" file to watch.
    path: PathBuf,
    // The "len" and "epoch" specified by the meta file.
    state: (u64, u64),
}

impl Wait {
    /// Construct `Wait` from a `Log`.
    pub fn from_log(log: &Log) -> crate::Result<Self> {
        let path = match log.dir.as_opt_path() {
            None => {
                return Err(crate::errors::Error::programming(
                    "Wait does not support in-memory Log",
                ));
            }
            Some(dir) => dir.join(META_FILE),
        };
        Ok(Self {
            path,
            state: state_from_meta(&log.meta),
        })
    }

    /// Wait for on-disk changes that changes the backing `Log`.
    pub fn wait_for_change(&mut self) -> crate::Result<()> {
        // Initialize atomicfile Wait before read_meta() to avoid races.
        let mut atomic_wait = atomicfile::Wait::from_path(&self.path)
            .context(&self.path, "initialize file change detector")?;
        tracing::debug!(" waiting meta change: {}", self.path.display());
        let mut new_state;
        loop {
            let new_meta = LogMetadata::read_file(&self.path)?;
            new_state = state_from_meta(&new_meta);
            if new_state != self.state {
                tracing::trace!(" state changed: {}", self.path.display());
                break;
            } else {
                tracing::trace!(" state not changed: {}", self.path.display());
                // Block.
                atomic_wait
                    .wait_for_change()
                    .context(&self.path, "waiting file change")?;
            }
        }
        self.state = new_state;
        Ok(())
    }
}

fn state_from_meta(meta: &LogMetadata) -> (u64, u64) {
    (meta.primary_len, meta.epoch)
}
