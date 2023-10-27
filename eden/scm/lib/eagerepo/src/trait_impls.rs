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
use storemodel::FileStore;
use storemodel::TreeFormat;
use storemodel::TreeStore;
use types::HgId;
use types::Key;
use types::RepoPath;

use crate::EagerRepoStore;

// storemodel traits

#[async_trait]
impl FileStore for EagerRepoStore {
    async fn get_content_stream(
        &self,
        keys: Vec<Key>,
    ) -> BoxStream<anyhow::Result<(minibytes::Bytes, Key)>> {
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

    async fn get_rename_stream(
        &self,
        keys: Vec<Key>,
    ) -> BoxStream<anyhow::Result<(Key, Option<Key>)>> {
        let iter = keys.into_iter().map(|k| {
            let id = k.hgid;
            let copy_from = match self.get_content(id)? {
                Some(data) => strip_metadata(&data)?.1,
                None => anyhow::bail!("no such file: {:?}", &k),
            };
            Ok((k, copy_from))
        });
        futures::stream::iter(iter).boxed()
    }

    fn get_local_content(&self, key: &Key) -> anyhow::Result<Option<minibytes::Bytes>> {
        let id = key.hgid;
        match self.get_content(id)? {
            Some(data) => Ok(Some(separate_metadata(&data)?.0)),
            None => Ok(None),
        }
    }

    fn refresh(&self) -> anyhow::Result<()> {
        let mut inner = self.inner.write();
        inner.flush()?;
        Ok(())
    }

    fn maybe_as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
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

    fn refresh(&self) -> anyhow::Result<()> {
        let mut inner = self.inner.write();
        inner.flush()?;
        Ok(())
    }
}
