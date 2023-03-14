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
use hgstore::separate_metadata;
use hgstore::strip_metadata;
use storemodel::types;
use storemodel::ReadFileContents;
use storemodel::RefreshableReadFileContents;
use storemodel::RefreshableTreeStore;
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
                Some(data) => separate_metadata(&data)?.0,
                None => anyhow::bail!("no such file: {:?}", &k),
            };
            Ok((data, k))
        });
        futures::stream::iter(iter).boxed()
    }

    fn read_rename_metadata(&self, keys: Vec<Key>) -> Result<Vec<(Key, Option<Key>)>, Self::Error> {
        keys.into_iter()
            .map(|k| {
                let id = k.hgid;
                let copy_from = match self.get_content(id)? {
                    Some(data) => strip_metadata(&data)?.1,
                    None => anyhow::bail!("no such file: {:?}", &k),
                };
                Ok((k, copy_from))
            })
            .collect()
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

impl RefreshableReadFileContents for EagerRepoStore {
    fn refresh(&self) -> Result<(), Self::Error> {
        let mut inner = self.inner.write();
        inner.flush()?;
        Ok(())
    }
}

impl RefreshableTreeStore for EagerRepoStore {
    fn refresh(&self) -> anyhow::Result<()> {
        let mut inner = self.inner.write();
        inner.flush()?;
        Ok(())
    }
}
