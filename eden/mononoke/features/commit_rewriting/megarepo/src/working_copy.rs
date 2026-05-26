/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobstore::Loadable;
use content_manifest_derivation::RootContentManifestId;
use context::CoreContext;
use derivation_queue_thrift::DerivationPriority;
use fsnodes::RootFsnodeId;
use futures::TryStreamExt;
use futures::future::try_join;
use manifest::Diff;
use manifest::ManifestOps;
use mercurial_derivation::DeriveHgChangeset;
use mercurial_types::NonRootMPath;
use mononoke_types::ChangesetId;
use mononoke_types::content_manifest::compat;
use tracing::info;
use unodes::RootUnodeManifestId;

use crate::Repo;

async fn derive_compat_manifest_ids(
    ctx: &CoreContext,
    repo: &impl Repo,
    bcs_id: ChangesetId,
    base_cs_id: ChangesetId,
) -> Result<(compat::ContentManifestId, compat::ContentManifestId), Error> {
    let use_content_manifests = justknobs::eval(
        "scm/mononoke:derived_data_use_content_manifests",
        None,
        Some(repo.repo_identity().name()),
    )?;

    if use_content_manifests {
        let id = repo.repo_derived_data().derive::<RootContentManifestId>(
            ctx,
            bcs_id,
            DerivationPriority::LOW,
        );
        let base_id = repo.repo_derived_data().derive::<RootContentManifestId>(
            ctx,
            base_cs_id,
            DerivationPriority::LOW,
        );
        let (id, base_id) = try_join(id, base_id).await?;
        Ok((
            id.into_content_manifest_id().into(),
            base_id.into_content_manifest_id().into(),
        ))
    } else {
        let id =
            repo.repo_derived_data()
                .derive::<RootFsnodeId>(ctx, bcs_id, DerivationPriority::LOW);
        let base_id = repo.repo_derived_data().derive::<RootFsnodeId>(
            ctx,
            base_cs_id,
            DerivationPriority::LOW,
        );
        let (id, base_id) = try_join(id, base_id).await?;
        Ok((id.into_fsnode_id().into(), base_id.into_fsnode_id().into()))
    }
}

pub async fn get_working_copy_paths(
    ctx: &CoreContext,
    repo: &impl Repo,
    bcs_id: ChangesetId,
) -> Result<Vec<NonRootMPath>, Error> {
    let hg_cs_id = repo.derive_hg_changeset(ctx, bcs_id).await?;

    let hg_cs = hg_cs_id.load(ctx, repo.repo_blobstore()).await?;
    info!("Getting working copy contents");
    let mut paths: Vec<_> = hg_cs
        .manifestid()
        .list_leaf_entries(ctx.clone(), repo.repo_blobstore().clone())
        .map_ok(|(path, (_file_type, _filenode_id))| path)
        .try_collect()
        .await?;
    paths.sort();
    info!("Done getting working copy contents");
    Ok(paths)
}

pub async fn get_changed_content_working_copy_paths(
    ctx: &CoreContext,
    repo: &impl Repo,
    bcs_id: ChangesetId,
    base_cs_id: ChangesetId,
) -> Result<Vec<NonRootMPath>, Error> {
    let (root_manifest_id, base_root_manifest_id) =
        derive_compat_manifest_ids(ctx, repo, bcs_id, base_cs_id).await?;

    let mut paths = base_root_manifest_id
        .diff(ctx.clone(), repo.repo_blobstore().clone(), root_manifest_id)
        .try_filter_map(|diff| async move {
            use Diff::*;
            let maybe_path =
                match diff {
                    Added(maybe_path, entry) => entry
                        .into_leaf()
                        .and(Option::<NonRootMPath>::from(maybe_path)),
                    Removed(_maybe_path, _entry) => None,
                    Changed(maybe_path, _old_entry, new_entry) => new_entry
                        .into_leaf()
                        .and(Option::<NonRootMPath>::from(maybe_path)),
                };

            Ok(maybe_path)
        })
        .try_collect::<Vec<_>>()
        .await?;

    paths.sort();
    Ok(paths)
}

pub async fn get_colliding_paths_between_commits(
    ctx: &CoreContext,
    repo: &impl Repo,
    bcs_id: ChangesetId,
    base_cs_id: ChangesetId,
) -> Result<Vec<NonRootMPath>, Error> {
    let (root_manifest_id, base_root_manifest_id) =
        derive_compat_manifest_ids(ctx, repo, bcs_id, base_cs_id).await?;

    let mut paths = base_root_manifest_id
        .diff(ctx.clone(), repo.repo_blobstore().clone(), root_manifest_id)
        .try_filter_map(|diff| async move {
            use Diff::*;
            let maybe_path =
                match diff {
                    Added(_maybe_path, _entry) => None,
                    Removed(_maybe_path, _entry) => None,
                    Changed(maybe_path, _old_entry, new_entry) => new_entry
                        .into_leaf()
                        .and(Option::<NonRootMPath>::from(maybe_path)),
                };

            Ok(maybe_path)
        })
        .try_collect::<Vec<_>>()
        .await?;

    paths.sort();
    Ok(paths)
}

pub async fn get_changed_working_copy_paths(
    ctx: &CoreContext,
    repo: &impl Repo,
    bcs_id: ChangesetId,
    base_cs_id: ChangesetId,
) -> Result<Vec<NonRootMPath>, Error> {
    let unode_id = repo.repo_derived_data().derive::<RootUnodeManifestId>(
        ctx,
        bcs_id,
        DerivationPriority::LOW,
    );
    let base_unode_id = repo.repo_derived_data().derive::<RootUnodeManifestId>(
        ctx,
        base_cs_id,
        DerivationPriority::LOW,
    );

    let (unode_id, base_unode_id) = try_join(unode_id, base_unode_id).await?;

    let mut paths = base_unode_id
        .manifest_unode_id()
        .diff(
            ctx.clone(),
            repo.repo_blobstore().clone(),
            *unode_id.manifest_unode_id(),
        )
        .try_filter_map(|diff| async move {
            use Diff::*;
            let maybe_path =
                match diff {
                    Added(maybe_path, entry) => entry
                        .into_leaf()
                        .and(Option::<NonRootMPath>::from(maybe_path)),
                    Removed(_maybe_path, _entry) => None,
                    Changed(maybe_path, _old_entry, new_entry) => new_entry
                        .into_leaf()
                        .and(Option::<NonRootMPath>::from(maybe_path)),
                };

            Ok(maybe_path)
        })
        .try_collect::<Vec<_>>()
        .await?;

    paths.sort();
    Ok(paths)
}

pub async fn get_working_copy_paths_by_prefixes(
    ctx: &CoreContext,
    repo: &impl Repo,
    bcs_id: ChangesetId,
    prefixes: impl IntoIterator<Item = NonRootMPath>,
) -> Result<Vec<NonRootMPath>, Error> {
    let unode_id = repo
        .repo_derived_data()
        .derive::<RootUnodeManifestId>(ctx, bcs_id, DerivationPriority::LOW)
        .await?;
    let mut paths = unode_id
        .manifest_unode_id()
        .list_leaf_entries_under(ctx.clone(), repo.repo_blobstore().clone(), prefixes)
        .map_ok(|(mpath, _)| mpath)
        .try_collect::<Vec<NonRootMPath>>()
        .await?;
    paths.sort();
    Ok(paths)
}
