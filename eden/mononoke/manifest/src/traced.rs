/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::hash::Hash;
use std::hash::Hasher;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use context::CoreContext;
use futures::stream::BoxStream;
use futures::stream::TryStreamExt;
use mononoke_types::MPathElement;
use mononoke_types::SortedVectorTrieMap;

use crate::types::Entry;
use crate::types::Manifest;

/// Traced allows you to trace a given parent through manifest derivation. For example, if you
/// assign ID 1 to a tree, then perform manifest derivation, then further entries you presented to
/// you that came from this parent will have the same ID.
#[derive(Debug)]
pub struct Traced<I, E>(Option<I>, E);

impl<I, E: Hash> Hash for Traced<I, E> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.1.hash(state);
    }
}

impl<I, E: PartialEq> PartialEq for Traced<I, E> {
    fn eq(&self, other: &Self) -> bool {
        self.1 == other.1
    }
}

impl<I, E: Eq> Eq for Traced<I, E> {}

impl<I: Copy, E: Copy> Copy for Traced<I, E> {}

impl<I: Clone, E: Clone> Clone for Traced<I, E> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone())
    }
}

impl<I, E> Traced<I, E> {
    pub fn generate(e: E) -> Self {
        Self(None, e)
    }

    pub fn assign(i: I, e: E) -> Self {
        Self(Some(i), e)
    }

    pub fn id(&self) -> Option<&I> {
        self.0.as_ref()
    }

    pub fn untraced(&self) -> &E {
        &self.1
    }

    pub fn into_untraced(self) -> E {
        self.1
    }
}

impl<I: Copy, E> Traced<I, E> {
    fn inherit_into_entry<TreeId, Leaf>(
        &self,
        e: Entry<TreeId, Leaf>,
    ) -> Entry<Traced<I, TreeId>, Traced<I, Leaf>> {
        match e {
            Entry::Tree(t) => Entry::Tree(Traced(self.0, t)),
            Entry::Leaf(l) => Entry::Leaf(Traced(self.0, l)),
        }
    }
}

impl<I, TreeId, Leaf> From<Entry<Traced<I, TreeId>, Traced<I, Leaf>>> for Entry<TreeId, Leaf> {
    fn from(entry: Entry<Traced<I, TreeId>, Traced<I, Leaf>>) -> Self {
        match entry {
            Entry::Tree(Traced(_, t)) => Entry::Tree(t),
            Entry::Leaf(Traced(_, l)) => Entry::Leaf(l),
        }
    }
}

#[async_trait]
impl<Store, I, M> Manifest<Store> for Traced<I, M>
where
    Store: Send + Sync,
    I: Send + Sync + Copy + 'static,
    M: Manifest<Store> + Send + Sync,
{
    type TreeId = Traced<I, <M as Manifest<Store>>::TreeId>;
    type Leaf = Traced<I, <M as Manifest<Store>>::Leaf>;
    type TrieMapType = SortedVectorTrieMap<Entry<Self::TreeId, Self::Leaf>>;

    async fn list(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        let stream = self
            .1
            .list(ctx, blobstore)
            .await?
            .map_ok(|(path, entry)| (path, self.inherit_into_entry(entry)));
        Ok(Box::pin(stream))
    }

    async fn list_prefix(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        let stream = self
            .1
            .list_prefix(ctx, blobstore, prefix)
            .await?
            .map_ok(|(path, entry)| (path, self.inherit_into_entry(entry)));
        Ok(Box::pin(stream))
    }

    async fn list_prefix_after(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
        after: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        let stream = self
            .1
            .list_prefix_after(ctx, blobstore, prefix, after)
            .await?
            .map_ok(|(path, entry)| (path, self.inherit_into_entry(entry)));
        Ok(Box::pin(stream))
    }

    async fn list_skip(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        skip: usize,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        let stream = self
            .1
            .list_skip(ctx, blobstore, skip)
            .await?
            .map_ok(|(path, entry)| (path, self.inherit_into_entry(entry)));
        Ok(Box::pin(stream))
    }

    async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::Leaf>>> {
        let entry = self.1.lookup(ctx, blobstore, name).await?;
        Ok(entry.map(|e| self.inherit_into_entry(e)))
    }

    async fn into_trie_map(
        self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        let entries = self
            .1
            .list(ctx, blobstore)
            .await?
            .map_ok(|(k, v)| (k.to_smallvec(), self.inherit_into_entry(v)))
            .try_collect()
            .await?;
        Ok(SortedVectorTrieMap::new(entries))
    }
}

#[async_trait]
impl<I: Clone + 'static + Send + Sync, M: Loadable + Send + Sync> Loadable for Traced<I, M> {
    type Value = Traced<I, <M as Loadable>::Value>;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        let id = self.0.clone();
        let v = self.1.load(ctx, blobstore).await?;
        Ok(Traced(id, v))
    }
}
