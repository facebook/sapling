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
use bookmarks::BookmarkUpdateLogId;
use borrowed::borrowed;
use changeset_info::ChangesetInfo;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use cloned::cloned;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use context::SessionContainer;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use manifest::compare_manifest_tree;
use manifest::Comparison;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_derivation::derive_hg_changeset::DeriveHgChangeset;
use mercurial_types::blobs::HgBlobManifest;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use metadata::Metadata;
use mononoke_app::args::RepoArg;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::MPath;
use mutable_counters::MutableCountersRef;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use slog::info;
use slog::Logger;
use stats::prelude::*;
use url::Url;

use crate::bul_util;
use crate::sender::dummy::DummySender;
use crate::sender::edenapi::EdenapiSender;
use crate::sender::ModernSyncSender;
use crate::ModernSyncArgs;
use crate::Repo;
const MODERN_SYNC_COUNTER_NAME: &str = "modern_sync";

define_stats! {
    prefix = "mononoke.modern_sync";
    completion_duration_secs: timeseries(Average, Sum, Count),
    synced_commits:  dynamic_timeseries("{}.commits_synced", (repo: String); Rate, Sum),
}

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

    let mut metadata = Metadata::default();
    metadata.add_client_info(ClientInfo::default_with_entry_point(
        ClientEntryPoint::ModernSync,
    ));

    let mut scuba = app.environment().scuba_sample_builder.clone();
    scuba.add_metadata(&metadata);

    let session_container = SessionContainer::builder(app.fb)
        .metadata(Arc::new(metadata))
        .build();

    let ctx = session_container
        .new_context(app.logger().clone(), scuba)
        .clone_with_repo_name(&repo_name.clone());

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
    let app_args = app.args::<ModernSyncArgs>()?;
    let sender: Arc<dyn ModernSyncSender + Send + Sync> = if dry_run {
        Arc::new(DummySender::new(logger.clone()))
    } else {
        let url = if let Some(socket) = app_args.dest_socket {
            // Only for integration tests
            format!("{}:{}/edenapi/", &config.url, socket)
        } else {
            format!("{}/edenapi/", &config.url)
        };

        let tls_args = app_args
            .tls_params
            .clone()
            .ok_or_else(|| format_err!("TLS params not found for repo {}", repo_name))?;

        let dest_repo = app_args.dest_repo_name.clone().unwrap_or(repo_name.clone());

        Arc::new(
            EdenapiSender::new(
                Url::parse(&url)?,
                dest_repo,
                logger.clone(),
                tls_args,
                ctx.clone(),
                repo.repo_blobstore().clone(),
            )
            .await?,
        )
    };

    let mut scuba_sample = ctx.scuba().clone();
    scuba_sample.add("repo", repo_name);
    scuba_sample.add("start_id", start_id);
    scuba_sample.add("dry_run", dry_run);
    scuba_sample.log();

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
                    for entry in entries {
                        let from = entry.from_changeset_id.into_iter().collect();
                        let to = entry.to_changeset_id.into_iter().collect();

                        let mut commits = repo
                            .commit_graph()
                            .ancestors_difference(ctx, to, from)
                            .await?;

                        commits.reverse();

                        stream::iter(commits.into_iter().map(Ok))
                            .try_for_each(|cs_id| {
                                cloned!(ctx, repo, logger, sender);
                                async move {
                                    process_one_changeset(
                                        &cs_id, &ctx, repo, &logger, sender, false,
                                    )
                                    .await
                                }
                            })
                            .await?;

                        if app_args.update_counters {
                            repo.mutable_counters()
                                .set_counter(ctx, MODERN_SYNC_COUNTER_NAME, entry.id.0 as i64, None)
                                .await?;

                            let from_changeset = if let Some(cs_id) = entry.from_changeset_id {
                                Some(repo.derive_hg_changeset(ctx, cs_id).await?)
                            } else {
                                None
                            };

                            let to_changeset = if let Some(cs_id) = entry.to_changeset_id {
                                Some(repo.derive_hg_changeset(ctx, cs_id).await?)
                            } else {
                                None
                            };

                            sender
                                .set_bookmark(
                                    entry.bookmark_name.name().to_string(),
                                    from_changeset,
                                    to_changeset,
                                )
                                .await?;
                        }
                    }
                    Ok(())
                }
            }
        }
    })
    .try_collect::<()>()
    .await?;

    Ok(())
}

pub async fn process_one_changeset(
    cs_id: &ChangesetId,
    ctx: &CoreContext,
    repo: Repo,
    logger: &Logger,
    sender: Arc<dyn ModernSyncSender + Send + Sync>,
    log_completion: bool,
) -> Result<()> {
    info!(logger, "Processing commit {:?}", cs_id);

    let cs_info = repo
        .repo_derived_data()
        .derive::<ChangesetInfo>(ctx, cs_id.clone())
        .await?;
    let bs = cs_id.load(ctx, repo.repo_blobstore()).await?;
    let thing: Vec<_> = bs.file_changes().collect();

    for (_path, file_change) in thing {
        let bs = match file_change {
            FileChange::Change(change) => Some(change.content_id()),
            FileChange::UntrackedChange(change) => Some(change.content_id()),
            _ => None,
        };

        if let Some(bs) = bs {
            let blob = bs.load(ctx, &repo.repo_blobstore()).await?;
            sender.upload_content(bs, blob).await?;
        }
    }

    let mut mf_ids_p = vec![];

    // TODO: Parallelize
    for parent in cs_info.parents() {
        let hg_cs_id = repo.derive_hg_changeset(ctx, parent).await?;
        let hg_cs = hg_cs_id.load(ctx, repo.repo_blobstore()).await?;
        let hg_mf_id = hg_cs.manifestid();
        mf_ids_p.push(hg_mf_id);
    }

    let hg_cs_id = repo.derive_hg_changeset(ctx, *cs_id).await?;
    let hg_cs = hg_cs_id.load(ctx, repo.repo_blobstore()).await?;
    let hg_mf_id = hg_cs.manifestid();

    let (mut mf_ids, file_ids) =
        sort_manifest_changes(ctx, repo.repo_blobstore(), hg_mf_id, mf_ids_p).await?;
    mf_ids.push(hg_mf_id);
    sender.upload_trees(mf_ids).await?;
    sender.upload_filenodes(file_ids).await?;
    sender.upload_hg_changeset(vec![hg_cs]).await?;

    if log_completion {
        STATS::synced_commits.add_value(1, (repo.repo_identity().name().to_string(),));
    }

    Ok(())
}

async fn sort_manifest_changes(
    ctx: &CoreContext,
    repo_blobstore: &RepoBlobstore,
    mf_id: HgManifestId,
    mf_ids_p: Vec<HgManifestId>,
) -> Result<(Vec<mercurial_types::HgManifestId>, Vec<HgFileNodeId>)> {
    let mut mf_ids: Vec<mercurial_types::HgManifestId> = vec![];
    let mut file_ids: Vec<HgFileNodeId> = vec![];

    let comparison_stream =
        compare_manifest_tree::<HgBlobManifest, _>(ctx, repo_blobstore, mf_id, mf_ids_p);
    futures::pin_mut!(comparison_stream);

    while let Some(mf) = comparison_stream.try_next().await? {
        match mf {
            Comparison::New(_elem, entry) => {
                process_new_entry(entry, &mut mf_ids, &mut file_ids, ctx, repo_blobstore).await?;
            }
            Comparison::ManyNew(_path, _prefix, map) => {
                for (_path, entry) in map {
                    process_new_entry(entry, &mut mf_ids, &mut file_ids, ctx, repo_blobstore)
                        .await?;
                }
            }
            Comparison::Changed(_path, entry, _changes) => match entry {
                Entry::Tree(mf_id) => {
                    mf_ids.push(mf_id);
                }
                Entry::Leaf((_ftype, nodeid)) => {
                    file_ids.push(nodeid);
                }
            },

            _ => (),
        }
    }

    Ok((mf_ids, file_ids))
}

async fn process_new_entry(
    entry: Entry<mercurial_types::HgManifestId, (mononoke_types::FileType, HgFileNodeId)>,
    mf_ids: &mut Vec<mercurial_types::HgManifestId>,
    file_ids: &mut Vec<HgFileNodeId>,
    ctx: &CoreContext,
    repo_blobstore: &RepoBlobstore,
) -> Result<()> {
    match entry {
        Entry::Tree(mf_id) => {
            let entries = mf_id
                .list_all_entries(ctx.clone(), repo_blobstore.clone())
                .try_collect::<Vec<_>>()
                .await?;
            classify_entries(entries, mf_ids, file_ids);
        }
        Entry::Leaf((_ftype, nodeid)) => {
            file_ids.push(nodeid);
        }
    }
    Ok(())
}

fn classify_entries(
    entries: Vec<(
        MPath,
        Entry<mercurial_types::HgManifestId, (mononoke_types::FileType, HgFileNodeId)>,
    )>,
    mf_ids: &mut Vec<mercurial_types::HgManifestId>,
    file_ids: &mut Vec<HgFileNodeId>,
) {
    for (_path, entry) in entries {
        match entry {
            Entry::Tree(mf_id) => {
                mf_ids.push(mf_id);
            }
            Entry::Leaf((_ftype, nodeid)) => {
                file_ids.push(nodeid);
            }
        }
    }
}
