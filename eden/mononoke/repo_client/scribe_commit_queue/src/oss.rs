/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use fbinit::FacebookInit;

use crate::{CommitInfo, ScribeCommitQueue};

pub struct LogToScribe {}

impl LogToScribe {
    pub fn new_with_default_scribe(_fb: FacebookInit, _category: String) -> Self {
        Self {}
    }

    pub fn new_with_discard() -> Self {
        Self {}
    }
}

#[async_trait]
impl ScribeCommitQueue for LogToScribe {
    async fn queue_commit(&self, _commit: &CommitInfo<'_>) -> Result<(), Error> {
        Ok(())
    }
}
