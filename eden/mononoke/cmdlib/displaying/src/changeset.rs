/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Display;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use serde::Serialize;

#[derive(Serialize)]
pub struct DisplayChangeset {
    pub changeset_id: ChangesetId,
    pub parents: Vec<ChangesetId>,
    pub author: String,
    pub author_date: DateTime,
    pub committer: Option<String>,
    pub committer_date: Option<DateTime>,
    pub message: String,
    pub extra: BTreeMap<String, Vec<u8>>,
    pub file_changes: BTreeMap<String, FileChange>,
}

impl TryFrom<&BonsaiChangeset> for DisplayChangeset {
    type Error = Error;

    fn try_from(bonsai: &BonsaiChangeset) -> Result<Self> {
        Ok(DisplayChangeset {
            changeset_id: bonsai.get_changeset_id(),
            parents: bonsai.parents().collect(),
            author: bonsai.author().to_string(),
            author_date: bonsai.author_date().clone(),
            committer: bonsai.committer().map(ToString::to_string),
            committer_date: bonsai.committer_date().cloned(),
            message: bonsai.message().to_string(),
            extra: bonsai
                .extra()
                .map(|(k, v)| (k.to_string(), v.to_vec()))
                .collect(),
            file_changes: bonsai
                .file_changes_map()
                .iter()
                .map(|(k, v)| {
                    Ok((
                        String::from_utf8(k.to_vec())
                            .with_context(|| format!("Invalid extra name: {:?}", k))?,
                        v.clone(),
                    ))
                })
                .collect::<Result<_>>()?,
        })
    }
}

impl Display for DisplayChangeset {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        writeln!(fmt, "BonsaiChangesetId: {}", self.changeset_id)?;
        writeln!(fmt, "Author: {}", self.author)?;
        writeln!(fmt, "Message: {}", self.message)?;
        writeln!(fmt, "FileChanges:")?;
        for (path, change) in self.file_changes.iter() {
            match change {
                FileChange::Change(change) => match change.copy_from() {
                    Some(_) => writeln!(fmt, "\t COPY/MOVE: {} {}", path, change.content_id())?,
                    None => writeln!(fmt, "\t ADDED/MODIFIED: {} {}", path, change.content_id())?,
                },
                FileChange::Deletion => writeln!(fmt, "\t REMOVED: {}", path)?,
                FileChange::UntrackedChange(change) => writeln!(
                    fmt,
                    "\t UNTRACKED ADD/MODIFY: {} {}",
                    path,
                    change.content_id()
                )?,
                FileChange::UntrackedDeletion => writeln!(fmt, "\t MISSING: {}", path)?,
            }
        }
        Ok(())
    }
}
