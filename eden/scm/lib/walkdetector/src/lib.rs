/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(test)]
mod tests;

use std::time::Instant;

use anyhow::Result;
use types::RepoPathBuf;

// Goals:
//  - Aggressively detect walk and aggressively cancel walk.
//  - Passive - don't fetch or query any stores.
//  - Minimize memory usage.

pub struct Detector {}

impl Detector {
    pub fn new() -> Self {
        Self {}
    }

    pub fn walks(&self) -> Vec<(RepoPathBuf, usize)> {
        Vec::new()
    }

    pub fn file_read(&self, time: Instant, path: RepoPathBuf) -> Result<()> {
        tracing::trace!(?time, %path, "file_read");

        // do something

        Ok(())
    }
}
