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
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::basename_suffix_skeleton_manifest_v3::BssmV3Directory;
use mononoke_types::basename_suffix_skeleton_manifest_v3::BssmV3Entry;
use mononoke_types::fsnode::Fsnode;
use mononoke_types::fsnode::FsnodeEntry;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::sharded_map_v2::LoadableShardedMapV2Node;
use mononoke_types::skeleton_manifest::SkeletonManifest;
use mononoke_types::skeleton_manifest::SkeletonManifestEntry;
use mononoke_types::skeleton_manifest_v2::SkeletonManifestV2;
use mononoke_types::skeleton_manifest_v2::SkeletonManifestV2Entry;
use mononoke_types::test_manifest::TestManifest;
use mononoke_types::test_manifest::TestManifestDirectory;
use mononoke_types::test_manifest::TestManifestEntry;
use mononoke_types::test_sharded_manifest::TestShardedManifest;
use mononoke_types::test_sharded_manifest::TestShardedManifestDirectory;
use mononoke_types::test_sharded_manifest::TestShardedManifestEntry;
use mononoke_types::unode::ManifestUnode;
use mononoke_types::unode::UnodeEntry;
use mononoke_types::FileUnodeId;
use mononoke_types::FsnodeId;
use mononoke_types::MPathElement;
use mononoke_types::ManifestUnodeId;
use mononoke_types::SkeletonManifestId;
use mononoke_types::SortedVectorTrieMap;
use serde_derive::Deserialize;
use serde_derive::Serialize;

#[async_trait]
pub trait AsyncManifest<Store: Send + Sync>: Sized + 'static {
    type TreeId: Send + Sync;
    type LeafId: Send + Sync;
    type TrieMapType: Send + Sync;

    async fn list(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>;
    /// List all subentries with a given prefix
    async fn list_prefix(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>;
    /// List all subentries with a given prefix after a specific key
    async fn list_prefix_after(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
        after: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>;
    /// List all subentries, skipping the first N
    async fn list_skip(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        skip: usize,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>;
    async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::LeafId>>>;
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

pub(crate) fn bssm_v3_to_mf_entry(entry: BssmV3Entry) -> Entry<BssmV3Directory, ()> {
    match entry {
        BssmV3Entry::Directory(dir) => Entry::Tree(dir),
        BssmV3Entry::File => Entry::Leaf(()),
    }
}

#[async_trait]
impl<Store: Blobstore> AsyncManifest<Store> for BssmV3Directory {
    type TreeId = BssmV3Directory;
    type LeafId = ();
    type TrieMapType = LoadableShardedMapV2Node<BssmV3Entry>;

    async fn list(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_subentries(ctx, blobstore)
                .map_ok(|(path, entry)| (path, bssm_v3_to_mf_entry(entry)))
                .boxed(),
        )
    }

    async fn list_prefix(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_prefix_subentries(ctx, blobstore, prefix)
                .map_ok(|(path, entry)| (path, bssm_v3_to_mf_entry(entry)))
                .boxed(),
        )
    }

    async fn list_prefix_after(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
        after: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_prefix_subentries_after(ctx, blobstore, prefix, after)
                .map_ok(|(path, entry)| (path, bssm_v3_to_mf_entry(entry)))
                .boxed(),
        )
    }

    async fn list_skip(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        skip: usize,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_subentries_skip(ctx, blobstore, skip)
                .map_ok(|(path, entry)| (path, bssm_v3_to_mf_entry(entry)))
                .boxed(),
        )
    }

    async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::LeafId>>> {
        Ok(self
            .lookup(ctx, blobstore, name)
            .await?
            .map(bssm_v3_to_mf_entry))
    }

    async fn into_trie_map(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        Ok(LoadableShardedMapV2Node::Inlined(self.subentries))
    }
}

pub(crate) fn skeleton_manifest_v2_to_mf_entry(
    entry: SkeletonManifestV2Entry,
) -> Entry<SkeletonManifestV2, ()> {
    match entry {
        SkeletonManifestV2Entry::Directory(dir) => Entry::Tree(dir),
        SkeletonManifestV2Entry::File => Entry::Leaf(()),
    }
}

#[async_trait]
impl<Store: Blobstore> AsyncManifest<Store> for SkeletonManifestV2 {
    type TreeId = SkeletonManifestV2;
    type LeafId = ();
    type TrieMapType = LoadableShardedMapV2Node<SkeletonManifestV2Entry>;

    async fn list(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_subentries(ctx, blobstore)
                .map_ok(|(path, entry)| (path, skeleton_manifest_v2_to_mf_entry(entry)))
                .boxed(),
        )
    }

    async fn list_prefix(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_prefix_subentries(ctx, blobstore, prefix)
                .map_ok(|(path, entry)| (path, skeleton_manifest_v2_to_mf_entry(entry)))
                .boxed(),
        )
    }

    async fn list_prefix_after(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
        after: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_prefix_subentries_after(ctx, blobstore, prefix, after)
                .map_ok(|(path, entry)| (path, skeleton_manifest_v2_to_mf_entry(entry)))
                .boxed(),
        )
    }

    async fn list_skip(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        skip: usize,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_subentries_skip(ctx, blobstore, skip)
                .map_ok(|(path, entry)| (path, skeleton_manifest_v2_to_mf_entry(entry)))
                .boxed(),
        )
    }

    async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::LeafId>>> {
        Ok(self
            .lookup(ctx, blobstore, name)
            .await?
            .map(skeleton_manifest_v2_to_mf_entry))
    }

    async fn into_trie_map(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        Ok(LoadableShardedMapV2Node::Inlined(self.subentries))
    }
}

impl Manifest for ManifestUnode {
    type TreeId = ManifestUnodeId;
    type LeafId = FileUnodeId;

    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
        self.lookup(name).map(convert_unode)
    }

    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
        let v: Vec<_> = self
            .list()
            .map(|(basename, entry)| (basename.clone(), convert_unode(entry)))
            .collect();
        Box::new(v.into_iter())
    }
}

fn convert_unode(unode_entry: &UnodeEntry) -> Entry<ManifestUnodeId, FileUnodeId> {
    match unode_entry {
        UnodeEntry::File(file_unode_id) => Entry::Leaf(file_unode_id.clone()),
        UnodeEntry::Directory(mf_unode_id) => Entry::Tree(mf_unode_id.clone()),
    }
}

impl Manifest for Fsnode {
    type TreeId = FsnodeId;
    type LeafId = FsnodeFile;

    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
        self.lookup(name).map(convert_fsnode)
    }

    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
        let v: Vec<_> = self
            .list()
            .map(|(basename, entry)| (basename.clone(), convert_fsnode(entry)))
            .collect();
        Box::new(v.into_iter())
    }
}

fn convert_fsnode(fsnode_entry: &FsnodeEntry) -> Entry<FsnodeId, FsnodeFile> {
    match fsnode_entry {
        FsnodeEntry::File(fsnode_file) => Entry::Leaf(*fsnode_file),
        FsnodeEntry::Directory(fsnode_directory) => Entry::Tree(fsnode_directory.id().clone()),
    }
}

impl Manifest for SkeletonManifest {
    type TreeId = SkeletonManifestId;
    type LeafId = ();

    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
        self.lookup(name).map(convert_skeleton_manifest)
    }

    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
        let v: Vec<_> = self
            .list()
            .map(|(basename, entry)| (basename.clone(), convert_skeleton_manifest(entry)))
            .collect();
        Box::new(v.into_iter())
    }
}

fn convert_skeleton_manifest(
    skeleton_entry: &SkeletonManifestEntry,
) -> Entry<SkeletonManifestId, ()> {
    match skeleton_entry {
        SkeletonManifestEntry::File => Entry::Leaf(()),
        SkeletonManifestEntry::Directory(skeleton_directory) => {
            Entry::Tree(skeleton_directory.id().clone())
        }
    }
}

impl Manifest for TestManifest {
    type TreeId = TestManifestDirectory;
    type LeafId = ();

    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
        self.lookup(name).map(convert_test_manifest)
    }

    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
        let v: Vec<_> = self
            .list()
            .map(|(basename, entry)| (basename.clone(), convert_test_manifest(entry)))
            .collect();
        Box::new(v.into_iter())
    }
}

fn convert_test_manifest(
    test_manifest_entry: &TestManifestEntry,
) -> Entry<TestManifestDirectory, ()> {
    match test_manifest_entry {
        TestManifestEntry::File => Entry::Leaf(()),
        TestManifestEntry::Directory(dir) => Entry::Tree(dir.clone()),
    }
}

#[async_trait]
impl<Store: Blobstore> AsyncManifest<Store> for TestShardedManifest {
    type TreeId = TestShardedManifestDirectory;
    type LeafId = ();
    type TrieMapType = LoadableShardedMapV2Node<TestShardedManifestEntry>;

    async fn list(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_subentries(ctx, blobstore)
                .map_ok(|(path, entry)| (path, convert_test_sharded_manifest(entry)))
                .boxed(),
        )
    }

    async fn list_prefix(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_prefix_subentries(ctx, blobstore, prefix)
                .map_ok(|(path, entry)| (path, convert_test_sharded_manifest(entry)))
                .boxed(),
        )
    }

    async fn list_prefix_after(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
        after: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_prefix_subentries_after(ctx, blobstore, prefix, after)
                .map_ok(|(path, entry)| (path, convert_test_sharded_manifest(entry)))
                .boxed(),
        )
    }

    async fn list_skip(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        skip: usize,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::LeafId>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_subentries_skip(ctx, blobstore, skip)
                .map_ok(|(path, entry)| (path, convert_test_sharded_manifest(entry)))
                .boxed(),
        )
    }

    async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::LeafId>>> {
        Ok(self
            .lookup(ctx, blobstore, name)
            .await?
            .map(convert_test_sharded_manifest))
    }

    async fn into_trie_map(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        Ok(LoadableShardedMapV2Node::Inlined(self.subentries))
    }
}

pub(crate) fn convert_test_sharded_manifest(
    test_sharded_manifest_entry: TestShardedManifestEntry,
) -> Entry<TestShardedManifestDirectory, ()> {
    match test_sharded_manifest_entry {
        TestShardedManifestEntry::File(_file) => Entry::Leaf(()),
        TestShardedManifestEntry::Directory(dir) => Entry::Tree(dir),
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

fn convert_bssm_v3_to_weighted(
    entry: Entry<BssmV3Directory, ()>,
) -> Entry<(Weight, BssmV3Directory), ()> {
    match entry {
        Entry::Tree(dir) => Entry::Tree((
            dir.rollup_count()
                .into_inner()
                .try_into()
                .unwrap_or(usize::MAX),
            dir,
        )),
        Entry::Leaf(()) => Entry::Leaf(()),
    }
}

#[async_trait]
impl<Store: Blobstore> AsyncOrderedManifest<Store> for BssmV3Directory {
    async fn list_weighted(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<
        BoxStream<
            'async_trait,
            Result<(MPathElement, Entry<(Weight, Self::TreeId), Self::LeafId>)>,
        >,
    > {
        self.list(ctx, blobstore).await.map(|stream| {
            stream
                .map_ok(|(p, entry)| (p, convert_bssm_v3_to_weighted(entry)))
                .boxed()
        })
    }

    async fn lookup_weighted(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<(Weight, Self::TreeId), Self::LeafId>>> {
        AsyncManifest::lookup(self, ctx, blobstore, name)
            .await
            .map(|opt| opt.map(convert_bssm_v3_to_weighted))
    }
}

fn convert_skeleton_manifest_v2_to_weighted(
    entry: Entry<SkeletonManifestV2, ()>,
) -> Entry<(Weight, SkeletonManifestV2), ()> {
    match entry {
        Entry::Tree(dir) => Entry::Tree((
            dir.rollup_count()
                .into_inner()
                .try_into()
                .unwrap_or(usize::MAX),
            dir,
        )),
        Entry::Leaf(()) => Entry::Leaf(()),
    }
}

#[async_trait]
impl<Store: Blobstore> AsyncOrderedManifest<Store> for SkeletonManifestV2 {
    async fn list_weighted(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<
        BoxStream<
            'async_trait,
            Result<(MPathElement, Entry<(Weight, Self::TreeId), Self::LeafId>)>,
        >,
    > {
        self.list(ctx, blobstore).await.map(|stream| {
            stream
                .map_ok(|(p, entry)| (p, convert_skeleton_manifest_v2_to_weighted(entry)))
                .boxed()
        })
    }

    async fn lookup_weighted(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<(Weight, Self::TreeId), Self::LeafId>>> {
        AsyncManifest::lookup(self, ctx, blobstore, name)
            .await
            .map(|opt| opt.map(convert_skeleton_manifest_v2_to_weighted))
    }
}

impl OrderedManifest for SkeletonManifest {
    fn lookup_weighted(
        &self,
        name: &MPathElement,
    ) -> Option<Entry<(Weight, <Self as Manifest>::TreeId), <Self as Manifest>::LeafId>> {
        self.lookup(name).map(convert_skeleton_manifest_weighted)
    }

    fn list_weighted(
        &self,
    ) -> Box<
        dyn Iterator<
            Item = (
                MPathElement,
                Entry<(Weight, <Self as Manifest>::TreeId), <Self as Manifest>::LeafId>,
            ),
        >,
    > {
        let v: Vec<_> = self
            .list()
            .map(|(basename, entry)| (basename.clone(), convert_skeleton_manifest_weighted(entry)))
            .collect();
        Box::new(v.into_iter())
    }
}

fn convert_skeleton_manifest_weighted(
    skeleton_entry: &SkeletonManifestEntry,
) -> Entry<(Weight, SkeletonManifestId), ()> {
    match skeleton_entry {
        SkeletonManifestEntry::File => Entry::Leaf(()),
        SkeletonManifestEntry::Directory(skeleton_directory) => {
            let summary = skeleton_directory.summary();
            let weight = summary.descendant_files_count + summary.descendant_dirs_count;
            Entry::Tree((weight as Weight, skeleton_directory.id().clone()))
        }
    }
}

impl OrderedManifest for Fsnode {
    fn lookup_weighted(
        &self,
        name: &MPathElement,
    ) -> Option<Entry<(Weight, <Self as Manifest>::TreeId), <Self as Manifest>::LeafId>> {
        self.lookup(name).map(convert_fsnode_weighted)
    }

    fn list_weighted(
        &self,
    ) -> Box<
        dyn Iterator<
            Item = (
                MPathElement,
                Entry<(Weight, <Self as Manifest>::TreeId), <Self as Manifest>::LeafId>,
            ),
        >,
    > {
        let v: Vec<_> = self
            .list()
            .map(|(basename, entry)| (basename.clone(), convert_fsnode_weighted(entry)))
            .collect();
        Box::new(v.into_iter())
    }
}

fn convert_fsnode_weighted(fsnode_entry: &FsnodeEntry) -> Entry<(Weight, FsnodeId), FsnodeFile> {
    match fsnode_entry {
        FsnodeEntry::File(fsnode_file) => Entry::Leaf(*fsnode_file),
        FsnodeEntry::Directory(fsnode_directory) => {
            let summary = fsnode_directory.summary();
            // Fsnodes don't have a full descendant dirs count, so we use the
            // child count as a lower-bound estimate.
            let weight = summary.descendant_files_count + summary.child_dirs_count;
            Entry::Tree((weight as Weight, fsnode_directory.id().clone()))
        }
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
