/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Implement traits from other crates.

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use storemodel::ReadFileContents;
use storemodel::TreeFormat;
use storemodel::TreeStore;
use types::HgId;
use types::Key;
use types::RepoPath;

use crate::GitStore;

#[async_trait]
impl ReadFileContents for GitStore {
    type Error = anyhow::Error;

    async fn read_file_contents(
        &self,
        keys: Vec<Key>,
    ) -> BoxStream<Result<(minibytes::Bytes, Key), Self::Error>> {
        let iter = keys.into_iter().map(|k| {
            let id = k.hgid;
            let data = self.read_obj(id, git2::ObjectType::Blob)?;
            Ok((data.into(), k))
        });
        futures::stream::iter(iter).boxed()
    }
}

impl TreeStore for GitStore {
    fn get(&self, _path: &RepoPath, hgid: HgId) -> anyhow::Result<minibytes::Bytes> {
        let data = self.read_obj(hgid, git2::ObjectType::Tree)?;
        Ok(data.into())
    }

    fn insert(&self, _path: &RepoPath, hgid: HgId, data: minibytes::Bytes) -> anyhow::Result<()> {
        let id = self.write_obj(git2::ObjectType::Tree, data.as_ref())?;
        if id != hgid {
            anyhow::bail!("tree id mismatch: {} (written) != {} (expected)", id, hgid);
        }
        Ok(())
    }

    fn format(&self) -> TreeFormat {
        TreeFormat::Git
    }
}
