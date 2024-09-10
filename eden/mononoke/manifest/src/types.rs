/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::hash::Hash;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use blobstore::Storable;
use context::CoreContext;
use either::Either;
use futures::future;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::MPathElement;
use mononoke_types::SortedVectorTrieMap;
use serde_derive::Deserialize;
use serde_derive::Serialize;

pub(crate) use self::bssm::bssm_v3_to_mf_entry;
pub(crate) use self::skeleton_manifests::skeleton_manifest_v2_to_mf_entry;
pub(crate) use self::test_manifests::convert_test_sharded_manifest;

mod bssm;
mod fsnodes;
mod skeleton_manifests;
mod test_manifests;
mod unodes;

#[async_trait]
pub trait AsyncManifest<Store: Send + Sync>: Sized + 'static {
    type TreeId: Send + Sync;
    type LeafId: Send + Sync;
    type TrieMapType: Send + Sync;

    /// Lookup an entry in this manifest.
    async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::LeafId>>>;

    /// List all entries of this manifest.
    async fn list(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>;

    /// List all entries with a given prefix
    async fn list_prefix(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        Ok(self
            .list(ctx, blobstore)
            .await?
            .try_filter(|(k, _)| future::ready(k.starts_with(prefix)))
            .boxed())
    }

    /// List all subentries with a given prefix after a specific key
    async fn list_prefix_after(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
        after: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        Ok(self
            .list(ctx, blobstore)
            .await?
            .try_filter(move |(k, _)| future::ready(k.as_ref() > after && k.starts_with(prefix)))
            .boxed())
    }

    /// List all subentries, skipping the first N
    async fn list_skip(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        skip: usize,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        Ok(self.list(ctx, blobstore).await?.skip(skip).boxed())
    }

    /// Convert this manifest into a trie-map from path element to entry.
    async fn into_trie_map(self, ctx: &CoreContext, blobstore: &Store)
    -> Result<Self::TrieMapType>;
}

pub trait Manifest: Sync + Sized + 'static {
    type TreeId: Send + Sync;
    type LeafId: Send + Sync;
    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>>;
    /// List all subentries with a given prefix
    fn list_prefix<'a>(
        &'a self,
        prefix: &'a [u8],
    ) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)> + 'a> {
        Box::new(self.list().filter(|(k, _)| k.starts_with(prefix)))
    }
    fn list_prefix_after<'a>(
        &'a self,
        prefix: &'a [u8],
        after: &'a [u8],
    ) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)> + 'a> {
        Box::new(
            self.list()
                .filter(move |(k, _)| k.as_ref() > after && k.starts_with(prefix)),
        )
    }
    fn list_skip<'a>(
        &'a self,
        skip: usize,
    ) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)> + 'a> {
        Box::new(self.list().skip(skip))
    }
    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>>;
}

#[async_trait]
impl<M: Manifest + Send, Store: Send + Sync> AsyncManifest<Store> for M {
    type TreeId = <Self as Manifest>::TreeId;
    type LeafId = <Self as Manifest>::LeafId;
    type TrieMapType = SortedVectorTrieMap<Entry<Self::TreeId, Self::LeafId>>;

    async fn list(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        Ok(stream::iter(Manifest::list(self).map(anyhow::Ok).collect::<Vec<_>>()).boxed())
    }

    async fn list_prefix(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
        prefix: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        Ok(stream::iter(
            Manifest::list_prefix(self, prefix)
                .map(anyhow::Ok)
                .collect::<Vec<_>>(),
        )
        .boxed())
    }

    async fn list_prefix_after(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
        prefix: &[u8],
        after: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        Ok(stream::iter(
            Manifest::list_prefix_after(self, prefix, after)
                .map(anyhow::Ok)
                .collect::<Vec<_>>(),
        )
        .boxed())
    }

    async fn list_skip(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
        skip: usize,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        Ok(stream::iter(
            Manifest::list_skip(self, skip)
                .map(anyhow::Ok)
                .collect::<Vec<_>>(),
        )
        .boxed())
    }

    async fn lookup(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::LeafId>>> {
        anyhow::Ok(Manifest::lookup(self, name))
    }

    async fn into_trie_map(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        let entries = Manifest::list(&self)
            .map(|(k, v)| (k.to_smallvec(), v))
            .collect();
        Ok(SortedVectorTrieMap::new(entries))
    }
}

pub type Weight = usize;

pub trait OrderedManifest: Manifest {
    fn lookup_weighted(
        &self,
        name: &MPathElement,
    ) -> Option<Entry<(Weight, <Self as Manifest>::TreeId), <Self as Manifest>::LeafId>>;
    fn list_weighted(
        &self,
    ) -> Box<
        dyn Iterator<
            Item = (
                MPathElement,
                Entry<(Weight, <Self as Manifest>::TreeId), <Self as Manifest>::LeafId>,
            ),
        >,
    >;
}

#[async_trait]
pub trait AsyncOrderedManifest<Store: Send + Sync>: AsyncManifest<Store> {
    async fn list_weighted(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<
        BoxStream<
            'async_trait,
            Result<(MPathElement, Entry<(Weight, Self::TreeId), Self::LeafId>)>,
        >,
    >;
    async fn lookup_weighted(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<(Weight, Self::TreeId), Self::LeafId>>>;
}

#[async_trait]
impl<M: OrderedManifest + Send, Store: Send + Sync> AsyncOrderedManifest<Store> for M {
    async fn list_weighted(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<
        BoxStream<
            'async_trait,
            Result<(MPathElement, Entry<(Weight, Self::TreeId), Self::LeafId>)>,
        >,
    > {
        Ok(stream::iter(
            OrderedManifest::list_weighted(self)
                .map(anyhow::Ok)
                .collect::<Vec<_>>(),
        )
        .boxed())
    }
    async fn lookup_weighted(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<(Weight, Self::TreeId), Self::LeafId>>> {
        anyhow::Ok(OrderedManifest::lookup_weighted(self, name))
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Entry<T, L> {
    Tree(T),
    Leaf(L),
}

impl<T, L> Entry<T, L> {
    pub fn into_tree(self) -> Option<T> {
        match self {
            Entry::Tree(tree) => Some(tree),
            _ => None,
        }
    }

    pub fn into_leaf(self) -> Option<L> {
        match self {
            Entry::Leaf(leaf) => Some(leaf),
            _ => None,
        }
    }

    pub fn map_leaf<L2>(self, m: impl FnOnce(L) -> L2) -> Entry<T, L2> {
        match self {
            Entry::Tree(tree) => Entry::Tree(tree),
            Entry::Leaf(leaf) => Entry::Leaf(m(leaf)),
        }
    }

    pub fn map_tree<T2>(self, m: impl FnOnce(T) -> T2) -> Entry<T2, L> {
        match self {
            Entry::Tree(tree) => Entry::Tree(m(tree)),
            Entry::Leaf(leaf) => Entry::Leaf(leaf),
        }
    }

    pub fn left_entry<T2, L2>(self) -> Entry<Either<T, T2>, Either<L, L2>> {
        match self {
            Entry::Tree(tree) => Entry::Tree(Either::Left(tree)),
            Entry::Leaf(leaf) => Entry::Leaf(Either::Left(leaf)),
        }
    }

    pub fn right_entry<T2, L2>(self) -> Entry<Either<T2, T>, Either<L2, L>> {
        match self {
            Entry::Tree(tree) => Entry::Tree(Either::Right(tree)),
            Entry::Leaf(leaf) => Entry::Leaf(Either::Right(leaf)),
        }
    }

    pub fn is_tree(&self) -> bool {
        match self {
            Entry::Tree(_) => true,
            _ => false,
        }
    }
}

#[async_trait]
impl<T, L> Loadable for Entry<T, L>
where
    T: Loadable + Sync,
    L: Loadable + Sync,
{
    type Value = Entry<T::Value, L::Value>;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        Ok(match self {
            Entry::Tree(tree_id) => Entry::Tree(tree_id.load(ctx, blobstore).await?),
            Entry::Leaf(leaf_id) => Entry::Leaf(leaf_id.load(ctx, blobstore).await?),
        })
    }
}

#[async_trait]
impl<T, L> Storable for Entry<T, L>
where
    T: Storable + Send,
    L: Storable + Send,
{
    type Key = Entry<T::Key, L::Key>;

    async fn store<'a, B: Blobstore>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Key> {
        Ok(match self {
            Entry::Tree(tree) => Entry::Tree(tree.store(ctx, blobstore).await?),
            Entry::Leaf(leaf) => Entry::Leaf(leaf.store(ctx, blobstore).await?),
        })
    }
}
