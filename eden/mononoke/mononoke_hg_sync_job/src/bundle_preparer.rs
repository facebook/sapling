/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::bundle_generator::{BookmarkChange, FilenodeVerifier};
use crate::errors::ErrorKind::{ReplayDataMissing, UnexpectedBookmarkMove};
use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use bookmarks::{BookmarkName, BookmarkUpdateLogEntry, BookmarkUpdateReason, RawBundleReplayData};
use cloned::cloned;
use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    future::{try_join, FutureExt as _, TryFutureExt},
};
use futures_ext::{BoxFuture, FutureExt};
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

    pub fn prepare_single_bundle(
        &self,
        ctx: CoreContext,
        log_entry: BookmarkUpdateLogEntry,
        overlay: crate::BookmarkOverlay,
    ) -> BoxFuture<PreparedBookmarkUpdateLogEntry, Error> {
        cloned!(self.repo, self.ty);

        let base_retry_delay_ms = self.base_retry_delay_ms;
        let retry_num = self.retry_num;
        async move {
            let entry_id = log_entry.id;
            let book_values = &overlay.get_bookmark_values();
            let (p, _attempts) = retry(
                &ctx.logger(),
                {
                    |_| {
                        Self::try_prepare_single_bundle(
                            &ctx,
                            &repo,
                            log_entry.clone(),
                            &ty,
                            book_values,
                        )
                    }
                },
                base_retry_delay_ms,
                retry_num,
            )
            .await?;

            info!(ctx.logger(), "successful prepare of entry #{}", entry_id);
            Ok(p)
        }
        .boxed()
        .compat()
        .boxify()
    }

    async fn try_prepare_single_bundle(
        ctx: &CoreContext,
        repo: &BlobRepo,
        log_entry: BookmarkUpdateLogEntry,
        bundle_type: &BundleType,
        hg_server_heads: &[ChangesetId],
    ) -> Result<PreparedBookmarkUpdateLogEntry, Error> {
        use BookmarkUpdateReason::*;

        info!(ctx.logger(), "preparing log entry #{} ...", log_entry.id);

        enum PrepareType<'a> {
            Generate {
                lca_hint: &'a Arc<dyn LeastCommonAncestorsHint>,
                lfs_params: SessionLfsParams,
                filenode_verifier: &'a FilenodeVerifier,
            },
            UseExisting {
                bundle_replay_data: &'a RawBundleReplayData,
            },
        }

        let blobstore = repo.get_blobstore();
        match log_entry.reason {
            Pushrebase | Backsyncer | ManualMove | ApiRequest | XRepoSync | Push => {}
            Blobimport | TestMove => {
                return Err(UnexpectedBookmarkMove(format!("{}", log_entry.reason)).into());
            }
        }

        let prepare_type = match bundle_type {
            BundleType::GenerateNew {
                lca_hint,
                lfs_params,
                filenode_verifier,
                bookmark_regex_force_lfs,
            } => PrepareType::Generate {
                lca_hint,
                lfs_params: get_session_lfs_params(
                    &ctx,
                    &log_entry.bookmark_name,
                    lfs_params.clone(),
                    &bookmark_regex_force_lfs,
                ),
                filenode_verifier,
            },
            BundleType::UseExisting => match &log_entry.bundle_replay_data {
                Some(bundle_replay_data) => PrepareType::UseExisting { bundle_replay_data },
                None => {
                    return Err(ReplayDataMissing { id: log_entry.id }.into());
                }
            },
        };

        let bundle_and_timestamps_files = async {
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
                        log_entry.bookmark_name.clone(),
                        BookmarkChange::new(&log_entry)?,
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
        };

        let cs_id = async {
            match log_entry.to_changeset_id {
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
            try_join(bundle_and_timestamps_files, cs_id).await?;

        Ok(PreparedBookmarkUpdateLogEntry {
            log_entry: log_entry.clone(),
            bundle_file: Arc::new(bundle_file),
            timestamps_file: Arc::new(timestamps_file),
            cs_id,
        })
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
