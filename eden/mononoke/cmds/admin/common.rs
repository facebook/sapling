/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::format_err;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bookmarks::BookmarksRef;
use cmdlib::args;
use cmdlib::helpers;
use context::CoreContext;
use facet::AsyncBuildable;
use fbinit::FacebookInit;
use futures::try_join;
use futures::TryStreamExt;
use manifest::ManifestOps;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::NonRootMPath;
use mononoke_types::BonsaiChangeset;
use mononoke_types::FileChange;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_factory::RepoFactoryBuilder;
use repo_identity::RepoIdentityRef;
use slog::debug;
use slog::Logger;
use synced_commit_mapping::SqlSyncedCommitMapping;

pub async fn fetch_bonsai_changeset(
    ctx: CoreContext,
    rev: &str,
    repo: impl RepoIdentityRef + BonsaiHgMappingRef + BookmarksRef,
    blobstore: &RepoBlobstore,
) -> Result<BonsaiChangeset, Error> {
    let csid = helpers::csid_resolve(&ctx, repo, rev.to_string()).await?;
    let cs = csid.load(&ctx, blobstore).await?;
    Ok(cs)
}

pub fn print_bonsai_changeset(bcs: &BonsaiChangeset) {
    println!(
        "BonsaiChangesetId: {} \n\
                     Author: {} \n\
                     Message: {} \n\
                     FileChanges:",
        bcs.get_changeset_id(),
        bcs.author(),
        bcs.message().lines().next().unwrap_or("")
    );

    for (path, file_change) in bcs.file_changes() {
        match file_change {
            FileChange::Change(file_change) => match file_change.copy_from() {
                Some(_) => println!("\t COPY/MOVE: {} {}", path, file_change.content_id()),
                None => println!("\t ADDED/MODIFIED: {} {}", path, file_change.content_id()),
            },
            FileChange::Deletion => println!("\t REMOVED: {}", path),
            FileChange::UntrackedChange(fc) => {
                println!("\t UNTRACKED ADD/MODIFY: {} {}", path, fc.content_id())
            }
            FileChange::UntrackedDeletion => println!("\t MISSING: {}", path),
        }
    }
}

// The function retrieves the HgFileNodeId of a file, based on path and rev.
// If the path is not valid an error is expected.
pub async fn get_file_nodes(
    ctx: CoreContext,
    logger: Logger,
    repo: &BlobRepo,
    cs_id: HgChangesetId,
    paths: Vec<NonRootMPath>,
) -> Result<Vec<HgFileNodeId>, Error> {
    let cs = cs_id.load(&ctx, repo.repo_blobstore()).await?;
    let root_mf_id = cs.manifestid().clone();
    let manifest_entries: HashMap<_, _> = root_mf_id
        .find_entries(ctx, repo.repo_blobstore().clone(), paths.clone())
        .try_filter_map(|(path, entry)| async move {
            let path = path.into_optional_non_root_path();
            let result =
                path.and_then(move |path| entry.into_leaf().map(move |leaf| (path, leaf.1)));
            Ok(result)
        })
        .try_collect()
        .await?;

    let mut existing_hg_nodes = Vec::new();
    let mut non_existing_paths = Vec::new();
    for path in paths.iter() {
        match manifest_entries.get(path) {
            Some(hg_node) => existing_hg_nodes.push(*hg_node),
            None => non_existing_paths.push(path.clone()),
        };
    }
    match non_existing_paths.len() {
        0 => {
            debug!(logger, "All the file paths are valid");
            Ok(existing_hg_nodes)
        }
        _ => Err(format_err!(
            "failed to identify the files associated with the file paths {:?}",
            non_existing_paths
        )),
    }
}

pub async fn get_source_target_repos_and_mapping<'a, R>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a args::MononokeMatches<'_>,
) -> Result<(R, R, SqlSyncedCommitMapping), Error>
where
    for<'builder> R: AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    let config_store = matches.config_store();

    let source_repo_id =
        args::not_shardmanager_compatible::get_source_repo_id(config_store, matches)?;
    let target_repo_id =
        args::not_shardmanager_compatible::get_target_repo_id(config_store, matches)?;

    let source_repo = args::open_repo_with_repo_id(fb, &logger, source_repo_id, matches);
    let target_repo = args::open_repo_with_repo_id(fb, &logger, target_repo_id, matches);
    // TODO(stash): in reality both source and target should point to the same mapping
    // It'll be nice to verify it
    let (source_repo, target_repo) = try_join!(source_repo, target_repo)?;

    let mapping = args::not_shardmanager_compatible::open_source_sql::<SqlSyncedCommitMapping>(
        fb,
        config_store,
        matches,
    )
    .await?;

    Ok((source_repo, target_repo, mapping))
}
