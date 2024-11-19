/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::format_err;
use anyhow::Result;
use blobstore::Loadable;
use bookmarks::BookmarkUpdateLogArc;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::BookmarkUpdateLogId;
use changeset_info::ChangesetInfo;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use mononoke_app::args::RepoArg;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mutable_counters::MutableCountersRef;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataArc;
use repo_identity::RepoIdentityRef;
use slog::info;
use slog::Logger;

use crate::bul_util;
use crate::sender::ModernSyncSender;
use crate::Repo;
const MODERN_SYNC_COUNTER_NAME: &str = "modern_sync";

#[derive(Clone)]
pub enum ExecutionType {
    SyncOnce,
    Tail,
}

pub async fn sync(
    app: Arc<MononokeApp>,
    start_id_arg: Option<u64>,
    repo_arg: RepoArg,
    exec_type: ExecutionType,
) -> Result<()> {
    let repo: Repo = app.open_repo(&repo_arg).await?;
    let _repo_id = repo.repo_identity().id();
    let repo_name = repo.repo_identity().name().to_string();

    let ctx = CoreContext::new_with_logger_and_client_info(
        app.fb,
        app.logger().clone(),
        ClientInfo::default_with_entry_point(ClientEntryPoint::ModernSync),
    )
    .clone_with_repo_name(&repo_name);

    let start_id = if let Some(id) = start_id_arg {
        id
    } else {
        repo.mutable_counters()
            .get_counter(&ctx, MODERN_SYNC_COUNTER_NAME)
            .await?
            .map(|val| val.try_into())
            .transpose()?
            .ok_or_else(|| {
                format_err!(
                    "No start-id or mutable counter {} provided",
                    MODERN_SYNC_COUNTER_NAME
                )
            })?
    };

    let entries = bul_util::read_bookmark_update_log(
        &ctx,
        BookmarkUpdateLogId(start_id),
        exec_type,
        repo.bookmark_update_log_arc(),
    );

    // TODO: This is a bit of a hack. We should be able to get the entries as a stream
    let entries_vec: Vec<BookmarkUpdateLogEntry> = entries
        .collect::<Vec<_>>()
        .await
        .into_iter() // Propagate errors
        .collect::<Result<Vec<Vec<BookmarkUpdateLogEntry>>>>() // Collect into a Result of Vec of Vecs
        .map(|vecs| vecs.into_iter().flatten().collect())?;

    let sender = ModernSyncSender::new(app.logger().clone());
    for raw_entry in entries_vec {
        info!(app.logger(), "Entry {:?}", raw_entry);
        let from = raw_entry.from_changeset_id.map_or(vec![], |val| vec![val]);
        let to = raw_entry.to_changeset_id.map_or(vec![], |val| vec![val]);

        let mut res = repo
            .commit_graph
            .ancestors_difference_stream(&ctx, to, from)
            .await?;

        while let Some(cs_id) = res.try_next().await? {
            process_one_changeset(
                &cs_id,
                &ctx,
                repo.repo_derived_data_arc(),
                repo.repo_blobstore_arc(),
                app.logger(),
                &sender,
            )
            .await?;
        }
    }

    Ok(())
}

async fn process_one_changeset(
    cs_id: &ChangesetId,
    ctx: &CoreContext,
    derived_data: Arc<RepoDerivedData>,
    blobstore: Arc<RepoBlobstore>,
    logger: &Logger,
    sender: &ModernSyncSender,
) -> Result<()> {
    info!(logger, "Found commit {:?}", cs_id);
    let cs_info = derived_data
        .derive::<ChangesetInfo>(ctx, cs_id.clone())
        .await?;
    info!(logger, "Commit info {:?}", cs_info);
    let bs = cs_id.load(ctx, &blobstore).await?;
    let thing: Vec<_> = bs.file_changes().collect();

    for (_path, file_change) in thing {
        info!(logger, "File change {:?}", file_change);
        let bs = match file_change {
            FileChange::Change(change) => Some(change.content_id()),
            FileChange::UntrackedChange(change) => Some(change.content_id()),
            _ => None,
        };

        if let Some(bs) = bs {
            let blob = bs.load(ctx, &blobstore).await?;
            sender.upload_content(bs, blob);
        }
    }
    Ok(())
}
