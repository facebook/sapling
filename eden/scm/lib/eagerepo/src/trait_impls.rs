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
use storemodel::types;
use storemodel::ReadFileContents;
use storemodel::TreeFormat;
use storemodel::TreeStore;
use types::HgId;
use types::Key;
use types::RepoPath;

use crate::EagerRepoStore;

// storemodel traits

#[async_trait]
impl ReadFileContents for EagerRepoStore {
    type Error = anyhow::Error;

    async fn read_file_contents(
        &self,
        keys: Vec<Key>,
    ) -> BoxStream<Result<(minibytes::Bytes, Key), Self::Error>> {
        let iter = keys.into_iter().map(|k| {
            let id = k.hgid;
            let data = match self.get_content(id)? {
                Some(data) => data,
                None => anyhow::bail!("no such file: {:?}", &k),
            };
            Ok((data, k))
        });
        futures::stream::iter(iter).boxed()
    }
}

impl TreeStore for EagerRepoStore {
    fn get(&self, path: &RepoPath, hgid: HgId) -> anyhow::Result<minibytes::Bytes> {
        let data = match self.get_content(hgid)? {
            Some(data) => data,
            None => anyhow::bail!("no such tree: {:?} {:?}", path, hgid),
        };
        Ok(data)
    }

    fn insert(&self, _path: &RepoPath, _hgid: HgId, _data: minibytes::Bytes) -> anyhow::Result<()> {
        anyhow::bail!("insert cannot be used for Hg trees");
    }

    fn format(&self) -> TreeFormat {
        TreeFormat::Hg
    }
}
