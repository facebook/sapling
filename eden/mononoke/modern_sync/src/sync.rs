/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::format_err;
use anyhow::Result;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateLogArc;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::BookmarkUpdateLogId;
use bookmarks::BookmarksRef;
use borrowed::borrowed;
use changeset_info::ChangesetInfo;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use cloned::cloned;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use context::SessionContainer;
use filestore::FetchKey;
use futures::channel::oneshot;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use manifest::compare_manifest_tree;
use manifest::Comparison;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_derivation::derive_hg_changeset::DeriveHgChangeset;
use mercurial_types::blobs::HgBlobManifest;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use metadata::Metadata;
use mononoke_app::args::SourceRepoArgs;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::MPath;
use mutable_counters::MutableCountersArc;
use mutable_counters::MutableCountersRef;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use slog::info;
use slog::Logger;
use stats::define_stats;
use stats::prelude::*;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use url::Url;

use crate::bul_util;
use crate::scuba;
use crate::sender::edenapi::EdenapiSender;
use crate::sender::manager::BookmarkInfo;
use crate::sender::manager::ChangesetMessage;
use crate::sender::manager::ContentMessage;
use crate::sender::manager::FileMessage;
use crate::sender::manager::SendManager;
use crate::sender::manager::TreeMessage;
use crate::sender::manager::MODERN_SYNC_BATCH_CHECKPOINT_NAME;
use crate::sender::manager::MODERN_SYNC_COUNTER_NAME;
use crate::sender::manager::MODERN_SYNC_CURRENT_ENTRY_ID;
use crate::ModernSyncArgs;
use crate::Repo;

define_stats! {
    prefix = "mononoke.modern_sync";
    changeset_procesing_time_s:  dynamic_timeseries("{}.changeset_procesing_time", (repo: String); Average),
    changeset_procesed:  dynamic_timeseries("{}.changeset_procesed", (repo: String); Sum),

}

#[derive(Clone)]
pub enum ExecutionType {
    SyncOnce,
    Tail,
}

pub async fn sync(
    app: Arc<MononokeApp>,
    start_id_arg: Option<u64>,
    source_repo_arg: SourceRepoArgs,
    dest_repo_name: String,
    exec_type: ExecutionType,
    dry_run: bool,
    chunk_size: u64,
    exit_file: PathBuf,
) -> Result<()> {
    let repo: Repo = app.open_repo(&source_repo_arg).await?;
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

    let scuba = scuba::new(app.clone(), &metadata, &repo_name, dry_run);
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

    let sender = {
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

        Arc::new(
            EdenapiSender::new(
                Url::parse(&url)?,
                dest_repo_name.clone(),
                logger.clone(),
                tls_args,
                ctx.clone(),
                repo.repo_blobstore().clone(),
            )
            .await?,
        )
    };
    info!(logger, "Established EdenAPI connection");

    let send_manager = SendManager::new(
        ctx.clone(),
        sender.clone(),
        logger.clone(),
        repo_name.clone(),
        exit_file,
        repo.mutable_counters_arc(),
    );
    info!(logger, "Initialized channels");

    scuba::log_sync_start(ctx, start_id);

    let last_entry = Arc::new(RwLock::new(None));
    bul_util::read_bookmark_update_log(
        ctx,
        BookmarkUpdateLogId(start_id),
        exec_type,
        repo.bookmark_update_log_arc(),
    )
    .then(|entries| {
        cloned!(repo, logger, sender, mut send_manager, last_entry);
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
                Ok(mut entries) => {
                    if app_args.flatten_bul && !entries.is_empty() {
                        let original_size = entries.len();
                        let flattened_bul = bul_util::group_entries(entries);
                        info!(
                            logger,
                            "Grouped {} entries into {} macro-entries",
                            original_size,
                            flattened_bul.len()
                        );
                        entries = flattened_bul;
                    }
                    for entry in entries {
                        let now = std::time::Instant::now();

                        process_bookmark_update_log_entry(
                            ctx,
                            &repo,
                            &entry,
                            &send_manager,
                            sender.clone(),
                            chunk_size,
                            app_args.log_to_ods,
                            &logger,
                            *last_entry.read().await,
                        )
                        .await
                        .inspect(|_| {
                            scuba::log_bookmark_update_entry_done(ctx, &entry, now.elapsed());
                        })
                        .inspect_err(|e| {
                            scuba::log_bookmark_update_entry_error(ctx, &entry, e, now.elapsed());
                        })?;
                        *last_entry.write().await = entry.to_changeset_id;
                    }
                    Ok(())
                }
            }
        }
    })
    .try_collect::<()>()
    .await?;

    // Wait for the last commit to be synced before exiting
    let (finish_tx, finish_rx) = oneshot::channel();
    send_manager
        .send_changeset(ChangesetMessage::NotifyCompletion(finish_tx))
        .await?;
    let _ = finish_rx.await?;

    Ok(())
}

pub async fn process_bookmark_update_log_entry(
    ctx: &CoreContext,
    repo: &Repo,
    entry: &BookmarkUpdateLogEntry,
    send_manager: &SendManager,
    sender: Arc<EdenapiSender>,
    chunk_size: u64,
    log_to_ods: bool,
    logger: &Logger,
    last_entry: Option<ChangesetId>,
) -> Result<()> {
    let repo_name = repo.repo_identity().name().to_string();

    let to_cs = entry
        .to_changeset_id
        .expect("bookmark update log entry should have a destination");

    // If the entry has a source, use it. Otherwise, get it from the bookmark.
    let from_cs = if let Some(cs_id) = entry.from_changeset_id {
        Some(cs_id)
    } else if let Some(cs_id) = last_entry {
        Some(cs_id)
    } else if let Some(hgid) = sender
        .read_bookmark(entry.bookmark_name.as_str().to_owned())
        .await?
    {
        repo.bonsai_hg_mapping()
            .get_bonsai_from_hg(ctx, hgid)
            .await?
    } else {
        None
    };

    let from_vec: Vec<ChangesetId> = from_cs.into_iter().collect();
    let to_vec: Vec<ChangesetId> = vec![to_cs];
    let bookmark_name = entry.bookmark_name.name().to_string();

    let to_generation = repo.commit_graph().changeset_generation(ctx, to_cs).await?;
    let (approx_count, approx_count_str) = if let Some(from_cs) = entry.from_changeset_id {
        let from_generation = repo
            .commit_graph()
            .changeset_generation(ctx, from_cs)
            .await?;
        let diff = to_generation.difference_from(from_generation);
        let diff_str = diff.map_or_else(
            // on the off chance we can't compute the difference, just log both generations
            || {
                format!(
                    "generation from {:?} to {:?}",
                    from_generation, to_generation
                )
            },
            |count| format!("approx {} commit(s)", count),
        );
        (diff, diff_str)
    } else {
        (
            Some(to_generation.value()),
            format!("to generation {:?}", to_generation.value()),
        )
    };
    info!(
        logger,
        "Calculating segments for entry {}, from changeset {:?} to changeset {:?}, {}",
        entry.id,
        from_cs,
        to_cs,
        approx_count_str,
    );
    let (_, ctx) = { scuba::log_bookmark_update_entry_start(ctx, entry, approx_count) };

    let commits = repo
        .commit_graph()
        .ancestors_difference_segment_slices(&ctx, to_vec, from_vec, chunk_size)
        .await?;

    let checkpointed_entry = repo
        .mutable_counters()
        .get_counter(&ctx, MODERN_SYNC_CURRENT_ENTRY_ID)
        .await?
        .unwrap_or(0);

    let latest_checkpoint = if checkpointed_entry == entry.id.0 as i64 {
        repo.mutable_counters()
            .get_counter(&ctx, MODERN_SYNC_BATCH_CHECKPOINT_NAME)
            .await?
            .unwrap_or(0)
    } else {
        0
    };

    info!(
        logger,
        "Resuming from latest entry checkpoint {}", latest_checkpoint
    );

    let skip_batch = (latest_checkpoint as u64) / chunk_size;
    let mut skip_commits = (latest_checkpoint as u64) % chunk_size;

    info!(
        logger,
        "Skipping {} batches from entry {}", skip_batch, entry.id
    );

    let current_position = Arc::new(Mutex::new(latest_checkpoint as u64));

    commits
        .skip(skip_batch as usize)
        .try_for_each(|chunk| {
            cloned!(
                ctx,
                repo,
                logger,
                sender,
                mut send_manager,
                bookmark_name,
                current_position
            );
            info!(logger, "Skipping {} commits within batch", skip_commits);
            let skip = std::mem::replace(&mut skip_commits, 0);

            async move {
                let hgids = stream::iter(chunk)
                    .skip(skip as usize)
                    .map(|cs_id| {
                        cloned!(repo, ctx);
                        async move {
                            let hgid = repo.derive_hg_changeset(&ctx, cs_id).await;
                            (hgid, cs_id)
                        }
                    })
                    .buffered(100)
                    .collect::<Vec<(Result<HgChangesetId, anyhow::Error>, ChangesetId)>>()
                    .await;
                let hgids_len = hgids.len();

                let ids = hgids
                    .into_iter()
                    .map(|(hgid, csid)| Ok((hgid?, csid)))
                    .collect::<Result<Vec<(HgChangesetId, ChangesetId)>>>()?;

                let missing_changesets = sender.filter_existing_commits(ids).await?;
                let existing_changesets = hgids_len - missing_changesets.len();
                *current_position.lock().await += existing_changesets as u64;

                info!(
                    logger,
                    "Starting sync of {} missing commits, {} were already synced",
                    missing_changesets.len(),
                    existing_changesets
                );

                stream::iter(missing_changesets.into_iter().map(Ok))
                    .try_for_each(|cs_id| {
                        cloned!(
                            ctx,
                            repo,
                            logger,
                            send_manager,
                            bookmark_name,
                            current_position
                        );

                        async move {
                            let now = std::time::Instant::now();

                            *current_position.lock().await += 1;
                            match process_one_changeset(
                                &cs_id,
                                &ctx,
                                repo,
                                logger,
                                &send_manager,
                                log_to_ods,
                                &bookmark_name,
                                Some((current_position.lock().await.clone(), entry.id.0 as i64)),
                            )
                            .await
                            {
                                Ok(res) => {
                                    scuba::log_changeset_done(&ctx, &cs_id, now.elapsed());
                                    Ok(res)
                                }
                                Err(e) => {
                                    scuba::log_changeset_error(&ctx, &cs_id, &e, now.elapsed());
                                    Err(e)
                                }
                            }
                        }
                    })
                    .await?;
                Ok(())
            }
        })
        .await?;

    send_manager
        .send_changeset(ChangesetMessage::CheckpointInEntry(0, entry.id.0 as i64))
        .await?;

    let from_changeset = if let Some(cs_id) = entry.from_changeset_id {
        Some(repo.derive_hg_changeset(&ctx, cs_id).await?)
    } else {
        None
    };

    let to_changeset = if let Some(cs_id) = entry.to_changeset_id {
        Some(repo.derive_hg_changeset(&ctx, cs_id).await?)
    } else {
        None
    };

    send_manager
        .send_changeset(ChangesetMessage::FinishEntry(
            BookmarkInfo {
                name: entry.bookmark_name.name().to_string(),
                from_cs_id: from_changeset,
                to_cs_id: to_changeset,
            },
            entry.id.0 as i64,
        ))
        .await?;

    bul_util::update_remaining_moves(
        entry.id,
        repo_name.clone(),
        ctx.clone(),
        repo.bookmark_update_log_arc(),
    )
    .await?;

    Ok(())
}

pub async fn process_one_changeset(
    cs_id: &ChangesetId,
    ctx: &CoreContext,
    repo: Repo,
    logger: Logger,
    send_manager: &SendManager,
    log_to_ods: bool,
    bookmark_name: &str,
    checkpoint: Option<(u64, i64)>, // (position, entry_id)
) -> Result<()> {
    scuba::log_changeset_start(ctx, cs_id);

    let now = std::time::Instant::now();

    let cs_info = repo
        .repo_derived_data()
        .derive::<ChangesetInfo>(ctx, cs_id.clone())
        .await?;

    let bs_cs = cs_id.load(ctx, repo.repo_blobstore()).await?;
    let commit_time = bs_cs.author_date().timestamp_secs();
    let cids: Vec<_> = bs_cs
        .file_changes()
        .filter_map(|(_path, file_change)| match file_change {
            FileChange::Change(change) => Some(change.content_id()),
            FileChange::UntrackedChange(change) => Some(change.content_id()),
            FileChange::Deletion | FileChange::UntrackedDeletion => None,
        })
        .collect();

    // Read the sizes of the contents concurrently (by reading the metadata blobs from blobstore)
    // Larger commits/older not cached commits would benefit from this concurrency.
    stream::iter(cids)
        .map(|cid| {
            cloned!(ctx, repo, send_manager);
            async move {
                let metadata =
                    filestore::get_metadata(repo.repo_blobstore(), &ctx, &FetchKey::Canonical(cid))
                        .await?
                        .expect("blob not found");
                send_manager
                    .send_content(ContentMessage::Content(cid, metadata.total_size))
                    .await?;
                anyhow::Ok(())
            }
        })
        .buffered(100)
        .try_collect::<Vec<_>>()
        .await?;

    // Notify contents for this changeset are ready
    let (content_files_tx, content_files_rx) = oneshot::channel();
    let (content_trees_tx, content_trees_rx) = oneshot::channel();
    send_manager
        .send_content(ContentMessage::ContentDone(
            content_files_tx,
            content_trees_tx,
        ))
        .await?;

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

    // Send files and trees
    send_manager
        .send_file(FileMessage::WaitForContents(content_files_rx))
        .await?;

    send_manager
        .send_tree(TreeMessage::WaitForContents(content_trees_rx))
        .await?;

    // Notify files and trees for this changeset are ready
    let (f_tx, f_rx) = oneshot::channel();
    let (t_tx, t_rx) = oneshot::channel();

    let (_, _) = tokio::try_join!(
        async {
            for mf_id in mf_ids {
                send_manager.send_tree(TreeMessage::Tree(mf_id)).await?;
            }
            send_manager.send_tree(TreeMessage::TreesDone(t_tx)).await?;
            anyhow::Ok(())
        },
        async {
            cloned!(send_manager);
            for file_id in file_ids {
                send_manager
                    .send_file(FileMessage::FileNode(file_id))
                    .await?;
            }
            send_manager.send_file(FileMessage::FilesDone(f_tx)).await?;
            anyhow::Ok(())
        }
    )?;

    // Upload changeset
    send_manager
        .send_changeset(ChangesetMessage::WaitForFilesAndTrees(f_rx, t_rx))
        .await?;
    send_manager
        .send_changeset(ChangesetMessage::Changeset((hg_cs, bs_cs)))
        .await?;

    if let Some(checkpoint) = checkpoint {
        send_manager
            .send_changeset(ChangesetMessage::CheckpointInEntry(
                checkpoint.0,
                checkpoint.1,
            ))
            .await?;
    }

    if log_to_ods {
        let lag = if let Some(cs_id) = repo
            .bookmarks()
            .get(ctx.clone(), &BookmarkKey::new(bookmark_name)?)
            .await?
        {
            let bookmark_commit = cs_id.load(ctx, repo.repo_blobstore()).await?;
            let bookmark_commit_time = bookmark_commit.author_date().timestamp_secs();

            Some(bookmark_commit_time - commit_time)
        } else {
            info!(logger, "Bookmark {} not found", bookmark_name);
            None
        };

        send_manager
            .send_changeset(ChangesetMessage::Log((
                repo.repo_identity().name().to_string(),
                lag,
            )))
            .await?;
    }

    let elapsed = now.elapsed();
    STATS::changeset_procesing_time_s.add_value(
        elapsed.as_secs() as i64,
        (repo.repo_identity().name().to_string(),),
    );
    STATS::changeset_procesed.add_value(1, (repo.repo_identity().name().to_string(),));

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

pub(crate) async fn get_unsharded_repo_args(
    app: Arc<MononokeApp>,
    app_args: &ModernSyncArgs,
) -> Result<(SourceRepoArgs, String)> {
    let source_repo: Repo = app.open_repo(&app_args.repo).await?;
    let source_repo_name = source_repo.repo_identity.name().to_string();
    let target_repo_name = app_args
        .dest_repo_name
        .clone()
        .unwrap_or(source_repo_name.clone());

    Ok((
        SourceRepoArgs::with_name(source_repo_name),
        target_repo_name,
    ))
}
