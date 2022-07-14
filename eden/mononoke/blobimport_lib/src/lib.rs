/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod bookmark;
mod changeset;
mod concurrency;

use std::cmp;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use ascii::AsciiString;
use futures::compat::Future01CompatExt;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::Stream;
use futures_01_ext::StreamExt as OldStreamExt;
use slog::debug;
use slog::error;
use slog::info;

use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use bonsai_globalrev_mapping::bulk_import_globalrevs;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use context::CoreContext;
use derived_data_utils::derive_data_for_csids;
use mercurial_revlog::revlog::RevIdx;
use mercurial_revlog::RevlogRepo;
use mercurial_types::HgChangesetId;
use mercurial_types::HgNodeHash;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use phases::PhasesRef;
use synced_commit_mapping::SyncedCommitMapping;
use synced_commit_mapping::SyncedCommitMappingEntry;
use synced_commit_mapping::SyncedCommitSourceRepo;

use crate::changeset::UploadChangesets;

pub use consts::HIGHEST_IMPORTED_GEN_NUM;

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
    pub origin_repo: Option<BlobRepo>,
}

impl<'a> Blobimport<'a> {
    pub async fn import(self) -> Result<Option<(RevIdx, ChangesetId)>, Error> {
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
            origin_repo,
        } = self;

        // Take refs to avoid `async move` blocks capturing data data
        // in async move blocks
        let blobrepo = &blobrepo;
        let globalrevs_store = &globalrevs_store;
        let synced_commit_mapping = &synced_commit_mapping;
        let derived_data_types = &derived_data_types;

        let repo_id = blobrepo.get_repoid();

        let revlogrepo = RevlogRepo::open(&revlogrepo_path)
            .with_context(|| format!("While opening revlog repo at {:?}", revlogrepo_path))?;
        let stale_bookmarks_fut = bookmark::read_bookmarks(&revlogrepo).compat();

        let log_step = match commits_limit {
            Some(commits_limit) => cmp::max(1, commits_limit / 10),
            None => 5000,
        };

        let chunk_size = 100;

        let is_import_from_beggining = changeset.is_none() && skip.is_none();
        let changesets = get_changeset_stream(&revlogrepo, changeset, skip, commits_limit)
            .boxed()
            .compat();

        let mut upload_changesets = UploadChangesets {
            ctx: ctx.clone(),
            blobrepo: blobrepo.clone(),
            revlogrepo: revlogrepo.clone(),
            lfs_helper,
            concurrent_changesets,
            concurrent_blobs,
            concurrent_lfs_imports,
            fixed_parent_order,
        }
        .upload(changesets, is_import_from_beggining, origin_repo)
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
                (revidx, cs.0)
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
        // PublishingOrPullDefaultPublishing here, which is the non-scratch set in Mononoke.
        let mononoke_bookmarks_fut = blobrepo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .map_ok(|(bookmark, changeset_id)| (bookmark.into_name(), changeset_id))
            .try_collect::<Vec<_>>();

        let (stale_bookmarks, mononoke_bookmarks) =
            future::try_join(stale_bookmarks_fut, mononoke_bookmarks_fut).await?;

        let mut max_rev_and_bcs_id = None;
        while let Some(chunk_result) = upload_changesets.next().await {
            let chunk = chunk_result?;
            for (rev, cs) in chunk.iter() {
                let max_rev = max_rev_and_bcs_id.map_or_else(RevIdx::zero, |(revidx, _)| revidx);
                if rev >= &max_rev {
                    max_rev_and_bcs_id = Some((*rev, cs.get_changeset_id()))
                }
            }

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
                            version_name: None,
                            source_repo: Some(SyncedCommitSourceRepo::Small),
                        })
                        .collect();
                    synced_commit_mapping
                        .add_bulk(ctx, entries)
                        .await
                        .map(|_| ())
                } else {
                    Ok(())
                }
            };

            let globalrevs_work = async {
                if has_globalrev {
                    bulk_import_globalrevs(ctx, &globalrevs_store, changesets.iter()).await
                } else {
                    Ok(())
                }
            };

            let git_mapping_work = async move {
                if populate_git_mapping {
                    let git_mapping_store = blobrepo.bonsai_git_mapping();
                    git_mapping_store
                        .bulk_import_from_bonsai(ctx, changesets)
                        .await
                } else {
                    Ok(())
                }
            };

            if !derived_data_types.is_empty() {
                info!(ctx.logger(), "Deriving data for: {:?}", derived_data_types);
            }

            let derivation_work = derive_data_for_csids(
                ctx,
                blobrepo,
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
        }

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
                    ctx.logger(),
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

        Ok(max_rev_and_bcs_id)
    }

    /// From a stream of revisions returns the latest revision that's already imported and public
    /// From this revision it's safe to restart blobimport next time.
    /// Note that we might have a situation where revision i is imported, i+1 is not and i+2 is imported.
    /// In that case this function would return i.
    pub async fn find_already_imported_revision(
        self,
    ) -> Result<Option<(RevIdx, ChangesetId)>, Error> {
        let Self {
            ctx,
            blobrepo,
            revlogrepo_path,
            changeset,
            skip,
            commits_limit,
            ..
        } = self;

        let blobrepo = &blobrepo;
        let revlogrepo = RevlogRepo::open(revlogrepo_path)?;
        let imported: Vec<_> = get_changeset_stream(&revlogrepo, changeset, skip, commits_limit)
            .chunks(100)
            .then(|chunk| async move {
                let chunk: Result<Vec<_>, Error> = chunk.into_iter().collect();
                let chunk = chunk?;
                let hg_to_bcs_ids = blobrepo
                    .get_hg_bonsai_mapping(
                        ctx.clone(),
                        chunk
                            .clone()
                            .into_iter()
                            .map(|(_, hg_cs_id)| HgChangesetId::new(hg_cs_id))
                            .collect::<Vec<_>>(),
                    )
                    .await?;

                let public = blobrepo
                    .phases()
                    .get_public(
                        ctx,
                        hg_to_bcs_ids
                            .clone()
                            .into_iter()
                            .map(|(_, bcs_id)| bcs_id)
                            .collect(),
                        false, /* ephemeral_derive */
                    )
                    .await?;

                let hg_cs_ids: HashMap<_, _> = hg_to_bcs_ids.into_iter().collect();
                let s = chunk.into_iter().map(move |(revidx, hg_cs_id)| {
                    match hg_cs_ids.get(&HgChangesetId::new(hg_cs_id)) {
                        Some(bcs_id) => {
                            if public.contains(bcs_id) {
                                Some((revidx, *bcs_id))
                            } else {
                                None
                            }
                        }
                        None => None,
                    }
                });

                Result::<_, Error>::Ok(stream::iter(s).map(Result::<_, Error>::Ok))
            })
            .try_flatten()
            .take_while(|res| match res {
                Ok(maybe_public_bcs_id) => future::ready(maybe_public_bcs_id.is_some()),
                // Take errors since they will just be propagated to the caller,
                // and if we don't take them then the caller won't know
                // about the error.
                Err(_) => future::ready(true),
            })
            .try_collect()
            .await?;

        let mut max_rev_and_bcs_id = None;
        for maybe_rev_cs in imported {
            if let Some((rev, bcs_id)) = maybe_rev_cs {
                let max_rev = max_rev_and_bcs_id.map_or_else(RevIdx::zero, |(revidx, _)| revidx);
                if rev >= max_rev {
                    max_rev_and_bcs_id = Some((rev, bcs_id))
                }
            }
        }

        Ok(max_rev_and_bcs_id)
    }
}

fn get_changeset_stream(
    revlogrepo: &RevlogRepo,
    changeset: Option<HgNodeHash>,
    skip: Option<usize>,
    commits_limit: Option<usize>,
) -> impl Stream<Item = Result<(RevIdx, HgNodeHash), Error>> + 'static {
    let changesets = match changeset {
        Some(hash) => {
            let maybe_idx = revlogrepo.get_rev_idx_for_changeset(HgChangesetId::new(hash));
            stream::once(async move {
                match maybe_idx {
                    Ok(idx) => Ok((idx, hash)),
                    Err(err) => Err(format_err!("{} not found in revlog repo: {}", hash, err)),
                }
            })
            .left_stream()
        }
        None => revlogrepo.changesets().compat().right_stream(),
    };

    let changesets = match skip {
        None => changesets.left_stream(),
        Some(skip) => changesets.skip(skip).right_stream(),
    };

    match commits_limit {
        None => changesets.left_stream(),
        Some(limit) => changesets.take(limit).right_stream(),
    }
}
