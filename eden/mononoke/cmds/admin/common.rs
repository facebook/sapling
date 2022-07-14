/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use cmdlib::args;
use cmdlib::helpers;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::try_join;
use futures::TryStreamExt;
use manifest::ManifestOps;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::MPath;
use mononoke_types::BonsaiChangeset;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::Timestamp;
use repo_blobstore::RepoBlobstore;
use repo_identity::RepoIdentityRef;
use serde_json::json;
use serde_json::to_string_pretty;
use slog::debug;
use slog::Logger;
use std::collections::HashMap;
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

pub fn format_bookmark_log_entry(
    json_flag: bool,
    changeset_id: String,
    reason: BookmarkUpdateReason,
    timestamp: Timestamp,
    changeset_type: &str,
    bookmark: BookmarkName,
    bundle_id: Option<u64>,
) -> String {
    let reason_str = reason.to_string();
    if json_flag {
        let answer = json!({
            "changeset_type": changeset_type,
            "changeset_id": changeset_id,
            "reason": reason_str,
            "timestamp_sec": timestamp.timestamp_seconds(),
            "bundle_id": bundle_id,
        });
        to_string_pretty(&answer).unwrap()
    } else {
        let dt: DateTime = timestamp.into();
        let dts = dt.as_chrono().format("%b %e %T %Y");
        match bundle_id {
            Some(bundle_id) => format!(
                "{} ({}) {} {} {}",
                bundle_id, bookmark, changeset_id, reason, dts
            ),
            None => format!("({}) {} {} {}", bookmark, changeset_id, reason, dts),
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
    paths: Vec<MPath>,
) -> Result<Vec<HgFileNodeId>, Error> {
    let cs = cs_id.load(&ctx, repo.blobstore()).await?;
    let root_mf_id = cs.manifestid().clone();
    let manifest_entries: HashMap<_, _> = root_mf_id
        .find_entries(ctx, repo.get_blobstore(), paths.clone())
        .try_filter_map(|(path, entry)| async move {
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

pub async fn get_source_target_repos_and_mapping<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a args::MononokeMatches<'_>,
) -> Result<(BlobRepo, BlobRepo, SqlSyncedCommitMapping), Error> {
    let config_store = matches.config_store();

    let source_repo_id = args::get_source_repo_id(config_store, matches)?;
    let target_repo_id = args::get_target_repo_id(config_store, matches)?;

    let source_repo = args::open_repo_with_repo_id(fb, &logger, source_repo_id, matches);
    let target_repo = args::open_repo_with_repo_id(fb, &logger, target_repo_id, matches);
    // TODO(stash): in reality both source and target should point to the same mapping
    // It'll be nice to verify it
    let (source_repo, target_repo) = try_join!(source_repo, target_repo)?;

    let mapping = args::open_source_sql::<SqlSyncedCommitMapping>(fb, config_store, matches)?;

    Ok((source_repo, target_repo, mapping))
}
