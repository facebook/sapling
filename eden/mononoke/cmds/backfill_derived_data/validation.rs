/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_override::DangerousOverride;
use blobstore::Blobstore;
use blobstore::StoreLoadable;
use bonsai_hg_mapping::ArcBonsaiHgMapping;
use bonsai_hg_mapping::MemWritesBonsaiHgMapping;
use borrowed::borrowed;
use cacheblob::MemWritesBlobstore;
use clap_old::ArgMatches;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use context::CoreContext;
use derived_data::BonsaiDerived;
use derived_data_manager::BonsaiDerivable;
use derived_data_utils::derived_data_utils;
use derived_data_utils::DerivedUtils;
use derived_data_utils::DERIVED_DATA_DEPS;
use fsnodes::RootFsnodeId;
use futures::future::try_join;
use futures::future::try_join_all;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use manifest::find_intersection_of_diffs;
use manifest::find_intersection_of_diffs_and_parents;
use manifest::Entry;
use manifest::Manifest;
use mercurial_derived_data::MappedHgChangesetId;
use mononoke_types::BlobstoreKey;
use mononoke_types::ChangesetId;
use readonlyblob::ReadOnlyBlobstore;
use skeleton_manifest::RootSkeletonManifestId;
use slog::info;
use slog::warn;
use std::sync::Arc;
use std::sync::Once;
use unodes::RootUnodeManifestId;

use crate::commit_discovery::CommitDiscoveryOptions;
use crate::regenerate;
use crate::ARG_DERIVED_DATA_TYPE;
use crate::ARG_VALIDATE_CHUNK_SIZE;

pub async fn validate(
    ctx: &CoreContext,
    matches: &MononokeMatches<'_>,
    sub_m: &ArgMatches<'_>,
    repo_name: String,
) -> Result<(), Error> {
    if !matches.environment().readonly_storage.0 {
        return Err(anyhow!(
            "validate subcommand should be run only on readonly storage!"
        ));
    }
    let repo: BlobRepo =
        args::open_repo_by_name_unredacted(ctx.fb, ctx.logger(), matches, repo_name).await?;
    let csids = CommitDiscoveryOptions::from_matches(ctx, &repo, sub_m)
        .await?
        .get_commits();

    let derived_data_type = sub_m
        .value_of(ARG_DERIVED_DATA_TYPE)
        .ok_or_else(|| anyhow!("{} is not set", ARG_DERIVED_DATA_TYPE))?;
    info!(
        ctx.logger(),
        "Validating {} on {}...",
        derived_data_type,
        repo.name()
    );
    let opts = regenerate::DeriveOptions::from_matches(sub_m)?;

    let validate_chunk_size = args::get_usize(&sub_m, ARG_VALIDATE_CHUNK_SIZE, 10000);
    let warn_once = Once::new();

    info!(ctx.logger(), "Started validation");
    for chunk in csids.chunks(validate_chunk_size) {
        let chunk = chunk.to_vec();
        info!(
            ctx.logger(),
            "Processing chunk starting from {:?}",
            chunk.get(0)
        );
        let orig_repo = repo.clone();
        let mut memblobstore = None;
        let mut membonsaihgmapping = None;
        let repo = repo
            .dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
                let blobstore = Arc::new(MemWritesBlobstore::new(blobstore));
                memblobstore = Some(blobstore.clone());
                blobstore
            })
            .dangerous_override(|bonsai_hg_mapping| -> ArcBonsaiHgMapping {
                let bonsai_hg_mapping = Arc::new(MemWritesBonsaiHgMapping::new(bonsai_hg_mapping));
                membonsaihgmapping = Some(bonsai_hg_mapping.clone());
                bonsai_hg_mapping
            });
        let memblobstore = memblobstore.unwrap();
        let membonsaihgmapping = membonsaihgmapping.unwrap();
        // By default MemWritesBonsaiHgMapping doesn't save data to cache if it
        // already exists in underlying mapping. This option disables this feature.
        membonsaihgmapping.set_save_noop_writes(true);

        let types = std::iter::once(derived_data_type.to_string())
            .chain(
                DERIVED_DATA_DEPS
                    .get(derived_data_type)
                    .unwrap()
                    .iter()
                    .map(|t| t.to_string()),
            )
            .collect::<Vec<_>>();

        regenerate::regenerate_derived_data(ctx, &repo, chunk.clone(), types, &opts).await?;

        {
            let cache = memblobstore.get_cache().lock().unwrap();
            info!(ctx.logger(), "created {} blobs", cache.len());
        }
        let real_derived_utils = &derived_data_utils(ctx.fb, &orig_repo, derived_data_type)?;

        // Make sure that the generated data was saved in memory blobstore
        membonsaihgmapping.set_no_access_to_inner(true);
        membonsaihgmapping.set_readonly(true);
        memblobstore.set_no_access_to_inner(true);
        let repo = repo.dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
            Arc::new(ReadOnlyBlobstore::new(blobstore))
        });
        let rederived_utils = &derived_data_utils(ctx.fb, &repo, derived_data_type)?;

        borrowed!(ctx, orig_repo, repo, warn_once);
        stream::iter(chunk)
            .map(Ok)
            .try_for_each_concurrent(100, |csid| async move {
                if !rederived_utils.is_derived(ctx, csid).await? {
                    return Err(anyhow!("{} unexpectedly not derived", csid));
                }

                let f1 = real_derived_utils.derive(ctx.clone(), orig_repo.clone(), csid);
                let f2 = rederived_utils.derive(ctx.clone(), repo.clone(), csid);
                let (real, rederived) = try_join(f1, f2).await?;
                if real != rederived {
                    return Err(anyhow!("mismatch in {}: {} vs {}", csid, real, rederived));
                };

                validate_generated_data(ctx, orig_repo, warn_once, real_derived_utils, csid, repo)
                    .await
                    .with_context(|| format!("failed validating generated data for {}", csid))
            })
            .await?;
        info!(ctx.logger(), "Validation successful!");
    }

    Ok(())
}

async fn validate_generated_data<'a>(
    ctx: &'a CoreContext,
    real_repo: &'a BlobRepo,
    warn_once: &Once,
    real_derived_utils: &'a Arc<dyn DerivedUtils>,
    cs_id: ChangesetId,
    mem_blob_repo: &'a BlobRepo,
) -> Result<(), Error> {
    let mem_blob = mem_blob_repo.blobstore().boxed();
    if real_derived_utils.name() == RootFsnodeId::NAME {
        validate_fsnodes(ctx, real_repo, cs_id, &mem_blob).await?;
    } else if real_derived_utils.name() == RootSkeletonManifestId::NAME {
        validate_skeleton_manifests(ctx, real_repo, cs_id, &mem_blob).await?;
    } else if real_derived_utils.name() == RootUnodeManifestId::NAME {
        validate_unodes(ctx, real_repo, cs_id, &mem_blob).await?;
    } else if real_derived_utils.name() == MappedHgChangesetId::NAME {
        validate_hgchangesets(ctx, real_repo, cs_id, &mem_blob).await?;
    } else {
        warn_once.call_once(||
            warn!(
                ctx.logger(),
                "Validating generated blobs is not supported for {}, so no validation of generated blobs was done!",
                real_derived_utils.name()
            )
        );
    }

    Ok(())
}

async fn validate_fsnodes<'a>(
    ctx: &'a CoreContext,
    real_repo: &'a BlobRepo,
    cs_id: ChangesetId,
    mem_blob: &'a Arc<dyn Blobstore>,
) -> Result<(), Error> {
    let real_blobstore = real_repo.blobstore().boxed();
    let (fsnode, parents) =
        find_cs_and_parents_derived_data::<RootFsnodeId>(ctx, real_repo, cs_id).await?;
    let fsnode = *fsnode.fsnode_id();
    let parents = parents
        .into_iter()
        .map(|fsnode| *fsnode.fsnode_id())
        .collect::<Vec<_>>();

    validate_new_manifest_entries(
        ctx,
        real_blobstore,
        fsnode,
        parents,
        mem_blob,
        |tree_id| Some(tree_id.blobstore_key()),
        |_| None,
    )
    .await?;

    Ok(())
}

async fn validate_skeleton_manifests<'a>(
    ctx: &'a CoreContext,
    real_repo: &'a BlobRepo,
    cs_id: ChangesetId,
    mem_blob: &'a Arc<dyn Blobstore>,
) -> Result<(), Error> {
    let real_blobstore = real_repo.blobstore().boxed();

    let (skeleton_manifest, parents) =
        find_cs_and_parents_derived_data::<RootSkeletonManifestId>(ctx, real_repo, cs_id).await?;
    let skeleton_manifest = *skeleton_manifest.skeleton_manifest_id();
    let parents = parents
        .into_iter()
        .map(|skeleton_manifest| *skeleton_manifest.skeleton_manifest_id())
        .collect::<Vec<_>>();

    validate_new_manifest_entries(
        ctx,
        real_blobstore,
        skeleton_manifest,
        parents,
        mem_blob,
        |tree_id| Some(tree_id.blobstore_key()),
        |_| None,
    )
    .await?;

    Ok(())
}

async fn validate_unodes<'a>(
    ctx: &'a CoreContext,
    real_repo: &'a BlobRepo,
    cs_id: ChangesetId,
    mem_blob: &'a Arc<dyn Blobstore>,
) -> Result<(), Error> {
    let real_blobstore = real_repo.blobstore().boxed();
    let (unode, parents) =
        find_cs_and_parents_derived_data::<RootUnodeManifestId>(ctx, real_repo, cs_id).await?;
    let unode = *unode.manifest_unode_id();
    let parents = parents
        .into_iter()
        .map(|unode| *unode.manifest_unode_id())
        .collect::<Vec<_>>();
    validate_new_manifest_entries(
        ctx,
        real_blobstore,
        unode,
        parents,
        mem_blob,
        |tree_id| Some(tree_id.blobstore_key()),
        |leaf_id| Some(leaf_id.blobstore_key()),
    )
    .await?;

    Ok(())
}

async fn validate_hgchangesets<'a>(
    ctx: &'a CoreContext,
    real_repo: &'a BlobRepo,
    cs_id: ChangesetId,
    mem_blob: &'a Arc<dyn Blobstore>,
) -> Result<(), Error> {
    let real_blobstore = real_repo.blobstore().boxed();

    let (derived, parents) =
        find_cs_and_parents_derived_data::<MappedHgChangesetId>(ctx, real_repo, cs_id).await?;
    let mem_blob = &mem_blob;
    let manifest = async {
        let hgchangeset = derived
            .hg_changeset_id()
            .load(ctx, real_repo.blobstore())
            .await?;
        check_exists(
            ctx,
            mem_blob,
            hgchangeset.get_changeset_id().blobstore_key(),
        )
        .await?;
        Result::<_, Error>::Ok(hgchangeset.manifestid())
    };
    let parents = try_join_all(parents.into_iter().map(|p| async move {
        let p = p.hg_changeset_id().load(ctx, real_repo.blobstore()).await?;
        Result::<_, Error>::Ok(p.manifestid())
    }));

    let (manifest, parents) = try_join(manifest, parents).await?;

    let mf_entries =
        find_intersection_of_diffs_and_parents(ctx.clone(), real_blobstore, manifest, parents)
            .try_filter_map(|(_, entry, parent_entries)| async move {
                match entry {
                    Entry::Leaf((ty, filenode_id)) => {
                        for p in parent_entries {
                            if let Entry::Leaf((_ty, parent_filenode_id)) = p {
                                // This is mode-only change, no new blobstore writes were made
                                if parent_filenode_id == filenode_id {
                                    return Ok(None);
                                }
                            }
                        }
                        Ok(Some(Entry::Leaf((ty, filenode_id))))
                    }
                    Entry::Tree(manifest_id) => Ok(Some(Entry::Tree(manifest_id))),
                }
            })
            .try_collect::<Vec<_>>()
            .await?;

    for entry in mf_entries {
        let key = match entry {
            Entry::Tree(mf_id) => mf_id.blobstore_key(),
            Entry::Leaf((_, filenode_id)) => filenode_id.blobstore_key(),
        };

        check_exists(ctx, mem_blob, key).await?;
    }

    Ok(())
}

async fn find_cs_and_parents_derived_data<D: BonsaiDerived>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<(D, Vec<D>), Error> {
    let parents = repo
        .get_changeset_fetcher()
        .get_parents(ctx.clone(), cs_id)
        .await?;

    let derived = D::derive(ctx, repo, cs_id).await?;
    let parents = try_join_all(parents.into_iter().map(|p| async move {
        let derived = D::derive(ctx, repo, p).await?;
        Result::<_, Error>::Ok(derived)
    }))
    .await?;

    Ok((derived, parents))
}

async fn validate_new_manifest_entries<TreeId, LeafId>(
    ctx: &CoreContext,
    real_blobstore: Arc<dyn Blobstore>,
    mfid: TreeId,
    parent_mfids: Vec<TreeId>,
    mem_blob: &Arc<dyn Blobstore>,
    tree_blob_key: impl Fn(TreeId) -> Option<String>,
    leaf_blob_key: impl Fn(LeafId) -> Option<String>,
) -> Result<(), Error>
where
    TreeId: StoreLoadable<Arc<dyn Blobstore>> + Clone + Send + Sync + Eq + Unpin + 'static,
    <TreeId as StoreLoadable<Arc<dyn Blobstore>>>::Value:
        Manifest<TreeId = TreeId, LeafId = LeafId> + Send,
    LeafId: Clone + Send + Eq + Unpin + 'static,
{
    let mf_entries = find_intersection_of_diffs(ctx.clone(), real_blobstore, mfid, parent_mfids)
        .map_ok(|(_, entry)| entry)
        .try_collect::<Vec<_>>()
        .await?;

    for entry in mf_entries {
        let maybe_key = match entry {
            Entry::Tree(tree_id) => tree_blob_key(tree_id),
            Entry::Leaf(leaf_id) => leaf_blob_key(leaf_id),
        };

        if let Some(key) = maybe_key {
            check_exists(ctx, mem_blob, key).await?;
        }
    }

    Ok(())
}

async fn check_exists(
    ctx: &CoreContext,
    mem_blob: &Arc<dyn Blobstore>,
    key: String,
) -> Result<(), Error> {
    let maybe_value = mem_blob.get(ctx, &key).await?;

    if maybe_value.is_none() {
        return Err(anyhow!("{} not found", key));
    }

    Ok(())
}
