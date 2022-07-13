/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Result;
use thrift_types::edenfs::ScmFileStatus;
use types::RepoPathBuf;

use crate::filesystem::ChangeType;
use crate::filesystem::PendingChangeResult;

pub struct EdenFileSystem {
    root: PathBuf,
}

impl EdenFileSystem {
    pub fn new(root: PathBuf) -> Result<Self> {
        Ok(EdenFileSystem { root })
    }

    pub fn pending_changes(&self) -> Result<Box<dyn Iterator<Item = Result<PendingChangeResult>>>> {
        let result = edenfs_client::status::get_status(&self.root)?;
        Ok(Box::new(result.status.entries.into_iter().filter_map(
            |(path, status)| {
                {
                    // TODO: Handle non-UTF8 encoded paths from Eden
                    let repo_path = match RepoPathBuf::from_utf8(path) {
                        Ok(repo_path) => repo_path,
                        Err(err) => return Some(Err(anyhow!(err))),
                    };
                    match status {
                        ScmFileStatus::REMOVED => Some(Ok(PendingChangeResult::File(
                            ChangeType::Deleted(repo_path),
                        ))),
                        ScmFileStatus::IGNORED => None,
                        _ => Some(Ok(PendingChangeResult::File(ChangeType::Changed(
                            repo_path,
                        )))),
                    }
                }
            },
        )))
    }
}
