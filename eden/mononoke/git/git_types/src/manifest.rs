/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::manifest::Entry;
use ::manifest::Manifest;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use mononoke_types::MPathElement;
use mononoke_types::SortedVectorTrieMap;

use crate::BlobHandle;
use crate::Tree;
use crate::TreeHandle;
use crate::Treeish;

#[async_trait]
impl<Store: Send + Sync> Manifest<Store> for Tree {
    type TreeId = TreeHandle;
    type Leaf = BlobHandle;
    type TrieMapType = SortedVectorTrieMap<Entry<TreeHandle, BlobHandle>>;

    async fn lookup(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::Leaf>>> {
        Ok(self.members().get(name).map(|e| e.clone().into()))
    }

    async fn list(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        let members: Vec<_> = self
            .members()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone().into()))
            .collect();
        Ok(stream::iter(members).map(Ok).boxed())
    }

    async fn into_trie_map(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        let members = self
            .members()
            .iter()
            .map(|(k, v)| (k.clone().to_smallvec(), v.clone().into()))
            .collect();
        Ok(SortedVectorTrieMap::new(members))
    }
}
