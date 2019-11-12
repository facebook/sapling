/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use clap::{App, Arg, ArgMatches, SubCommand};
use failure_ext::{format_err, Error};
use fbinit::FacebookInit;
use futures::{future::IntoFuture, stream, Future, Stream};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt, StreamExt};

use blobrepo::BlobRepo;
use bookmark_renaming::{get_large_to_small_renamer, BookmarkRenamer};
use bookmarks::BookmarkName;
use cloned::cloned;
use cmdlib::{args, helpers};
use context::CoreContext;
use cross_repo_sync::{CommitSyncOutcome, CommitSyncRepos, CommitSyncer};
use futures_preview::{
    compat::Future01CompatExt,
    future::{FutureExt as PreviewFutureExt, TryFutureExt},
};
use futures_util::{
    stream::{self as new_stream, StreamExt as NewStreamExt},
    try_join, TryStreamExt,
};
use manifest::{Entry, ManifestOps};
use mercurial_types::{Changeset, HgFileNodeId, HgManifestId};
use metaconfig_types::RepoConfig;
use mononoke_types::{ChangesetId, MPath, RepositoryId};
use movers::{get_large_to_small_mover, Mover};
use slog::{debug, info, warn, Logger};
use std::collections::{HashMap, HashSet};
use synced_commit_mapping::{SqlSyncedCommitMapping, SyncedCommitMapping};

use crate::error::SubcommandError;

const MAP_SUBCOMMAND: &str = "map";
const VERIFY_WC_SUBCOMMAND: &str = "verify-wc";
const VERIFY_BOOKMARKS_SUBCOMMAND: &str = "verify-bookmarks";
const HASH_ARG: &str = "HASH";
const LARGE_REPO_HASH_ARG: &str = "LARGE_REPO_HASH";

pub fn subcommand_crossrepo(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    let source_repo_id = try_boxfuture!(args::get_source_repo_id(matches));
    let target_repo_id = try_boxfuture!(args::get_target_repo_id(matches));

    args::init_cachelib(fb, &matches);
    let source_repo = args::open_repo_with_repo_id(fb, &logger, source_repo_id, matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    // TODO(stash): in reality both source and target should point to the same mapping
    // It'll be nice to verify it
    let mapping = args::open_source_sql::<SqlSyncedCommitMapping>(&matches);

    match sub_m.subcommand() {
        (MAP_SUBCOMMAND, Some(sub_sub_m)) => {
            let hash = sub_sub_m.value_of(HASH_ARG).unwrap().to_owned();
            source_repo
                .join(mapping)
                .from_err()
                .and_then(move |(source_repo, mapping)| {
                    subcommand_map(ctx, source_repo, target_repo_id, mapping, hash)
                })
                .boxify()
        }
        (VERIFY_WC_SUBCOMMAND, Some(sub_sub_m)) => {
            let (_, source_repo_config) =
                try_boxfuture!(args::get_config_by_repoid(matches, source_repo_id));
            let target_repo_fut =
                args::open_repo_with_repo_id(fb, &logger, target_repo_id, matches);
            let hash = sub_sub_m.value_of(LARGE_REPO_HASH_ARG).unwrap().to_owned();

            source_repo
                .join3(target_repo_fut, mapping)
                .from_err()
                .and_then(move |(source_repo, target_repo, mapping)| {
                    subcommand_verify_wc(
                        ctx,
                        source_repo,
                        source_repo_config,
                        target_repo,
                        mapping,
                        hash,
                    )
                    .boxed()
                    .compat()
                })
                .boxify()
        }
        (VERIFY_BOOKMARKS_SUBCOMMAND, Some(_sub_sub_m)) => {
            let (_, source_repo_config) =
                try_boxfuture!(args::get_config_by_repoid(matches, source_repo_id));
            let target_repo_fut =
                args::open_repo_with_repo_id(fb, &logger, target_repo_id, matches);

            source_repo
                .join3(target_repo_fut, mapping)
                .from_err()
                .and_then(move |(source_repo, target_repo, mapping)| {
                    subcommand_verify_bookmarks(
                        ctx,
                        source_repo,
                        source_repo_config,
                        target_repo,
                        mapping,
                    )
                    .boxed()
                    .compat()
                })
                .boxify()
        }
        _ => Err(SubcommandError::InvalidArgs).into_future().boxify(),
    }
}

fn subcommand_map(
    ctx: CoreContext,
    source_repo: BlobRepo,
    target_repo_id: RepositoryId,
    mapping: SqlSyncedCommitMapping,
    hash: String,
) -> BoxFuture<(), SubcommandError> {
    let source_repo_id = source_repo.get_repoid();
    let source_hash = helpers::csid_resolve(ctx.clone(), source_repo, hash);
    source_hash
        .and_then(move |source_hash| {
            mapping
                .get(ctx, source_repo_id, source_hash, target_repo_id)
                .and_then(move |mapped| {
                    match mapped {
                        None => println!(
                            "Hash {} not currently remapped (could be present in target as-is)",
                            source_hash
                        ),
                        Some(target_hash) => {
                            println!("Hash {} maps to {}", source_hash, target_hash)
                        }
                    };
                    Ok(())
                })
        })
        .from_err()
        .boxify()
}

async fn subcommand_verify_wc(
    ctx: CoreContext,
    source_repo: BlobRepo,
    source_repo_config: RepoConfig,
    target_repo: BlobRepo,
    mapping: SqlSyncedCommitMapping,
    large_repo_hash: String,
) -> Result<(), SubcommandError> {
    let commit_sync_repos =
        get_large_to_small_commit_sync_repos(source_repo, target_repo, &source_repo_config)?;
    let commit_syncer = CommitSyncer::new(mapping, commit_sync_repos);

    let large_repo = commit_syncer.get_large_repo();
    let small_repo = commit_syncer.get_small_repo();

    let large_hash = helpers::csid_resolve(ctx.clone(), large_repo.clone(), large_repo_hash)
        .compat()
        .await?;

    let small_hash = get_synced_commit(ctx.clone(), &commit_syncer, large_hash).await?;
    info!(ctx.logger(), "small repo cs id: {}", small_hash);

    let moved_large_repo_entries = async {
        let large_root_mf_id =
            fetch_root_mf_id(ctx.clone(), large_repo.clone(), large_hash.clone()).await?;

        let large_repo_entries =
            list_all_filenode_ids(ctx.clone(), large_repo.clone(), large_root_mf_id)
                .compat()
                .await?;

        if large_hash == small_hash {
            // No need to move any paths, because this commit was preserved as is
            Ok(large_repo_entries)
        } else {
            move_all_paths(large_repo_entries, commit_syncer.get_mover())
        }
    };

    let small_repo_entries = async {
        let small_root_mf_id =
            fetch_root_mf_id(ctx.clone(), small_repo.clone(), small_hash.clone()).await?;

        list_all_filenode_ids(ctx.clone(), small_repo.clone(), small_root_mf_id)
            .compat()
            .await
    };

    let (moved_large_repo_entries, small_repo_entries) =
        try_join!(moved_large_repo_entries, small_repo_entries)?;

    compare_contents(
        ctx.clone(),
        (large_repo.clone(), &moved_large_repo_entries),
        (small_repo.clone(), &small_repo_entries),
        large_hash,
    )
    .await?;

    for (path, _) in small_repo_entries {
        if moved_large_repo_entries.get(&path).is_none() {
            return Err(
                format_err!("{:?} is present in small repo, but not in large", path).into(),
            );
        }
    }

    info!(ctx.logger(), "all is well!");
    Ok(())
}

async fn subcommand_verify_bookmarks(
    ctx: CoreContext,
    source_repo: BlobRepo,
    source_repo_config: RepoConfig,
    target_repo: BlobRepo,
    mapping: SqlSyncedCommitMapping,
) -> Result<(), SubcommandError> {
    let commit_sync_repos = get_large_to_small_commit_sync_repos(
        source_repo.clone(),
        target_repo.clone(),
        &source_repo_config,
    )?;
    let commit_syncer = CommitSyncer::new(mapping, commit_sync_repos);
    let large_repo = commit_syncer.get_large_repo();
    let small_repo = commit_syncer.get_small_repo();

    let small_bookmarks = small_repo
        .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
        .map(|(bookmark, cs_id)| (bookmark.name().clone(), cs_id))
        .collect_to::<HashMap<_, _>>()
        .compat()
        .await?;

    let large_repo_bookmarks = large_repo
        .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
        .map(|(bookmark, cs_id)| (bookmark.name().clone(), cs_id))
        .collect()
        .compat()
        .await?;

    let large_repo_bookmarks = rename_large_repo_bookmarks(
        ctx.clone(),
        &commit_syncer,
        commit_syncer.get_bookmark_renamer(),
        large_repo_bookmarks,
    )
    .await?;

    let mut failed = false;
    for (small_book, small_value) in &small_bookmarks {
        let maybe_large_value = large_repo_bookmarks.get(small_book);
        if maybe_large_value != Some(small_value) {
            failed = true;
            warn!(
                ctx.logger(),
                "inconsistent value of {}: small repo {}, large repo {:?}",
                small_book,
                small_value,
                maybe_large_value,
            );
        }
    }

    for large_book in large_repo_bookmarks.keys() {
        if !small_bookmarks.contains_key(large_book) {
            failed = true;
            warn!(
                ctx.logger(),
                "large repo bookmark {} not found in small repo", large_book,
            );
        }
    }

    if failed {
        Err(format_err!("inconsistent bookmarks").into())
    } else {
        Ok(())
    }
}

async fn rename_large_repo_bookmarks<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<M>,
    bookmark_renamer: &BookmarkRenamer,
    large_repo_bookmarks: impl IntoIterator<Item = (BookmarkName, ChangesetId)>,
) -> Result<HashMap<BookmarkName, ChangesetId>, Error> {
    let mut renamed_large_repo_bookmarks = vec![];
    for (bookmark, cs_id) in large_repo_bookmarks {
        if let Some(bookmark) = bookmark_renamer(&bookmark) {
            let maybe_sync_outcome = commit_syncer
                .get_commit_sync_outcome(ctx.clone(), cs_id)
                .map(move |maybe_sync_outcome| {
                    let maybe_sync_outcome = maybe_sync_outcome?;
                    use CommitSyncOutcome::*;
                    let remapped_cs_id = match maybe_sync_outcome {
                        Some(Preserved) => cs_id,
                        Some(RewrittenAs(cs_id)) | Some(EquivalentWorkingCopyAncestor(cs_id)) => {
                            cs_id
                        }
                        Some(NotSyncCandidate) => {
                            return Err(format_err!("{} is not a sync candidate", cs_id));
                        }
                        None => {
                            return Err(format_err!("{} is not remapped for {}", cs_id, bookmark));
                        }
                    };
                    Ok((bookmark, remapped_cs_id))
                })
                .boxed();
            renamed_large_repo_bookmarks.push(maybe_sync_outcome);
        }
    }

    let large_repo_bookmarks = new_stream::iter(renamed_large_repo_bookmarks)
        .buffer_unordered(100)
        .try_collect::<HashMap<_, _>>()
        .await?;

    Ok(large_repo_bookmarks)
}

fn move_all_paths(
    filenodes: HashMap<Option<MPath>, HgFileNodeId>,
    mover: &Mover,
) -> Result<HashMap<Option<MPath>, HgFileNodeId>, Error> {
    let mut moved_large_repo_entries = HashMap::new();
    for (path, filenode_id) in filenodes {
        if let Some(path) = path {
            let moved_path = mover(&path)?;
            if let Some(moved_path) = moved_path {
                moved_large_repo_entries.insert(Some(moved_path), filenode_id);
            }
        }
    }

    Ok(moved_large_repo_entries)
}

async fn get_synced_commit<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<M>,
    hash: ChangesetId,
) -> Result<ChangesetId, Error> {
    let maybe_sync_outcome = commit_syncer
        .get_commit_sync_outcome(ctx.clone(), hash)
        .await?;
    let sync_outcome = maybe_sync_outcome.ok_or(format_err!(
        "No sync outcome for {} in {:?}",
        hash,
        commit_syncer
    ))?;

    use CommitSyncOutcome::*;
    match sync_outcome {
        NotSyncCandidate => {
            return Err(format_err!("{} does not remap in small repo", hash).into());
        }
        RewrittenAs(cs_id) | EquivalentWorkingCopyAncestor(cs_id) => Ok(cs_id),
        Preserved => Ok(hash),
    }
}

async fn compare_contents(
    ctx: CoreContext,
    (large_repo, large_filenodes): (BlobRepo, &HashMap<Option<MPath>, HgFileNodeId>),
    (small_repo, small_filenodes): (BlobRepo, &HashMap<Option<MPath>, HgFileNodeId>),
    large_hash: ChangesetId,
) -> Result<(), Error> {
    let mut different_filenodes = HashSet::new();
    for (path, left_filenode_id) in large_filenodes {
        let maybe_right_filenode_id = small_filenodes.get(&path);
        if maybe_right_filenode_id != Some(&left_filenode_id) {
            match maybe_right_filenode_id {
                Some(right_filenode_id) => {
                    different_filenodes.insert((
                        path.clone(),
                        *left_filenode_id,
                        *right_filenode_id,
                    ));
                }
                None => {
                    return Err(format_err!(
                        "{:?} exists in large repo but not in small repo",
                        path
                    ));
                }
            }
        }
    }

    info!(
        ctx.logger(),
        "found {} filenodes that are different, checking content...",
        different_filenodes.len(),
    );

    let fetched_content_ids = stream::iter_ok(different_filenodes)
        .map({
            cloned!(ctx, large_repo, small_repo);
            move |(path, left_filenode_id, right_filenode_id)| {
                debug!(
                    ctx.logger(),
                    "checking content for different filenodes: {} vs {}",
                    left_filenode_id,
                    right_filenode_id,
                );
                let f1 = large_repo.get_file_content_id(ctx.clone(), left_filenode_id);
                let f2 = small_repo.get_file_content_id(ctx.clone(), right_filenode_id);

                f1.join(f2).map(move |(c1, c2)| (path, c1, c2))
            }
        })
        .buffered(1000)
        .collect()
        .compat()
        .await?;

    for (path, small_content_id, large_content_id) in fetched_content_ids {
        if small_content_id != large_content_id {
            return Err(format_err!(
                "different contents for {:?}: {} vs {}, {}",
                path,
                small_content_id,
                large_content_id,
                large_hash,
            ));
        }
    }

    Ok(())
}

fn list_all_filenode_ids(
    ctx: CoreContext,
    repo: BlobRepo,
    mf_id: HgManifestId,
) -> BoxFuture<HashMap<Option<MPath>, HgFileNodeId>, Error> {
    info!(
        ctx.logger(),
        "fetching filenode ids for {}",
        repo.get_repoid()
    );
    mf_id
        .list_all_entries(ctx.clone(), repo.get_blobstore())
        .filter_map(move |(path, entry)| match entry {
            Entry::Leaf((_, filenode_id)) => Some((path, filenode_id)),
            Entry::Tree(_) => None,
        })
        .collect_to::<HashMap<_, _>>()
        .inspect(move |res| {
            debug!(
                ctx.logger(),
                "fetched {} filenode ids for {}",
                res.len(),
                repo.get_repoid()
            );
        })
        .boxify()
}

async fn fetch_root_mf_id(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
) -> Result<HgManifestId, Error> {
    let hg_cs_id = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
        .compat()
        .await?;
    let changeset = repo
        .get_changeset_by_changesetid(ctx.clone(), hg_cs_id)
        .compat()
        .await?;
    Ok(changeset.manifestid())
}

pub fn build_subcommand(name: &str) -> App {
    let map_subcommand = SubCommand::with_name(MAP_SUBCOMMAND)
        .about("Check cross-repo commit mapping")
        .arg(
            Arg::with_name(HASH_ARG)
                .required(true)
                .help("bonsai changeset hash to map"),
        );

    let verify_wc_subcommand = SubCommand::with_name(VERIFY_WC_SUBCOMMAND)
        .about("verify working copy")
        .arg(
            Arg::with_name(LARGE_REPO_HASH_ARG)
                .required(true)
                .help("bonsai changeset hash from large repo to verify"),
        );

    let verify_bookmarks_subcommand = SubCommand::with_name(VERIFY_BOOKMARKS_SUBCOMMAND).about(
        "verify that bookmarks are the same in small and large repo (subject to bookmark renames)",
    );

    SubCommand::with_name(name)
        .subcommand(map_subcommand)
        .subcommand(verify_wc_subcommand)
        .subcommand(verify_bookmarks_subcommand)
}

fn get_large_to_small_commit_sync_repos(
    source_repo: BlobRepo,
    target_repo: BlobRepo,
    repo_config: &RepoConfig,
) -> Result<CommitSyncRepos, Error> {
    repo_config
        .commit_sync_config
        .as_ref()
        .ok_or_else(|| format_err!("missing CommitSyncMapping config"))
        .and_then(|commit_sync_config| {
            let (large_repo, small_repo) = if commit_sync_config.large_repo_id
                == source_repo.get_repoid()
                && commit_sync_config
                    .small_repos
                    .contains_key(&target_repo.get_repoid())
            {
                (source_repo, target_repo)
            } else if commit_sync_config.large_repo_id == target_repo.get_repoid()
                && commit_sync_config
                    .small_repos
                    .contains_key(&source_repo.get_repoid())
            {
                (target_repo, source_repo)
            } else {
                return Err(format_err!(
                    "CommitSyncMapping incompatible with source repo {:?} and target repo {:?}",
                    source_repo.get_repoid(),
                    target_repo.get_repoid()
                ));
            };

            let bookmark_renamer =
                get_large_to_small_renamer(commit_sync_config, small_repo.get_repoid())?;
            get_large_to_small_mover(&commit_sync_config, small_repo.get_repoid()).map(
                move |mover| {
                    (CommitSyncRepos::LargeToSmall {
                        large_repo,
                        small_repo,
                        mover,
                        bookmark_renamer,
                    })
                },
            )
        })
}
