/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::Storable;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use either::Either;
use futures::channel::mpsc;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use manifest::derive_manifest_with_io_sender;
use manifest::Entry;
use manifest::LeafInfo;
use manifest::TreeInfoSubentries;
use mononoke_types::content_manifest::ContentManifest;
use mononoke_types::content_manifest::ContentManifestDirectory;
use mononoke_types::content_manifest::ContentManifestEntry;
use mononoke_types::content_manifest::ContentManifestFile;
use mononoke_types::sharded_map_v2::LoadableShardedMapV2Node;
use mononoke_types::sharded_map_v2::ShardedMapV2Node;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ContentManifestId;
use mononoke_types::NonRootMPath;
use mononoke_types::TrieMap;

use crate::ContentManifestDerivationError;

pub(crate) fn get_changes(
    bcs: &BonsaiChangeset,
) -> Vec<(NonRootMPath, Option<ContentManifestFile>)> {
    bcs.file_changes()
        .map(|(mpath, file_change)| {
            (
                mpath.clone(),
                file_change.simplify().map(|bc| ContentManifestFile {
                    content_id: bc.content_id(),
                    file_type: bc.file_type(),
                    size: bc.size(),
                }),
            )
        })
        .collect()
}

async fn empty_directory(
    ctx: &CoreContext,
    blobstore: &impl Blobstore,
) -> Result<ContentManifestId> {
    ContentManifest::empty()
        .into_blob()
        .store(ctx, blobstore)
        .await
}

fn create_entry(entry: Entry<ContentManifestId, ContentManifestFile>) -> ContentManifestEntry {
    match entry {
        Entry::Leaf(file) => ContentManifestEntry::File(file),
        Entry::Tree(id) => ContentManifestEntry::Directory(ContentManifestDirectory { id }),
    }
}

async fn create_content_manifest_directory(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    sender: &mpsc::UnboundedSender<BoxFuture<'static, Result<()>>>,
    subentries: TreeInfoSubentries<
        ContentManifestId,
        ContentManifestFile,
        (),
        LoadableShardedMapV2Node<ContentManifestEntry>,
    >,
) -> Result<((), ContentManifestId)> {
    let subentries: TrieMap<_> = subentries
        .into_iter()
        .map(|(prefix, entry_or_map)| match entry_or_map {
            Either::Left((_ctx, entry)) => (prefix, Either::Left(create_entry(entry))),
            Either::Right(map) => (prefix, Either::Right(map)),
        })
        .collect();

    let subentries =
        ShardedMapV2Node::from_entries_and_partial_maps(&ctx, &blobstore, subentries).await?;

    let directory = ContentManifest { subentries };
    let blob = directory.into_blob();
    let id = *blob.id();
    sender
        .unbounded_send(
            async move {
                blob.store(&ctx, &blobstore).await?;
                Ok(())
            }
            .boxed(),
        )
        .map_err(|e| anyhow!("failed to send manifest store future: {}", e))?;

    Ok(((), id))
}

async fn create_content_manifest_file(
    leaf_info: LeafInfo<ContentManifestFile, ContentManifestFile>,
) -> Result<((), ContentManifestFile)> {
    if let Some(file) = leaf_info.change {
        Ok(((), file))
    } else {
        let mut iter = leaf_info.parents.into_iter();
        let file = iter
            .next()
            .ok_or(ContentManifestDerivationError::NoParents)?;
        if iter.all(|next| next == file) {
            Ok(((), file))
        } else {
            Err(ContentManifestDerivationError::MergeConflictNotResolved)?
        }
    }
}

pub(crate) async fn derive_content_manifest(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: BonsaiChangeset,
    parents: Vec<ContentManifestId>,
) -> Result<ContentManifestId> {
    let blobstore = derivation_ctx.blobstore();
    let changes = get_changes(&bonsai);
    let derive_fut = derive_manifest_with_io_sender(
        ctx.clone(),
        blobstore.clone(),
        parents.clone(),
        changes,
        {
            cloned!(blobstore, ctx);
            move |tree_info, sender| {
                cloned!(blobstore, ctx);
                async move {
                    create_content_manifest_directory(ctx, blobstore, &sender, tree_info.subentries)
                        .await
                }
            }
        },
        |leaf_info, _sender| create_content_manifest_file(leaf_info),
    )
    .boxed();
    let root = derive_fut.await?;
    match root {
        Some(root) => Ok(root),
        None => Ok(empty_directory(ctx, blobstore).await?),
    }
}
