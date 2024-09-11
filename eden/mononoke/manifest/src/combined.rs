/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::LoadableError;
use blobstore::StoreLoadable;
use context::CoreContext;
use either::Either;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::try_join;
use mononoke_types::MPathElement;
use mononoke_types::TrieMap;

use crate::types::Entry;
use crate::types::Manifest;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CombinedId<M, N>(pub M, pub N);

pub struct Combined<M, N>(pub M, pub N);

fn combine_entries<
    M: Manifest<Store> + Send + Sync,
    N: Manifest<Store> + Send + Sync,
    Store: Send + Sync,
>(
    (m_result, n_result): (
        Result<(MPathElement, Entry<M::TreeId, M::Leaf>)>,
        Result<(MPathElement, Entry<N::TreeId, N::Leaf>)>,
    ),
) -> Result<(
    MPathElement,
    Entry<<Combined<M, N> as Manifest<Store>>::TreeId, <Combined<M, N> as Manifest<Store>>::Leaf>,
)> {
    let (m_elem, m_entry) = m_result?;
    let (n_elem, n_entry) = n_result?;

    match (m_elem == n_elem, m_entry, n_entry) {
        (true, Entry::Tree(m_tree), Entry::Tree(n_tree)) => {
            Ok((m_elem, Entry::Tree(CombinedId(m_tree, n_tree))))
        }
        (true, Entry::Leaf(m_leaf), Entry::Leaf(n_leaf)) => {
            Ok((m_elem, Entry::Leaf(CombinedId(m_leaf, n_leaf))))
        }
        _ => bail!(
            "Found non-matching entries while iterating over a pair of manifests: {} vs {}",
            m_elem,
            n_elem,
        ),
    }
}

#[async_trait]
impl<S, M, N> StoreLoadable<S> for CombinedId<M, N>
where
    M: StoreLoadable<S> + Send + Sync + Clone + Eq,
    M::Value: Send + Sync,
    N: StoreLoadable<S> + Send + Sync + Clone + Eq,
    N::Value: Send + Sync,
    S: Send + Sync,
{
    type Value = Combined<M::Value, N::Value>;

    async fn load<'a>(
        &'a self,
        ctx: &'a CoreContext,
        store: &'a S,
    ) -> Result<Self::Value, LoadableError> {
        let CombinedId(m_id, n_id) = self;
        let (m, n) = try_join!(m_id.load(ctx, store), n_id.load(ctx, store))?;
        Ok(Combined(m, n))
    }
}

#[async_trait]
impl<M: Manifest<Store> + Send + Sync, N: Manifest<Store> + Send + Sync, Store: Send + Sync>
    Manifest<Store> for Combined<M, N>
{
    type TreeId = CombinedId<<M as Manifest<Store>>::TreeId, <N as Manifest<Store>>::TreeId>;
    type Leaf = CombinedId<<M as Manifest<Store>>::Leaf, <N as Manifest<Store>>::Leaf>;
    type TrieMapType = TrieMap<Entry<Self::TreeId, Self::Leaf>>;

    async fn list(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        let Combined(m, n) = self;
        Ok(m.list(ctx, blobstore)
            .await?
            .zip(n.list(ctx, blobstore).await?)
            .map(combine_entries::<M, N, Store>)
            .boxed())
    }

    async fn list_prefix(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        let Combined(m, n) = self;
        Ok(m.list_prefix(ctx, blobstore, prefix)
            .await?
            .zip(n.list_prefix(ctx, blobstore, prefix).await?)
            .map(combine_entries::<M, N, Store>)
            .boxed())
    }

    async fn list_prefix_after(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
        after: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        let Combined(m, n) = self;
        Ok(m.list_prefix_after(ctx, blobstore, prefix, after)
            .await?
            .zip(n.list_prefix_after(ctx, blobstore, prefix, after).await?)
            .map(combine_entries::<M, N, Store>)
            .boxed())
    }

    async fn list_skip(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        skip: usize,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        let Combined(m, n) = self;
        Ok(m.list_skip(ctx, blobstore, skip)
            .await?
            .zip(n.list_skip(ctx, blobstore, skip).await?)
            .map(combine_entries::<M, N, Store>)
            .boxed())
    }

    async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::Leaf>>> {
        let Combined(m, n) = self;
        match (
            m.lookup(ctx, blobstore, name).await?,
            n.lookup(ctx, blobstore, name).await?,
        ) {
            (Some(Entry::Tree(m_tree)), Some(Entry::Tree(n_tree))) => {
                Ok(Some(Entry::Tree(CombinedId(m_tree, n_tree))))
            }
            (Some(Entry::Leaf(m_leaf)), Some(Entry::Leaf(n_leaf))) => {
                Ok(Some(Entry::Leaf(CombinedId(m_leaf, n_leaf))))
            }
            (None, None) => Ok(None),
            _ => bail!("Found non-matching entry types during lookup for {}", name),
        }
    }

    async fn into_trie_map(
        self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        self.list(ctx, blobstore).await?.try_collect().await
    }
}

#[async_trait]
impl<M: Manifest<Store> + Send + Sync, N: Manifest<Store> + Send + Sync, Store: Send + Sync>
    Manifest<Store> for Either<M, N>
{
    type TreeId = Either<<M as Manifest<Store>>::TreeId, <N as Manifest<Store>>::TreeId>;
    type Leaf = Either<<M as Manifest<Store>>::Leaf, <N as Manifest<Store>>::Leaf>;
    type TrieMapType =
        Either<<M as Manifest<Store>>::TrieMapType, <N as Manifest<Store>>::TrieMapType>;

    async fn list(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        let stream = match self {
            Either::Left(m) => m
                .list(ctx, blobstore)
                .await?
                .map_ok(|(path, entry)| (path, entry.left_entry()))
                .boxed(),
            Either::Right(n) => n
                .list(ctx, blobstore)
                .await?
                .map_ok(|(path, entry)| (path, entry.right_entry()))
                .boxed(),
        };
        Ok(stream)
    }

    async fn list_prefix(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        let stream = match self {
            Either::Left(m) => m
                .list_prefix(ctx, blobstore, prefix)
                .await?
                .map_ok(|(path, entry)| (path, entry.left_entry()))
                .boxed(),
            Either::Right(n) => n
                .list_prefix(ctx, blobstore, prefix)
                .await?
                .map_ok(|(path, entry)| (path, entry.right_entry()))
                .boxed(),
        };
        Ok(stream)
    }

    async fn list_prefix_after(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
        after: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        let stream = match self {
            Either::Left(m) => m
                .list_prefix_after(ctx, blobstore, prefix, after)
                .await?
                .map_ok(|(path, entry)| (path, entry.left_entry()))
                .boxed(),
            Either::Right(n) => n
                .list_prefix_after(ctx, blobstore, prefix, after)
                .await?
                .map_ok(|(path, entry)| (path, entry.right_entry()))
                .boxed(),
        };
        Ok(stream)
    }

    async fn list_skip(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        skip: usize,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        let stream = match self {
            Either::Left(m) => m
                .list_skip(ctx, blobstore, skip)
                .await?
                .map_ok(|(path, entry)| (path, entry.left_entry()))
                .boxed(),
            Either::Right(n) => n
                .list_skip(ctx, blobstore, skip)
                .await?
                .map_ok(|(path, entry)| (path, entry.right_entry()))
                .boxed(),
        };
        Ok(stream)
    }

    async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::Leaf>>> {
        match self {
            Either::Left(m) => Ok(m.lookup(ctx, blobstore, name).await?.map(Entry::left_entry)),
            Either::Right(n) => Ok(n
                .lookup(ctx, blobstore, name)
                .await?
                .map(Entry::right_entry)),
        }
    }

    async fn into_trie_map(
        self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        match self {
            Either::Left(m) => Ok(Either::Left(m.into_trie_map(ctx, blobstore).await?)),
            Either::Right(n) => Ok(Either::Right(n.into_trie_map(ctx, blobstore).await?)),
        }
    }
}
