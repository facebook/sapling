/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::bundle_generator::{BookmarkChange, FilenodeVerifier};
use crate::errors::{
    ErrorKind::{BookmarkMismatchInBundleCombining, ReplayDataMissing, UnexpectedBookmarkMove},
    PipelineError,
};
use crate::{bind_sync_err, CombinedBookmarkUpdateLogEntry};
use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use bookmarks::{BookmarkName, BookmarkUpdateLogEntry, BookmarkUpdateReason, RawBundleReplayData};
use cloned::cloned;
use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    future::{self, try_join, try_join_all, BoxFuture, FutureExt, TryFutureExt},
    Future,
};
use getbundle_response::SessionLfsParams;
use itertools::Itertools;
use mercurial_bundle_replay_data::BundleReplayData;
use mercurial_types::HgChangesetId;
use metaconfig_types::LfsParams;
use mononoke_hg_sync_job_helper_lib::{
    retry, save_bundle_to_temp_file, save_bytes_to_temp_file, write_to_named_temp_file,
};
use mononoke_types::{datetime::Timestamp, ChangesetId};
use reachabilityindex::LeastCommonAncestorsHint;
use regex::Regex;
use skiplist::fetch_skiplist_index;
use slog::info;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::sync::Arc;
use tempfile::NamedTempFile;

#[derive(Clone)]
pub struct PreparedBookmarkUpdateLogEntry {
    pub log_entry: BookmarkUpdateLogEntry,
    pub bundle_file: Arc<NamedTempFile>,
    pub timestamps_file: Arc<NamedTempFile>,
    pub cs_id: Option<(ChangesetId, HgChangesetId)>,
}

pub struct BundlePreparer {
    repo: BlobRepo,
    base_retry_delay_ms: u64,
    retry_num: usize,
    ty: BundleType,
}

#[derive(Clone)]
enum PrepareType {
    Generate {
        lca_hint: Arc<dyn LeastCommonAncestorsHint>,
        lfs_params: SessionLfsParams,
        filenode_verifier: FilenodeVerifier,
    },
    UseExisting {
        bundle_replay_data: RawBundleReplayData,
    },
}

#[derive(Clone)]
enum BundleType {
    // Use a bundle that was saved on Mononoke during the push
    UseExisting,
    // Generate a new bundle
    GenerateNew {
        lca_hint: Arc<dyn LeastCommonAncestorsHint>,
        lfs_params: LfsParams,
        filenode_verifier: FilenodeVerifier,
        bookmark_regex_force_lfs: Option<Regex>,
    },
}

impl BundlePreparer {
    pub async fn new_use_existing(
        repo: BlobRepo,
        base_retry_delay_ms: u64,
        retry_num: usize,
    ) -> Result<BundlePreparer, Error> {
        Ok(BundlePreparer {
            repo,
            base_retry_delay_ms,
            retry_num,
            ty: BundleType::UseExisting,
        })
    }

    pub async fn new_generate_bundles(
        ctx: CoreContext,
        repo: BlobRepo,
        base_retry_delay_ms: u64,
        retry_num: usize,
        maybe_skiplist_blobstore_key: Option<String>,
        lfs_params: LfsParams,
        filenode_verifier: FilenodeVerifier,
        bookmark_regex_force_lfs: Option<Regex>,
    ) -> Result<BundlePreparer, Error> {
        let blobstore = repo.get_blobstore().boxed();
        let skiplist =
            fetch_skiplist_index(&ctx, &maybe_skiplist_blobstore_key, &blobstore).await?;

        let lca_hint: Arc<dyn LeastCommonAncestorsHint> = skiplist;
        Ok(BundlePreparer {
            repo,
            base_retry_delay_ms,
            retry_num,
            ty: BundleType::GenerateNew {
                lca_hint,
                lfs_params,
                filenode_verifier,
                bookmark_regex_force_lfs,
            },
        })
    }

    pub fn prepare_bundles(
        &self,
        ctx: &CoreContext,
        entries: Vec<BookmarkUpdateLogEntry>,
        overlay: &mut crate::BookmarkOverlay,
    ) -> impl Future<Output = Result<Vec<CombinedBookmarkUpdateLogEntry>, PipelineError>> {
        use BookmarkUpdateReason::*;

        for log_entry in &entries {
            match log_entry.reason {
                Pushrebase | Backsyncer | ManualMove | ApiRequest | XRepoSync | Push => {}
                Blobimport | TestMove => {
                    let err: Error = UnexpectedBookmarkMove(format!("{}", log_entry.reason)).into();
                    let err = bind_sync_err(&[log_entry.clone()], err);

                    return future::ready(Err(err)).boxed();
                }
            };
        }

        let mut futs = vec![];

        match &self.ty {
            BundleType::GenerateNew {
                lca_hint,
                lfs_params,
                filenode_verifier,
                bookmark_regex_force_lfs,
            } => {
                for log_entry in entries {
                    let prepare_type = PrepareType::Generate {
                        lca_hint: lca_hint.clone(),
                        lfs_params: get_session_lfs_params(
                            &ctx,
                            &log_entry.bookmark_name,
                            lfs_params.clone(),
                            &bookmark_regex_force_lfs,
                        ),
                        filenode_verifier: filenode_verifier.clone(),
                    };

                    let bookmark_name = log_entry.bookmark_name.clone();
                    let from_cs_id = log_entry.from_changeset_id;
                    let to_cs_id = log_entry.to_changeset_id;
                    let entries = vec![log_entry];
                    let f = self.prepare_single_bundle(
                        ctx.clone(),
                        entries.clone(),
                        overlay,
                        prepare_type,
                        bookmark_name,
                        from_cs_id,
                        to_cs_id,
                    );
                    futs.push((f, entries));
                }
            }
            BundleType::UseExisting => {
                for log_entry in entries {
                    let prepare_type = match &log_entry.bundle_replay_data {
                        Some(bundle_replay_data) => PrepareType::UseExisting {
                            bundle_replay_data: bundle_replay_data.clone(),
                        },
                        None => {
                            let err: Error = ReplayDataMissing { id: log_entry.id }.into();
                            return future::ready(Err(bind_sync_err(&[log_entry], err))).boxed();
                        }
                    };

                    let bookmark_name = log_entry.bookmark_name.clone();
                    let from_cs_id = log_entry.from_changeset_id;
                    let to_cs_id = log_entry.to_changeset_id;
                    let entries = vec![log_entry];
                    let f = self.prepare_single_bundle(
                        ctx.clone(),
                        entries.clone(),
                        overlay,
                        prepare_type.clone(),
                        bookmark_name,
                        from_cs_id,
                        to_cs_id,
                    );
                    futs.push((f, entries));
                }
            }
        }

        let futs = futs
            .into_iter()
            .map(|(f, entries)| async move {
                let f = tokio::spawn(f);
                let res = f.map_err(Error::from).await;
                let res = match res {
                    Ok(Ok(res)) => Ok(res),
                    Ok(Err(err)) => Err(err),
                    Err(err) => Err(err),
                };
                res.map_err(|err| bind_sync_err(&entries, err))
            })
            .collect::<Vec<_>>();
        async move { try_join_all(futs).await }.boxed()
    }

    // Prepares a bundle that might be a result of combining a few BookmarkUpdateLogEntry.
    // Note that these entries should all move the same bookmark.
    fn prepare_single_bundle(
        &self,
        ctx: CoreContext,
        entries: Vec<BookmarkUpdateLogEntry>,
        overlay: &mut crate::BookmarkOverlay,
        prepare_type: PrepareType,
        bookmark_name: BookmarkName,
        from_cs_id: Option<ChangesetId>,
        to_cs_id: Option<ChangesetId>,
    ) -> BoxFuture<'static, Result<CombinedBookmarkUpdateLogEntry, Error>> {
        cloned!(self.repo);

        let book_values = overlay.get_bookmark_values();
        overlay.update(bookmark_name.clone(), to_cs_id.clone());

        let base_retry_delay_ms = self.base_retry_delay_ms;
        let retry_num = self.retry_num;
        async move {
            let entry_ids = entries
                .iter()
                .map(|log_entry| log_entry.id)
                .collect::<Vec<_>>();
            info!(ctx.logger(), "preparing log entry ids #{:?} ...", entry_ids);
            // Check that all entries modify bookmark_name
            for entry in &entries {
                if entry.bookmark_name != bookmark_name {
                    return Err(BookmarkMismatchInBundleCombining {
                        ids: entry_ids,
                        entry_id: entry.id,
                        entry_bookmark_name: entry.bookmark_name.clone(),
                        bundle_bookmark_name: bookmark_name,
                    }
                    .into());
                }
            }

            let bookmark_change = BookmarkChange::new(from_cs_id, to_cs_id)?;
            let bundle_timestamps = retry(
                &ctx.logger(),
                {
                    |_| {
                        Self::try_prepare_bundle_timestamps_file(
                            &ctx,
                            &repo,
                            prepare_type.clone(),
                            &book_values,
                            &bookmark_change,
                            &bookmark_name,
                        )
                    }
                },
                base_retry_delay_ms,
                retry_num,
            )
            .map_ok(|(res, _)| res);

            let cs_id = async {
                match to_cs_id {
                    Some(to_changeset_id) => {
                        let hg_cs_id = repo
                            .get_hg_from_bonsai_changeset(ctx.clone(), to_changeset_id)
                            .compat()
                            .await?;
                        Ok(Some((to_changeset_id, hg_cs_id)))
                    }
                    None => Ok(None),
                }
            };

            let ((bundle_file, timestamps_file), cs_id) =
                try_join(bundle_timestamps, cs_id).await?;

            info!(
                ctx.logger(),
                "successful prepare of entries #{:?}", entry_ids
            );

            Ok(CombinedBookmarkUpdateLogEntry {
                components: entries,
                bundle_file: Arc::new(bundle_file),
                timestamps_file: Arc::new(timestamps_file),
                cs_id,
                bookmark: bookmark_name,
            })
        }
        .boxed()
    }

    async fn try_prepare_bundle_timestamps_file<'a>(
        ctx: &'a CoreContext,
        repo: &'a BlobRepo,
        prepare_type: PrepareType,
        hg_server_heads: &'a [ChangesetId],
        bookmark_change: &'a BookmarkChange,
        bookmark_name: &'a BookmarkName,
    ) -> Result<(NamedTempFile, NamedTempFile), Error> {
        let blobstore = repo.get_blobstore();

        match prepare_type {
            PrepareType::Generate {
                lca_hint,
                lfs_params,
                filenode_verifier,
            } => {
                let (bytes, timestamps) = crate::bundle_generator::create_bundle(
                    ctx.clone(),
                    repo.clone(),
                    lca_hint.clone(),
                    bookmark_name.clone(),
                    bookmark_change.clone(),
                    hg_server_heads.to_vec(),
                    lfs_params,
                    filenode_verifier.clone(),
                )
                .compat()
                .await?;

                try_join(
                    save_bytes_to_temp_file(&bytes),
                    save_timestamps_to_file(&timestamps),
                )
                .await
            }
            PrepareType::UseExisting { bundle_replay_data } => {
                match BundleReplayData::try_from(bundle_replay_data) {
                    Ok(bundle_replay_data) => {
                        try_join(
                            save_bundle_to_temp_file(
                                &ctx,
                                &blobstore,
                                bundle_replay_data.bundle2_id,
                            ),
                            save_timestamps_to_file(&bundle_replay_data.timestamps),
                        )
                        .await
                    }
                    Err(e) => Err(e),
                }
            }
        }
    }
}

fn get_session_lfs_params(
    ctx: &CoreContext,
    bookmark: &BookmarkName,
    lfs_params: LfsParams,
    bookmark_regex_force_lfs: &Option<Regex>,
) -> SessionLfsParams {
    if let Some(regex) = bookmark_regex_force_lfs {
        if regex.is_match(bookmark.as_str()) {
            info!(ctx.logger(), "force generating lfs bundle for {}", bookmark);
            return SessionLfsParams {
                threshold: lfs_params.threshold,
            };
        }
    }

    if lfs_params.generate_lfs_blob_in_hg_sync_job {
        SessionLfsParams {
            threshold: lfs_params.threshold,
        }
    } else {
        SessionLfsParams { threshold: None }
    }
}

async fn save_timestamps_to_file(
    timestamps: &HashMap<HgChangesetId, Timestamp>,
) -> Result<NamedTempFile, Error> {
    let encoded_timestamps = timestamps
        .iter()
        .map(|(key, value)| {
            let timestamp = value.timestamp_seconds();
            format!("{}={}", key, timestamp)
        })
        .join("\n");

    write_to_named_temp_file(encoded_timestamps).await
}
