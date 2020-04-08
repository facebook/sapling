/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod bookmark;
mod changeset;
mod concurrency;

use std::cmp;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Error;
use ascii::AsciiString;
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future::{self, ready, Future},
    stream::{futures_unordered::FuturesUnordered, StreamExt, TryStreamExt},
};
use futures_ext::StreamExt as OldStreamExt;
use futures_old::{Future as OldFuture, Stream};
use slog::{debug, error, info};

use blobrepo::BlobRepo;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::{bulk_import_globalrevs, BonsaiGlobalrevMapping};
use context::CoreContext;
use derived_data_utils::derived_data_utils;
use mercurial_revlog::{revlog::RevIdx, RevlogRepo};
use mercurial_types::{HgChangesetId, HgNodeHash};
use mononoke_types::{ChangesetId, RepositoryId};
use synced_commit_mapping::{SyncedCommitMapping, SyncedCommitMappingEntry};

use crate::changeset::UploadChangesets;

fn derive_data_for_csids(
    ctx: CoreContext,
    repo: BlobRepo,
    csids: Vec<ChangesetId>,
    derived_data_types: &[String],
) -> Result<impl Future<Output = Result<(), Error>>, Error> {
    let derivations = FuturesUnordered::new();

    for data_type in derived_data_types {
        let derived_utils = derived_data_utils(repo.clone(), data_type)?;

        derivations.push(
            derived_utils
                .derive_batch(ctx.clone(), repo.clone(), csids.clone())
                .map(|_| ())
                .compat(),
        );
    }

    Ok(async move {
        derivations.try_for_each(|_| ready(Ok(()))).await?;
        Ok(())
    })
}

// What to do with bookmarks when blobimporting a repo
pub enum BookmarkImportPolicy {
    // Do not import bookmarks
    Ignore,
    // Prefix bookmark names when importing
    Prefix(AsciiString),
}

pub struct Blobimport<'a> {
    pub ctx: &'a CoreContext,
    pub blobrepo: BlobRepo,
    pub revlogrepo_path: PathBuf,
    pub changeset: Option<HgNodeHash>,
    pub skip: Option<usize>,
    pub commits_limit: Option<usize>,
    pub bookmark_import_policy: BookmarkImportPolicy,
    pub globalrevs_store: Arc<dyn BonsaiGlobalrevMapping>,
    pub synced_commit_mapping: Arc<dyn SyncedCommitMapping>,
    pub lfs_helper: Option<String>,
    pub concurrent_changesets: usize,
    pub concurrent_blobs: usize,
    pub concurrent_lfs_imports: usize,
    pub fixed_parent_order: HashMap<HgChangesetId, Vec<HgChangesetId>>,
    pub has_globalrev: bool,
    pub populate_git_mapping: bool,
    pub small_repo_id: Option<RepositoryId>,
    pub derived_data_types: Vec<String>,
}

impl<'a> Blobimport<'a> {
    pub async fn import(self) -> Result<Option<RevIdx>, Error> {
        let Self {
            ctx,
            blobrepo,
            revlogrepo_path,
            changeset,
            skip,
            commits_limit,
            bookmark_import_policy,
            globalrevs_store,
            synced_commit_mapping,
            lfs_helper,
            concurrent_changesets,
            concurrent_blobs,
            concurrent_lfs_imports,
            fixed_parent_order,
            has_globalrev,
            populate_git_mapping,
            small_repo_id,
            derived_data_types,
        } = self;

        // Take refs to avoid `async move` blocks capturing data data
        // in async move blocks
        let blobrepo = &blobrepo;
        let globalrevs_store = &globalrevs_store;
        let synced_commit_mapping = &synced_commit_mapping;
        let derived_data_types = &derived_data_types;

        let repo_id = blobrepo.get_repoid();

        let stale_bookmarks_fut = {
            let revlogrepo = RevlogRepo::open(&revlogrepo_path).expect("cannot open revlogrepo");
            bookmark::read_bookmarks(revlogrepo)
        }
        .compat();

        let revlogrepo = RevlogRepo::open(revlogrepo_path).expect("cannot open revlogrepo");

        let log_step = match commits_limit {
            Some(commits_limit) => cmp::max(1, commits_limit / 10),
            None => 5000,
        };

        let chunk_size = 100;

        let upload_changesets = UploadChangesets {
            ctx: ctx.clone(),
            blobrepo: blobrepo.clone(),
            revlogrepo: revlogrepo.clone(),
            changeset,
            skip,
            commits_limit,
            lfs_helper,
            concurrent_changesets,
            concurrent_blobs,
            concurrent_lfs_imports,
            fixed_parent_order,
        }
        .upload()
        .enumerate()
        .compat()
        .map_ok({
            move |(cs_count, (revidx, cs))| {
                debug!(
                    ctx.logger(),
                    "{} inserted: {}",
                    cs_count,
                    cs.1.get_changeset_id()
                );
                if cs_count % log_step == 0 {
                    info!(ctx.logger(), "inserted commits # {}", cs_count);
                }
                (revidx, cs.0.clone())
            }
        })
        .chunks(chunk_size)
        .map(|chunk: Vec<Result<_, _>>| chunk.into_iter().collect::<Result<Vec<_>, _>>())
        .map_err({
            move |err| {
                let msg = format!("failed to blobimport: {}", err);
                error!(ctx.logger(), "{}", msg);

                let mut err = err.deref() as &dyn StdError;
                while let Some(cause) = failure_ext::cause(err) {
                    info!(ctx.logger(), "cause: {}", cause);
                    err = cause;
                }
                info!(ctx.logger(), "root cause: {:?}", err);

                Error::msg(msg)
            }
        });

        // Blobimport does not see scratch bookmarks in Mercurial, so we use
        // PublishingOrPullDefault here, which is the non-scratch set in Mononoke.
        let mononoke_bookmarks_fut = blobrepo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .map(|(bookmark, changeset_id)| (bookmark.into_name(), changeset_id))
            .collect()
            .compat();

        let (stale_bookmarks, mononoke_bookmarks) =
            future::try_join(stale_bookmarks_fut, mononoke_bookmarks_fut).await?;

        let max_rev = upload_changesets
            .and_then({
                move |chunk: Vec<_>| async move {
                    let max_rev = chunk.iter().map(|(rev, _)| rev).max().cloned();

                    let changesets: &Vec<_> = &chunk.into_iter().map(|(_, cs)| cs).collect();

                    let synced_commit_mapping_work = async {
                        if let Some(small_repo_id) = small_repo_id {
                            let entries = changesets
                                .iter()
                                .map(|cs| SyncedCommitMappingEntry {
                                    large_repo_id: repo_id,
                                    large_bcs_id: cs.get_changeset_id(),
                                    small_repo_id,
                                    small_bcs_id: cs.get_changeset_id(),
                                })
                                .collect();
                            synced_commit_mapping
                                .add_bulk(ctx.clone(), entries)
                                .map(|_| ())
                                .compat()
                                .await
                        } else {
                            Ok(())
                        }
                    };

                    let globalrevs_work = async {
                        if has_globalrev {
                            bulk_import_globalrevs(
                                ctx.clone(),
                                repo_id,
                                globalrevs_store.clone(),
                                changesets.iter(),
                            )
                            .compat()
                            .await
                        } else {
                            Ok(())
                        }
                    };

                    let git_mapping_work = async move {
                        if populate_git_mapping {
                            let git_mapping_store = blobrepo.bonsai_git_mapping();
                            git_mapping_store
                                .bulk_import_from_bonsai(ctx.clone(), changesets)
                                .await
                        } else {
                            Ok(())
                        }
                    };

                    if !derived_data_types.is_empty() {
                        info!(ctx.logger(), "Deriving data for: {:?}", derived_data_types);
                    }

                    let derivation_work = derive_data_for_csids(
                        ctx.clone(),
                        blobrepo.clone(),
                        changesets.iter().map(|cs| cs.get_changeset_id()).collect(),
                        &derived_data_types[..],
                    )?;

                    future::try_join4(
                        synced_commit_mapping_work,
                        globalrevs_work,
                        git_mapping_work,
                        derivation_work,
                    )
                    .await?;

                    Ok(max_rev)
                }
            })
            .try_fold(None, |mut acc, rev| async move {
                if let Some(rev) = rev {
                    acc = Some(::std::cmp::max(acc.unwrap_or_else(RevIdx::zero), rev));
                }
                let res: Result<_, Error> = Ok(acc);
                res
            })
            .await?;

        info!(
            ctx.logger(),
            "finished uploading changesets, globalrevs and deriving data"
        );

        match bookmark_import_policy {
            BookmarkImportPolicy::Ignore => {
                info!(
                    ctx.logger(),
                    "since --no-bookmark was provided, bookmarks won't be imported"
                );
            }
            BookmarkImportPolicy::Prefix(prefix) => {
                bookmark::upload_bookmarks(
                    ctx.clone(),
                    &ctx.logger(),
                    revlogrepo,
                    blobrepo.clone(),
                    stale_bookmarks,
                    mononoke_bookmarks,
                    bookmark::get_bookmark_prefixer(prefix),
                )
                .compat()
                .await?
            }
        };

        Ok(max_rev)
    }
}
