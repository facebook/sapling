/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use blobstore::KeyedBlobstore;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use fsnodes::RootFsnodeId;
use futures::TryStreamExt;
use manifest::ManifestOps;
use manifest::derive_manifest;
use mononoke_types::ContentManifestId;
use mononoke_types::content_manifest::ContentManifestFile;

use crate::RootContentManifestId;
use crate::derive::create_content_manifest_directory;
use crate::derive::create_content_manifest_file;
use crate::derive::empty_directory;

pub(crate) async fn inner_derive_from_predecessor(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    restricted_paths: &Arc<restricted_paths::RestrictedPaths>,
    predecessor: mononoke_types::FsnodeId,
    chunk_size: usize,
) -> Result<Option<ContentManifestId>> {
    predecessor
        .list_leaf_entries(ctx.clone(), blobstore.clone())
        .map_ok(|(path, fsnode_file)| {
            (
                path,
                Some(ContentManifestFile {
                    content_id: *fsnode_file.content_id(),
                    file_type: *fsnode_file.file_type(),
                    size: fsnode_file.size(),
                }),
            )
        })
        .try_chunks(chunk_size)
        .map_err(anyhow::Error::msg)
        .try_fold(None, |maybe_content_manifest_id, paths| {
            cloned!(ctx, blobstore, restricted_paths);
            async move {
                let parents: Vec<ContentManifestId> =
                    maybe_content_manifest_id.into_iter().collect();
                let new_content_manifest_id = derive_manifest(
                    ctx.clone(),
                    blobstore.clone(),
                    parents,
                    paths,
                    Vec::new(),
                    {
                        cloned!(blobstore, ctx, restricted_paths);
                        move |tree_info| {
                            cloned!(blobstore, ctx, restricted_paths);
                            async move {
                                create_content_manifest_directory(
                                    ctx,
                                    blobstore,
                                    &tree_info.path,
                                    &restricted_paths,
                                    tree_info.subentries,
                                )
                                .await
                            }
                        }
                    },
                    create_content_manifest_file,
                )
                .await?;
                Ok(new_content_manifest_id)
            }
        })
        .await
}

pub(crate) async fn derive_from_predecessor(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    predecessor: RootFsnodeId,
) -> Result<RootContentManifestId> {
    let blobstore = derivation_ctx.blobstore();
    let restricted_paths = derivation_ctx.restricted_paths();
    let predecessor = predecessor.into_fsnode_id();

    let chunk_size = justknobs::get_as::<usize>(
        "scm/mononoke:content_manifest_derive_from_predecessor_chunk_size",
        None,
    )?;

    let maybe_id =
        inner_derive_from_predecessor(ctx, blobstore, &restricted_paths, predecessor, chunk_size)
            .await?;

    match maybe_id {
        Some(id) => Ok(RootContentManifestId(id)),
        None => Ok(RootContentManifestId(
            empty_directory(ctx, blobstore)
                .await
                .context("failed to store empty ContentManifest blob")?,
        )),
    }
}
