/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use chrono::DateTime;
use chrono::Utc;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use mononoke_types::RepositoryId;
use permission_checker::MononokeIdentitySet;
use scribe_ext::Scribe;
use serde_derive::Serialize;
use std::num::NonZeroU64;

#[derive(Serialize)]
pub struct CommitInfo<'a> {
    repo_id: RepositoryId,
    repo_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    bookmark: Option<&'a str>,
    generation: Generation,
    changeset_id: ChangesetId,
    #[serde(skip_serializing_if = "Option::is_none")]
    bubble_id: Option<NonZeroU64>,
    parents: Vec<ChangesetId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_unix_name: Option<&'a str>,
    #[serde(skip_serializing_if = "MononokeIdentitySet::is_empty")]
    user_identities: &'a MononokeIdentitySet,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_hostname: Option<&'a str>,
    #[serde(with = "::chrono::serde::ts_seconds")]
    received_timestamp: DateTime<Utc>,
    #[serde(flatten)]
    changed_files_info: ChangedFilesInfo,
}

#[derive(Serialize)]
pub struct ChangedFilesInfo {
    changed_files_count: u64,
    changed_files_size: u64,
}

impl ChangedFilesInfo {
    pub fn new(bcs: &BonsaiChangeset) -> Self {
        let changed_files_count: u64 = bcs.file_changes_map().len().try_into().unwrap();

        let mut changed_files_size = 0;
        for fc in bcs.file_changes_map().values() {
            changed_files_size += fc.size().unwrap_or(0);
        }

        Self {
            changed_files_count,
            changed_files_size,
        }
    }
}

impl<'a> CommitInfo<'a> {
    pub fn new(
        repo_id: RepositoryId,
        repo_name: &'a str,
        bookmark: Option<&'a str>,
        generation: Generation,
        changeset_id: ChangesetId,
        bubble_id: Option<NonZeroU64>,
        parents: Vec<ChangesetId>,
        user_unix_name: Option<&'a str>,
        user_identities: &'a MononokeIdentitySet,
        source_hostname: Option<&'a str>,
        received_timestamp: DateTime<Utc>,
        changed_files_info: ChangedFilesInfo,
    ) -> Self {
        Self {
            repo_id,
            repo_name,
            bookmark,
            generation,
            changeset_id,
            bubble_id,
            parents,
            user_unix_name,
            user_identities,
            source_hostname,
            received_timestamp,
            changed_files_info,
        }
    }
}

pub struct LogToScribe {
    client: Option<Scribe>,
    category: String,
}

impl LogToScribe {
    pub fn new(client: Scribe, category: String) -> Self {
        Self {
            client: Some(client),
            category,
        }
    }

    pub fn new_with_discard() -> Self {
        Self {
            client: None,
            category: String::new(),
        }
    }

    pub fn queue_commit(&self, commit: &CommitInfo<'_>) -> Result<(), Error> {
        match &self.client {
            Some(ref client) => {
                let commit = serde_json::to_string(commit)?;
                client.offer(&self.category, &commit)
            }
            None => Ok(()),
        }
    }
}
