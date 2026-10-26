/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use blobstore::KeyedBlobstore;
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use manifest::BonsaiDiffFileChange;
use manifest::ManifestOps;
use manifest::bonsai_diff;
use manifest::bonsai_diff_sharded;
use mercurial_types::FileType;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgFileEnvelope;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::NULL_HASH;
use mercurial_types::NonRootMPath;
use mercurial_types::RepoPath;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::blobs::HgBlobEnvelope;
use mercurial_types::sharded_augmented_manifest::HgAugmentedFileLeafNode;
use mononoke_types::BlobstoreKey;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::GitLfs;
use mononoke_types::MPath;
use mononoke_types::subtree_change::SubtreeChange;
use repo_blobstore::RepoBlobstore;
use sorted_vector_map::SortedVectorMap;

/// Creates bonsai changeset from already created HgBlobChangeset.
pub async fn create_bonsai_changeset_object(
    ctx: &CoreContext,
    cs: HgBlobChangeset,
    parent_manifests: Vec<HgManifestId>,
    bonsai_parents: Vec<ChangesetId>,
    subtree_changes: SortedVectorMap<MPath, SubtreeChange>,
    blobstore: &RepoBlobstore,
    read_from_augmented: bool,
) -> Result<BonsaiChangeset, Error> {
    let file_changes = find_file_changes(
        ctx,
        cs.clone(),
        parent_manifests,
        blobstore,
        bonsai_parents.clone(),
        read_from_augmented,
    )
    .await?;

    let extra = cs
        .extra()
        .iter()
        .map(|(key, value)| {
            // Extra keys must be valid UTF-8.   Mercurial supports arbitrary
            // bytes, but that is not supported in Mononoke.  Extra values can
            // be arbitrary bytes.
            let key = String::from_utf8(key.to_vec())?;
            Ok((key, value.clone()))
        })
        .collect::<Result<SortedVectorMap<_, _>, Error>>()?;

    let author = String::from_utf8(cs.user().to_vec())
        .with_context(|| format!("While converting author name {:?}", cs.user()))?;
    let message = String::from_utf8(cs.message().to_vec())
        .with_context(|| format!("While converting commit message {:?}", cs.message()))?;
    BonsaiChangesetMut {
        parents: bonsai_parents,
        author,
        author_date: *cs.time(),
        message,
        hg_extra: extra.into_iter().map(|(k, v)| (k, v.into())).collect(),
        file_changes,
        subtree_changes,
        ..Default::default()
    }
    .freeze()
}

pub async fn save_bonsai_changeset_object(
    ctx: &CoreContext,
    blobstore: &RepoBlobstore,
    bonsai_cs: BonsaiChangeset,
) -> Result<(), Error> {
    let bonsai_blob = bonsai_cs.into_blob();
    let bcs_id = bonsai_blob.id().clone();
    let blobstore_key = bcs_id.blobstore_key();

    blobstore.put(ctx, blobstore_key, bonsai_blob.into()).await
}

// Finds files that were changed in the commit and returns it in the format suitable for BonsaiChangeset
async fn find_file_changes(
    ctx: &CoreContext,
    cs: HgBlobChangeset,
    parent_manifests: Vec<HgManifestId>,
    blobstore: &RepoBlobstore,
    bonsai_parents: Vec<ChangesetId>,
    read_from_augmented: bool,
) -> Result<SortedVectorMap<NonRootMPath, FileChange>, Error> {
    let diff: Result<_, Error> = manifest_bonsai_diff(
        ctx,
        blobstore,
        cs.manifestid(),
        &parent_manifests,
        read_from_augmented,
    )
    .map_ok(|diff| {
        cloned!(parent_manifests, bonsai_parents);
        async move {
            match diff {
                BonsaiDiffFileChange::Changed(path, (ty, entry_id)) => {
                    let file_node_id = HgFileNodeId::new(entry_id.into_nodehash());
                    let envelope = file_node_id
                        .load(ctx, blobstore)
                        .await
                        .context("While fetching bonsai file changes")?;
                    let size = envelope.content_size();
                    let content_id = envelope.content_id();

                    let copyinfo = get_copy_info(
                        ctx.clone(),
                        blobstore.clone(),
                        bonsai_parents,
                        path.clone(),
                        envelope,
                        parent_manifests,
                        read_from_augmented,
                    )
                    .await
                    .context("While fetching copy information")?;
                    Ok((
                        path,
                        FileChange::tracked(content_id, ty, size, copyinfo, GitLfs::FullContent),
                    ))
                }
                BonsaiDiffFileChange::ChangedReusedId(path, (ty, entry_id)) => {
                    let file_node_id = HgFileNodeId::new(entry_id.into_nodehash());
                    let envelope = file_node_id
                        .load(ctx, blobstore)
                        .await
                        .context("While fetching bonsai file changes")?;
                    let size = envelope.content_size();
                    let content_id = envelope.content_id();

                    // Reused ID means copy info is *not* stored.
                    Ok((
                        path,
                        FileChange::tracked(content_id, ty, size, None, GitLfs::FullContent),
                    ))
                }
                BonsaiDiffFileChange::Deleted(path) => Ok((path, FileChange::Deletion)),
            }
        }
    })
    .try_buffer_unordered(100)
    .try_collect::<std::collections::BTreeMap<_, _>>()
    .await;

    Ok(SortedVectorMap::from_iter(diff?))
}

/// `bonsai_diff` of `root` against `parents`, over their augmented manifests
/// (skipping shards that are the same in all of them) when
/// `read_from_augmented` is set, and over their Hg manifests otherwise.
fn manifest_bonsai_diff(
    ctx: &CoreContext,
    blobstore: &RepoBlobstore,
    root: HgManifestId,
    parents: &[HgManifestId],
    read_from_augmented: bool,
) -> BoxStream<'static, Result<BonsaiDiffFileChange<(FileType, HgFileNodeId)>>> {
    if !read_from_augmented {
        return bonsai_diff(
            ctx.clone(),
            blobstore.clone(),
            root,
            parents.iter().cloned().collect(),
        )
        .boxed();
    }
    // The null manifest is empty and has no augmented manifest.
    let parents: Vec<HgAugmentedManifestId> = parents
        .iter()
        .filter(|id| id.into_nodehash() != NULL_HASH)
        .map(|id| HgAugmentedManifestId::from(*id))
        .collect();
    let diff = if root.into_nodehash() == NULL_HASH {
        // Every file of every parent is deleted.
        stream::iter(parents)
            .map({
                cloned!(ctx, blobstore);
                move |parent| parent.list_leaf_entries(ctx.clone(), blobstore.clone())
            })
            .flatten()
            .map_ok(|(path, _)| BonsaiDiffFileChange::Deleted(path))
            .boxed()
    } else {
        bonsai_diff_sharded(
            ctx.clone(),
            blobstore.clone(),
            root.into(),
            parents.into_iter().collect(),
        )
        .boxed()
    };
    diff.map_ok(|change| {
        change.map_leaf(|leaf: HgAugmentedFileLeafNode| {
            (leaf.file_type, HgFileNodeId::new(leaf.filenode))
        })
    })
    .boxed()
}

// Returns copy information for a given path and node if this file was copied.
// This function is quite complicated because hg and bonsai store copy information differently.
// In hg copy information is (path, filenode), in bonsai it's (path, parent cs id). That means that
// we need to find a parent from which this filenode was copied.
async fn get_copy_info(
    ctx: CoreContext,
    blobstore: RepoBlobstore,
    bonsai_parents: Vec<ChangesetId>,
    copy_to_path: NonRootMPath,
    envelope: HgFileEnvelope,
    parent_manifests: Vec<HgManifestId>,
    read_from_augmented: bool,
) -> Result<Option<(NonRootMPath, ChangesetId)>, Error> {
    let node_id = envelope.node_id();

    let maybecopy = envelope
        .get_copy_info()?
        .map(|(path, hash)| (RepoPath::FilePath(path), hash));

    match maybecopy {
        Some((copy_from_path, copy_from_node)) => {
            let copy_from_path = copy_from_path
                .mpath()
                .ok_or_else(|| anyhow!("Root path wasn't expected at this context"))?;

            let parents_bonsai_and_mfs =
                stream::iter(bonsai_parents.into_iter().zip(parent_manifests));

            let get_bonsai_cs_copied_from =
                |(bonsai_parent, parent_mf): (ChangesetId, HgManifestId)| {
                    cloned!(ctx, blobstore);
                    async move {
                        let parent_filenode = if read_from_augmented {
                            find_augmented_filenode(ctx, blobstore, parent_mf, copy_from_path)
                                .await?
                        } else {
                            parent_mf
                                .find_entry(ctx, blobstore, copy_from_path.clone().into())
                                .await
                                .ok()
                                .flatten()
                                .and_then(|entry| entry.into_leaf())
                                .map(|(_, filenode)| filenode)
                        };
                        anyhow::Ok(
                            (parent_filenode == Some(copy_from_node)).then_some(bonsai_parent),
                        )
                    }
                };

            let copied_from = parents_bonsai_and_mfs
                .then(get_bonsai_cs_copied_from)
                .try_filter_map(|x| async move { Ok(x) })
                .try_collect::<Vec<ChangesetId>>()
                .await?;

            match copied_from.first() {
                Some(bonsai_cs_copied_from) => Ok(Some((
                    copy_from_path.clone(),
                    bonsai_cs_copied_from.clone(),
                ))),
                None => Err(anyhow!(
                    "Incorrect copy info: not found a file version {copy_from_path} {copy_from_node} the file {copy_to_path} {node_id} was copied from"
                )),
            }
        }
        None => Ok(None),
    }
}

/// The filenode at `path` in the augmented manifest of `manifest`, if `path`
/// is a file there.
async fn find_augmented_filenode(
    ctx: CoreContext,
    blobstore: RepoBlobstore,
    manifest: HgManifestId,
    path: &NonRootMPath,
) -> Result<Option<HgFileNodeId>> {
    // The null manifest is empty and has no augmented manifest.
    if manifest.into_nodehash() == NULL_HASH {
        return Ok(None);
    }
    let entry = HgAugmentedManifestId::from(manifest)
        .find_entry(ctx, blobstore, path.clone().into())
        .await?;
    Ok(entry
        .and_then(|entry| entry.into_leaf())
        .map(|leaf| HgFileNodeId::new(leaf.filenode)))
}
