/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::format_err;
use anyhow::Result;
use assembly_line::TryAssemblyLine;
use blobstore::Loadable;
use bookmarks::BookmarkUpdateLogArc;
use bookmarks::BookmarkUpdateLogId;
use borrowed::borrowed;
use changeset_info::ChangesetInfo;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use cloned::cloned;
use commit_graph::CommitGraphArc;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use mononoke_app::args::RepoArg;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mutable_counters::MutableCountersRef;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use slog::info;
use slog::Logger;
use url::Url;

use crate::bul_util;
use crate::sender::dummy::DummySender;
use crate::sender::edenapi::EdenapiSender;
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
    dry_run: bool,
) -> Result<()> {
    let repo: Repo = app.open_repo(&repo_arg).await?;
    let _repo_id = repo.repo_identity().id();
    let repo_name = repo.repo_identity().name().to_string();

    let config = repo
        .repo_config
        .modern_sync_config
        .clone()
        .ok_or(format_err!(
            "No modern sync config found for repo {}",
            repo_name
        ))?;

    let logger = app.logger().clone();

    let ctx = CoreContext::new_with_logger_and_client_info(
        app.fb,
        logger.clone(),
        ClientInfo::default_with_entry_point(ClientEntryPoint::ModernSync),
    )
    .clone_with_repo_name(&repo_name);

    borrowed!(ctx);
    let start_id = if let Some(id) = start_id_arg {
        id
    } else {
        repo.mutable_counters()
            .get_counter(ctx, MODERN_SYNC_COUNTER_NAME)
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

    let sender: Arc<dyn ModernSyncSender + Send + Sync> = if dry_run {
        Arc::new(DummySender::new(logger.clone()))
    } else {
        Arc::new(EdenapiSender::new(Url::parse(&config.url)?, repo_name)?)
    };

    bul_util::read_bookmark_update_log(
        ctx,
        BookmarkUpdateLogId(start_id),
        exec_type,
        repo.bookmark_update_log_arc(),
    )
    .then(|entries| {
        cloned!(repo, logger, sender);
        borrowed!(ctx);
        async move {
            match entries {
                Err(e) => {
                    info!(
                        logger,
                        "Found error while getting bookmark update log entry {:#?}", e
                    );
                    Err(e)
                }
                Ok(entries) => {
                    bul_util::get_commit_stream(entries, repo.commit_graph_arc(), ctx)
                        .await
                        .fuse()
                        .try_next_step(move |cs_id| {
                            cloned!(ctx, repo, logger, sender);
                            async move {
                                process_one_changeset(&cs_id, &ctx, repo, &logger, sender).await
                            }
                        })
                        .try_collect::<()>()
                        .await
                }
            }
            // TODO Update counter after processing one entry
        }
    })
    .try_collect::<()>()
    .await?;

    Ok(())
}

async fn process_one_changeset(
    cs_id: &ChangesetId,
    ctx: &CoreContext,
    repo: Repo,
    logger: &Logger,
    sender: Arc<dyn ModernSyncSender + Send + Sync>,
) -> Result<()> {
    info!(logger, "Found commit {:?}", cs_id);
    let cs_info = repo
        .repo_derived_data()
        .derive::<ChangesetInfo>(ctx, cs_id.clone())
        .await?;
    info!(logger, "Commit info {:?}", cs_info);
    let bs = cs_id.load(ctx, repo.repo_blobstore()).await?;
    let thing: Vec<_> = bs.file_changes().collect();

    for (_path, file_change) in thing {
        info!(logger, "File change {:?}", file_change);
        let bs = match file_change {
            FileChange::Change(change) => Some(change.content_id()),
            FileChange::UntrackedChange(change) => Some(change.content_id()),
            _ => None,
        };

        if let Some(bs) = bs {
            let blob = bs.load(ctx, &repo.repo_blobstore()).await?;
            sender.upload_content(bs, blob);
        }
    }
    Ok(())
}
