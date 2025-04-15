/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use context::CoreContext;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::ContentManifestId;
use mononoke_types::MPathElement;
use mononoke_types::content_manifest::ContentManifest;
use mononoke_types::content_manifest::ContentManifestEntry;
use mononoke_types::content_manifest::ContentManifestFile;
use mononoke_types::sharded_map_v2::LoadableShardedMapV2Node;

use super::Entry;
use super::Manifest;

#[async_trait]
impl<Store: Blobstore> Manifest<Store> for ContentManifest {
    type TreeId = ContentManifestId;
    type Leaf = ContentManifestFile;
    type TrieMapType = LoadableShardedMapV2Node<ContentManifestEntry>;

    async fn list(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_subentries(ctx, blobstore)
                .map_ok(|(path, entry)| (path, convert_content_manifest(entry)))
                .boxed(),
        )
    }

    async fn list_prefix(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_prefix_subentries(ctx, blobstore, prefix)
                .map_ok(|(path, entry)| (path, convert_content_manifest(entry)))
                .boxed(),
        )
    }

    async fn list_prefix_after(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
        after: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_prefix_subentries_after(ctx, blobstore, prefix, after)
                .map_ok(|(path, entry)| (path, convert_content_manifest(entry)))
                .boxed(),
        )
    }

    async fn list_skip(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        skip: usize,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_subentries_skip(ctx, blobstore, skip)
                .map_ok(|(path, entry)| (path, convert_content_manifest(entry)))
                .boxed(),
        )
    }

    async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::Leaf>>> {
        Ok(self
            .lookup(ctx, blobstore, name)
            .await?
            .map(convert_content_manifest))
    }

    async fn into_trie_map(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        Ok(LoadableShardedMapV2Node::Inlined(self.subentries))
    }
}

pub(crate) fn convert_content_manifest(
    content_manifest_entry: ContentManifestEntry,
) -> Entry<ContentManifestId, ContentManifestFile> {
    match content_manifest_entry {
        ContentManifestEntry::File(file) => Entry::Leaf(file),
        ContentManifestEntry::Directory(dir) => Entry::Tree(dir.id),
    }
}
