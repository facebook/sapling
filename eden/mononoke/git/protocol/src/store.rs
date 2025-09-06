/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use bytes::Bytes;
use context::CoreContext;
use futures::TryStreamExt;
use git_types::GitDeltaManifestEntryOps;
use git_types::GitIdentifier;
use git_types::GitPackfileBaseItem;
use git_types::HeaderState;
use git_types::PackfileItem;
use git_types::fetch_git_delta_manifest;
use git_types::fetch_git_object_bytes;
use git_types::fetch_non_blob_git_object;
use git_types::fetch_non_blob_git_object_bytes;
use git_types::fetch_packfile_base_item;
use git_types::fetch_packfile_base_item_if_exists;
use git_types::upload_packfile_base_item;
use gix_hash::ObjectId;
use metaconfig_types::GitDeltaManifestVersion;
use mononoke_types::ChangesetId;
use repo_blobstore::ArcRepoBlobstore;
use repo_derived_data::ArcRepoDerivedData;
use rustc_hash::FxHashSet;

use crate::types::FetchContainer;
use crate::types::PackfileItemInclusion;
use crate::utils::delta_base;
use crate::utils::filter_object;

/// The type of identifier used for identifying the base git object
/// for fetching from the blobstore
pub(crate) enum ObjectIdentifierType {
    /// A GitIdentifier hash has information about the type and size of the object
    /// and hence can be used as an identifier for all types of Git objects
    AllObjects(GitIdentifier),
    /// The ObjectId cannot provide type and size information and hence should be
    /// used only when the object is NOT a blob
    NonBlobObjects(ObjectId),
}

impl ObjectIdentifierType {
    pub(crate) fn to_object_id(&self) -> Result<ObjectId> {
        match self {
            Self::AllObjects(ident) => ident.to_object_id(),
            Self::NonBlobObjects(oid) => Ok(*oid),
        }
    }
}

/// Fetch the raw content of the Git object based on the type of identifier provided
async fn object_bytes(
    ctx: &CoreContext,
    blobstore: ArcRepoBlobstore,
    id: ObjectIdentifierType,
) -> Result<Bytes> {
    let bytes = match id {
        ObjectIdentifierType::AllObjects(git_ident) => {
            // The object identifier has been passed along with size and type information. This means
            // that it can be any type of Git object. We store Git blobs as file content and all other
            // Git objects as raw git content. The fetch_git_object_bytes function fetches from the appropriate
            // source depending on the type of the object.
            fetch_git_object_bytes(ctx, blobstore.clone(), &git_ident, HeaderState::Included)
                .await?
        }
        ObjectIdentifierType::NonBlobObjects(oid) => {
            // The object identifier has only been passed with an ObjectId. This means that it must be a
            // non-blob Git object that can be fetched directly from the blobstore.
            fetch_non_blob_git_object_bytes(ctx, &blobstore, oid.as_ref()).await?
        }
    };
    Ok(bytes)
}

/// Fetch (or generate and fetch) the packfile item for the base git object
/// based on the packfile_item_inclusion setting
pub(crate) async fn base_packfile_item(
    ctx: Arc<CoreContext>,
    blobstore: ArcRepoBlobstore,
    id: ObjectIdentifierType,
    packfile_item_inclusion: PackfileItemInclusion,
) -> Result<PackfileItem> {
    let git_objectid = id.to_object_id()?;
    match packfile_item_inclusion {
        // Generate the packfile item based on the raw commit object
        PackfileItemInclusion::Generate => {
            let object_bytes = object_bytes(&ctx, blobstore.clone(), id).await.with_context(|| {
                format!(
                    "Error in fetching raw git object bytes for object {:?} while generating packfile item",
                    &git_objectid
                )
            })?;
            let packfile_item = PackfileItem::new_base(object_bytes).with_context(|| {
                format!(
                    "Error in creating packfile item from git object bytes for {:?}",
                    &git_objectid
                )
            })?;
            Ok(packfile_item)
        }
        // Return the stored packfile item if it exists, otherwise error out
        PackfileItemInclusion::FetchOnly => {
            let packfile_base_item =
                fetch_packfile_base_item(&ctx, &blobstore, git_objectid.as_ref())
                    .await
                    .with_context(|| {
                        format!(
                            "Error in fetching packfile item for git object {:?} in FetchOnly mode",
                            &git_objectid
                        )
                    })?;
            Ok(PackfileItem::new_encoded_base(
                packfile_base_item.try_into()?,
            ))
        }
        // Return the stored packfile item if its exists, if it doesn't exist, generate it and store it
        PackfileItemInclusion::FetchAndStore => {
            let fetch_result = fetch_packfile_base_item_if_exists(
                &ctx,
                &blobstore,
                git_objectid.as_ref(),
            )
            .await
            .with_context(|| {
                format!(
                    "Error in fetching packfile item for git object {:?} in FetchAndStore mode",
                    &git_objectid
                )
            })?;
            match fetch_result {
                Some(packfile_base_item) => Ok(PackfileItem::new_encoded_base(
                    packfile_base_item.try_into()?,
                )),
                None => {
                    let object_bytes = object_bytes(&ctx, blobstore.clone(), id).await.with_context(|| {
                        format!(
                            "Error in fetching raw git object bytes for object {:?} while fetching-and-storing packfile item",
                            &git_objectid
                        )
                    })?;
                    let packfile_base_item = upload_packfile_base_item(
                        &ctx,
                        &blobstore,
                        git_objectid.as_ref(),
                        object_bytes.to_vec(),
                    )
                    .await?;
                    Ok(PackfileItem::new_encoded_base(
                        packfile_base_item.try_into()?,
                    ))
                }
            }
        }
    }
}

/// Fetch the delta manifest entries for the given changeset
pub(crate) async fn changeset_delta_manifest_entries(
    ctx: Arc<CoreContext>,
    blobstore: ArcRepoBlobstore,
    derived_data: ArcRepoDerivedData,
    git_delta_manifest_version: GitDeltaManifestVersion,
    changeset_id: ChangesetId,
) -> Result<Vec<(ChangesetId, Box<dyn GitDeltaManifestEntryOps + Send>)>> {
    let delta_manifest = fetch_git_delta_manifest(
        &ctx,
        &derived_data,
        &blobstore,
        git_delta_manifest_version,
        changeset_id,
    )
    .await?;
    // Most delta manifests would contain tens of entries. These entries are just metadata and
    // not the actual object so its safe to load them all into memory instead of chaining streams
    // which significantly slows down the entire process.
    delta_manifest
        .into_entries(&ctx, &blobstore.boxed())
        .map_ok(|entry| (changeset_id, entry))
        .try_collect::<Vec<_>>()
        .await
}

/// Fetch the packfile item for the given delta manifest entry
pub(crate) async fn packfile_item_for_delta_manifest_entry(
    fetch_container: FetchContainer,
    base_set: Arc<FxHashSet<ObjectId>>,
    mut entry: Box<dyn GitDeltaManifestEntryOps + Send>,
) -> Result<Option<PackfileItem>> {
    let FetchContainer {
        ctx,
        blobstore,
        delta_inclusion,
        filter,
        packfile_item_inclusion,
        chain_breaking_mode,
        ..
    } = fetch_container;

    let object_id = entry.full_object_oid();
    let (kind, size) = (entry.full_object_kind(), entry.full_object_size());
    if base_set.contains(&object_id) {
        // This object is already present at the client, so do not include it in the packfile
        return Ok(None);
    }
    if !filter_object(filter.clone(), entry.path(), kind, size) {
        // This object does not pass the filter specified by the client, so do not include it in the packfile
        return Ok(None);
    }

    let delta = delta_base(entry.as_ref(), delta_inclusion, filter, chain_breaking_mode);
    match delta {
        Some(delta) => {
            let instruction_bytes = delta.instruction_bytes(&ctx, &blobstore.boxed()).await?;

            let packfile_item = PackfileItem::new_delta(
                entry.full_object_oid(),
                delta.base_object_oid(),
                delta.instructions_uncompressed_size(),
                instruction_bytes,
            );
            Ok(Some(packfile_item))
        }
        None => {
            // Use the full object instead
            if let Some(inlined_bytes) = entry.into_full_object_inlined_bytes() {
                Ok(Some(PackfileItem::new_encoded_base(
                    GitPackfileBaseItem::from_encoded_bytes(inlined_bytes)
                        .with_context(|| {
                            format!(
                                "Error in creating GitPackfileBaseItem from encoded bytes for {:?}",
                                entry.full_object_oid(),
                            )
                        })?
                        .try_into()?,
                )))
            } else {
                Ok(Some(
                    base_packfile_item(
                        ctx.clone(),
                        blobstore.clone(),
                        ObjectIdentifierType::AllObjects(GitIdentifier::Rich(
                            entry.full_object_rich_git_sha1()?,
                        )),
                        packfile_item_inclusion,
                    )
                    .await?,
                ))
            }
        }
    }
}

/// Method for fetching the entire hierarchy of nested tags
pub(crate) async fn fetch_nested_tags(
    ctx: &CoreContext,
    blobstore: &ArcRepoBlobstore,
    tag_hash: ObjectId,
) -> Result<Vec<ObjectId>> {
    let mut maybe_target = Some(tag_hash);
    let mut nested_tags = vec![];
    while let Some(target_hash) = maybe_target {
        let target_object = fetch_non_blob_git_object(ctx, blobstore, target_hash.as_ref()).await?;
        if let Some(next_target) = target_object.with_parsed_as_tag(|tag| tag.target()) {
            // The current object is tag so add it to the list of nested tags
            nested_tags.push(target_hash.clone());
            // The target of the tag is the next object to be fetched
            maybe_target = Some(next_target);
        } else {
            maybe_target = None;
        }
    }
    Ok(nested_tags)
}
