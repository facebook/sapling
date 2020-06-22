/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::bonsai_generation::{create_bonsai_changeset_object, save_bonsai_changeset_object};
use crate::repo_commit::*;
use crate::{BlobRepoHg, ErrorKind};
use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use bonsai_hg_mapping::{BonsaiHgMapping, BonsaiHgMappingEntry};
use changesets::{ChangesetInsert, Changesets};
use cloned::cloned;
use context::CoreContext;
use failure_ext::{Compat, FutureFailureErrorExt, FutureFailureExt};
use futures::future::{FutureExt as NewFutureExt, TryFutureExt};
use futures_ext::{spawn_future, BoxFuture, BoxStream, FutureExt};
use futures_old::future::{self, Future};
use futures_old::sync::oneshot;
use futures_old::IntoFuture;
use futures_stats::Timed;
use mercurial_types::{
    blobs::{ChangesetMetadata, HgBlobChangeset, HgBlobEntry},
    HgNodeHash, RepoPath,
};
use mononoke_types::{BlobstoreValue, BonsaiChangeset, MPath};
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use stats::prelude::*;
use std::{
    convert::From,
    sync::{Arc, Mutex},
};
use tracing::{trace_args, EventId, Traced};
use uuid::Uuid;

define_stats! {
    prefix = "mononoke.blobrepo";
    create_changeset: timeseries(Rate, Sum),
    create_changeset_compute_cf: timeseries("create_changeset.compute_changed_files"; Rate, Sum),
    create_changeset_expected_cf: timeseries("create_changeset.expected_changed_files"; Rate, Sum),
    create_changeset_cf_count: timeseries("create_changeset.changed_files_count"; Average, Sum),
}

pub struct CreateChangeset {
    /// This should always be provided, keeping it an Option for tests
    pub expected_nodeid: Option<HgNodeHash>,
    pub expected_files: Option<Vec<MPath>>,
    pub p1: Option<ChangesetHandle>,
    pub p2: Option<ChangesetHandle>,
    // root_manifest can be None f.e. when commit removes all the content of the repo
    pub root_manifest: BoxFuture<Option<(HgBlobEntry, RepoPath)>, Error>,
    pub sub_entries: BoxStream<(HgBlobEntry, RepoPath), Error>,
    pub cs_metadata: ChangesetMetadata,
    pub must_check_case_conflicts: bool,
}

impl CreateChangeset {
    pub fn create(
        self,
        ctx: CoreContext,
        repo: &BlobRepo,
        mut scuba_logger: ScubaSampleBuilder,
    ) -> ChangesetHandle {
        STATS::create_changeset.add_value(1);
        // This is used for logging, so that we can tie up all our pieces without knowing about
        // the final commit hash
        let uuid = Uuid::new_v4();
        scuba_logger.add("changeset_uuid", format!("{}", uuid));
        let event_id = EventId::new();

        let entry_processor = UploadEntries::new(repo.get_blobstore(), scuba_logger.clone());
        let (signal_parent_ready, can_be_parent) = oneshot::channel();
        let signal_parent_ready = Arc::new(Mutex::new(Some(signal_parent_ready)));
        let expected_nodeid = self.expected_nodeid;

        let upload_entries = process_entries(
            ctx.clone(),
            &entry_processor,
            self.root_manifest,
            self.sub_entries,
        )
        .context("While processing entries")
        .traced_with_id(&ctx.trace(), "uploading entries", trace_args!(), event_id);

        let parents_complete = extract_parents_complete(&self.p1, &self.p2);
        let parents_data = handle_parents(scuba_logger.clone(), self.p1, self.p2)
            .context("While waiting for parents to upload data")
            .traced_with_id(
                &ctx.trace(),
                "waiting for parents data",
                trace_args!(),
                event_id,
            );
        let must_check_case_conflicts = self.must_check_case_conflicts.clone();
        let changeset = {
            let mut scuba_logger = scuba_logger.clone();
            upload_entries
                .join(parents_data)
                .from_err()
                .and_then({
                    cloned!(ctx, repo, mut scuba_logger, signal_parent_ready);
                    let expected_files = self.expected_files;
                    let cs_metadata = self.cs_metadata;
                    let blobstore = repo.get_blobstore();

                    move |(root_mf_id, (parents, parent_manifest_hashes, bonsai_parents))| {
                        let files = if let Some(expected_files) = expected_files {
                            STATS::create_changeset_expected_cf.add_value(1);
                            // We are trusting the callee to provide a list of changed files, used
                            // by the import job
                            future::ok(expected_files).boxify()
                        } else {
                            STATS::create_changeset_compute_cf.add_value(1);
                            compute_changed_files(
                                ctx.clone(),
                                repo.clone(),
                                root_mf_id,
                                parent_manifest_hashes.get(0).cloned(),
                                parent_manifest_hashes.get(1).cloned(),
                            )
                        };

                        let p1_mf = parent_manifest_hashes.get(0).cloned();
                        let check_case_conflicts = if must_check_case_conflicts {
                            cloned!(ctx, repo);
                            async move {
                                check_case_conflicts(&ctx, &repo, root_mf_id, p1_mf).await
                            }
                            .boxed()
                            .compat()
                            .left_future()
                        } else {
                            future::ok(()).right_future()
                        };

                        let changesets = files
                            .join(check_case_conflicts)
                            .and_then(move |(files, ())| {
                                STATS::create_changeset_cf_count.add_value(files.len() as i64);
                                make_new_changeset(parents, root_mf_id, cs_metadata, files)
                            })
                            .and_then({
                                cloned!(ctx, parent_manifest_hashes);
                                move |hg_cs| {
                                    create_bonsai_changeset_object(
                                        ctx,
                                        hg_cs.clone(),
                                        parent_manifest_hashes,
                                        bonsai_parents,
                                        repo.clone(),
                                    )
                                    .map(|bonsai_cs| (hg_cs, bonsai_cs))
                                }
                            });

                        changesets
                            .context("While computing changed files")
                            .and_then({
                                cloned!(ctx);
                                move |(blobcs, bonsai_cs)| {
                                    let fut: BoxFuture<(HgBlobChangeset, BonsaiChangeset), Error> =
                                        (move || {
                                            let bonsai_blob = bonsai_cs.clone().into_blob();
                                            let bcs_id = bonsai_blob.id().clone();

                                            let cs_id = blobcs.get_changeset_id().into_nodehash();
                                            let manifest_id = blobcs.manifestid();

                                            if let Some(expected_nodeid) = expected_nodeid {
                                                if cs_id != expected_nodeid {
                                                    return future::err(
                                                        ErrorKind::InconsistentChangesetHash(
                                                            expected_nodeid,
                                                            cs_id,
                                                            blobcs,
                                                        )
                                                        .into(),
                                                    )
                                                    .boxify();
                                                }
                                            }

                                            scuba_logger
                                                .add("changeset_id", format!("{}", cs_id))
                                                .log_with_msg(
                                                    "Changeset uuid to hash mapping",
                                                    None,
                                                );
                                            // NOTE(luk): an attempt was made in D8187210 to split the
                                            // upload_entries signal into upload_entries and
                                            // processed_entries and to signal_parent_ready after
                                            // upload_entries, so that one doesn't need to wait for the
                                            // entries to be processed. There were no performance gains
                                            // from that experiment
                                            //
                                            // We deliberately eat this error - this is only so that
                                            // another changeset can start verifying data in the blob
                                            // store while we verify this one
                                            let _ = signal_parent_ready
                                                .lock()
                                                .expect("poisoned lock")
                                                .take()
                                                .expect("signal_parent_ready cannot be taken yet")
                                                .send(Ok((bcs_id, cs_id, manifest_id)));

                                            let bonsai_cs_fut = save_bonsai_changeset_object(
                                                ctx.clone(),
                                                blobstore.clone(),
                                                bonsai_cs.clone(),
                                            );

                                            blobcs
                                                .save(ctx.clone(), blobstore)
                                                .join(bonsai_cs_fut)
                                                .context("While writing to blobstore")
                                                .join(
                                                    entry_processor
                                                        .finalize(
                                                            ctx,
                                                            root_mf_id,
                                                            parent_manifest_hashes,
                                                        )
                                                        .context("While finalizing processing"),
                                                )
                                                .from_err()
                                                .map(move |_| (blobcs, bonsai_cs))
                                                .boxify()
                                        })();

                                    fut.context(
                                        "While creating and verifying Changeset for blobstore",
                                    )
                                }
                            })
                            .traced_with_id(
                                &ctx.trace(),
                                "uploading changeset",
                                trace_args!(),
                                event_id,
                            )
                            .from_err()
                    }
                })
                .timed(move |stats, result| {
                    if result.is_ok() {
                        scuba_logger
                            .add_future_stats(&stats)
                            .log_with_msg("Changeset created", None);
                    }
                    Ok(())
                })
                .inspect_err({
                    cloned!(signal_parent_ready);
                    move |e| {
                        let trigger = signal_parent_ready.lock().expect("poisoned lock").take();
                        if let Some(trigger) = trigger {
                            // Ignore errors if the receiving end has gone away.
                            let e = format_err!("signal_parent_ready failed: {:?}", e);
                            let _ = trigger.send(Err(e));
                        }
                    }
                })
        };

        let parents_complete = parents_complete
            .context("While waiting for parents to complete")
            .traced_with_id(
                &ctx.trace(),
                "waiting for parents complete",
                trace_args!(),
                event_id,
            )
            .timed({
                let mut scuba_logger = scuba_logger.clone();
                move |stats, result| {
                    if result.is_ok() {
                        scuba_logger
                            .add_future_stats(&stats)
                            .log_with_msg("Parents completed", None);
                    }
                    Ok(())
                }
            });

        let complete_changesets = repo.get_changesets_object();
        cloned!(repo);
        let repoid = repo.get_repoid();
        let changeset_complete_fut = changeset
            .join(parents_complete)
            .and_then({
                cloned!(ctx);
                let bonsai_hg_mapping = repo.get_bonsai_hg_mapping().clone();
                move |((hg_cs, bonsai_cs), _)| {
                    let bcs_id = bonsai_cs.get_changeset_id();
                    let bonsai_hg_entry = BonsaiHgMappingEntry {
                        repo_id: repoid.clone(),
                        hg_cs_id: hg_cs.get_changeset_id(),
                        bcs_id,
                    };

                    bonsai_hg_mapping
                        .add(ctx.clone(), bonsai_hg_entry)
                        .map(move |_| (hg_cs, bonsai_cs))
                        .context("While inserting mapping")
                        .traced_with_id(
                            &ctx.trace(),
                            "uploading bonsai hg mapping",
                            trace_args!(),
                            event_id,
                        )
                }
            })
            .and_then(move |(hg_cs, bonsai_cs)| {
                let completion_record = ChangesetInsert {
                    repo_id: repo.get_repoid(),
                    cs_id: bonsai_cs.get_changeset_id(),
                    parents: bonsai_cs.parents().into_iter().collect(),
                };
                complete_changesets
                    .add(ctx.clone(), completion_record)
                    .map(|_| (bonsai_cs, hg_cs))
                    .context("While inserting into changeset table")
                    .traced_with_id(
                        &ctx.trace(),
                        "uploading final changeset",
                        trace_args!(),
                        event_id,
                    )
            })
            .with_context(move || {
                format!(
                    "While creating Changeset {:?}, uuid: {}",
                    expected_nodeid, uuid
                )
            })
            .timed({
                move |stats, result| {
                    if result.is_ok() {
                        scuba_logger
                            .add_future_stats(&stats)
                            .log_with_msg("CreateChangeset Finished", None);
                    }
                    Ok(())
                }
            });

        let can_be_parent = can_be_parent
            .into_future()
            .then(|r| match r {
                Ok(res) => res,
                Err(e) => Err(format_err!("can_be_parent: {:?}", e)),
            })
            .map_err(Compat)
            .boxify()
            .shared();

        ChangesetHandle::new_pending(
            can_be_parent,
            spawn_future(changeset_complete_fut)
                .map_err(Compat)
                .boxify()
                .shared(),
        )
    }
}
