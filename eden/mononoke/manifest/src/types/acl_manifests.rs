/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use blobstore::KeyedBlobstore;
use context::CoreContext;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::MPathElement;
use mononoke_types::acl_manifest::AclManifest;
use mononoke_types::acl_manifest::AclManifestEntry;
use mononoke_types::acl_manifest::AclManifestRestriction;
use mononoke_types::sharded_map_v2::LoadableShardedMapV2Node;
use mononoke_types::typed_hash::AclManifestId;

use super::Entry;
use super::Manifest;

pub(crate) fn convert_acl_manifest(
    entry: AclManifestEntry,
) -> Entry<AclManifestId, AclManifestRestriction> {
    match entry {
        AclManifestEntry::AclFile(restriction) => Entry::Leaf(restriction),
        AclManifestEntry::Directory(dir) => Entry::Tree(dir.id),
    }
}

#[async_trait]
impl<Store: KeyedBlobstore> Manifest<Store> for AclManifest {
    type TreeId = AclManifestId;
    type Leaf = AclManifestRestriction;
    type TrieMapType = LoadableShardedMapV2Node<AclManifestEntry>;

    async fn list(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        anyhow::Ok(
            self.clone()
                .into_subentries(ctx, blobstore)
                .map_ok(|(path, entry)| (path, convert_acl_manifest(entry)))
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
            .map(convert_acl_manifest))
    }

    async fn into_trie_map(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        Ok(LoadableShardedMapV2Node::Inlined(self.subentries))
    }
}
