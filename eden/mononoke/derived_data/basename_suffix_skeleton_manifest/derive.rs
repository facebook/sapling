/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! See docs/basename_suffix_skeleton_manifest.md for more information

use anyhow::Result;
use blobstore::Blobstore;
use blobstore::Storable;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use futures::future::FutureExt;
use manifest::derive_manifest_with_io_sender;
use manifest::Entry;
use manifest::LeafInfo;
use manifest::TreeInfo;
use mononoke_types::basename_suffix_skeleton_manifest::BasenameSuffixSkeletonManifest;
use mononoke_types::basename_suffix_skeleton_manifest::BssmDirectory;
use mononoke_types::basename_suffix_skeleton_manifest::BssmEntry;
use mononoke_types::BasenameSuffixSkeletonManifestId;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ContentId;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use skeleton_manifest::mapping::get_file_changes;

use crate::mapping::RootBasenameSuffixSkeletonManifest;

// 36 = code for '$'
const SENTINEL_CHAR: u8 = 36;

fn bsm_sentinel() -> MPathElement {
    MPathElement::new(vec![SENTINEL_CHAR]).unwrap()
}

/// Put reversed basename in beginning, plus sentinel in the end
struct BsmPath(MPath);

impl BsmPath {
    fn transform(path: MPath) -> Self {
        let (dirname, basename) = path.split_dirname();
        let mut basename = basename.clone();
        // Basename is reversed to allow for faster suffix queries
        basename.reverse();
        // Let's add a sentinel add the end of the path
        // This prevents bugs, otherwise files in top-level become files
        // But they should become directories
        // So a repo with files `file` and `dir/file` will become a repo with files
        // `elif/$` and `elif/dir/$`, which otherwise could cause a file-dir conflict.
        let dirname = MPath::join_opt_element(dirname.as_ref(), &bsm_sentinel());
        Self(MPath::from(basename).join(&dirname))
    }
}

/// Calculate a list of changes of the changeset, but putting the basename first and
/// reversing it.
fn get_fixed_up_changes(bcs: &BonsaiChangeset) -> Vec<(MPath, Option<(ContentId, FileType)>)> {
    get_file_changes(bcs)
        .into_iter()
        .map(|(path, content)| (BsmPath::transform(path).0, content))
        .collect()
}

async fn empty_mf(
    ctx: &CoreContext,
    blobstore: &impl Blobstore,
) -> Result<BasenameSuffixSkeletonManifestId> {
    let leaf = BasenameSuffixSkeletonManifest::empty();
    leaf.into_blob().store(ctx, blobstore).await
}

pub(crate) async fn derive_single(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: BonsaiChangeset,
    parents: Vec<RootBasenameSuffixSkeletonManifest>,
) -> Result<RootBasenameSuffixSkeletonManifest> {
    let changes = get_fixed_up_changes(&bonsai);
    let blobstore = derivation_ctx.blobstore();
    // Types to help understand how to use derive_manifest helper
    type Leaf = (ContentId, FileType);
    type LeafId = ();
    type TreeId = BssmDirectory;
    type IntermediateLeafId = LeafId;
    type Ctx = ();
    // TODO(yancouto): Optimise by doing the first query separately using the optimisations
    // in sharded map, which are unused in common manifest code recently
    let root = derive_manifest_with_io_sender(
        ctx.clone(),
        blobstore.clone(),
        parents.into_iter().map(|root| root.0),
        changes,
        // create_tree
        {
            cloned!(ctx, blobstore);
            move |info: TreeInfo<TreeId, IntermediateLeafId, Ctx>, fut_sender| {
                cloned!(ctx, blobstore);
                async move {
                    // Number of entries in subtree, including directories
                    let mut rollup_count = 1;
                    let entries =
                        info.subentries
                            .into_iter()
                            .map(|(path_el, (_maybe_ctx, entry_in))| {
                                let entry = match entry_in {
                                    Entry::Leaf(()) => BssmEntry::File,
                                    Entry::Tree(entry) => BssmEntry::Directory(entry),
                                };
                                rollup_count += entry.rollup_count();
                                (path_el, Some(entry))
                            });

                    let mf = BasenameSuffixSkeletonManifest::empty()
                        .update(&ctx, &blobstore, entries.collect())
                        .await?;
                    let entry = {
                        let blob = mf.into_blob();
                        let id = *blob.id();
                        fut_sender
                            .unbounded_send(
                                async move { blob.store(&ctx, &blobstore).await.map(|_| ()) }
                                    .boxed(),
                            )
                            .map_err(|err| {
                                anyhow::anyhow!("failed to send manifest future {}", err)
                            })?;
                        BssmDirectory { id, rollup_count }
                    };
                    anyhow::Ok(((), entry))
                }
            }
        },
        // create_leaf
        {
            move |_leaf_info: LeafInfo<IntermediateLeafId, Leaf>, _fut_sender| async move {
                anyhow::Ok(((), ()))
            }
        },
    )
    .await?;
    Ok(RootBasenameSuffixSkeletonManifest(match root {
        Some(root) => root,
        // Only happens on empty repo
        None => BssmDirectory {
            id: empty_mf(ctx, blobstore).await?,
            rollup_count: 1,
        },
    }))
}
