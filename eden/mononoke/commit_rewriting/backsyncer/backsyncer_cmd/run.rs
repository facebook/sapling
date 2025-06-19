/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Error;
use anyhow::format_err;
use backsyncer::BacksyncLimit;
use backsyncer::Repo;
use backsyncer::backsync_latest;
use backsyncer::format_counter;
use backsyncer::open_backsyncer_dbs;
use blobrepo_hg::BlobRepoHg;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::Freshness;
use cloned::cloned;
use cmdlib_cross_repo::create_single_direction_commit_syncer;
use context::CoreContext;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::CommitSyncData;
use cross_repo_sync::CommitSyncOutcome;
use cross_repo_sync::sync_commit;
use futures::future;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::try_join;
use live_commit_sync_config::LiveCommitSyncConfig;
use mercurial_derivation::DeriveHgChangeset;
use mercurial_types::HgChangesetId;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use repo_identity::RepoIdentityRef;
use slog::debug;
use slog::info;
use stats::prelude::*;
use wireproto_handler::TargetRepoDbs;

use crate::cli::BacksyncerArgs;
use crate::cli::BacksyncerCommand;
use crate::cli::CommitsCommandArgs;

define_stats! {
    prefix = "mononoke.backsyncer";
    remaining_entries: dynamic_singleton_counter(
        "{}.{}.remaining_entries",
        (source_repo_name: String, target_repo_name: String)
    ),
    delay_secs: dynamic_singleton_counter(
        "{}.{}.delay_secs",
        (source_repo_name: String, target_repo_name: String)
    ),
}

pub(crate) async fn run_backsyncer(
    ctx: Arc<CoreContext>,
    app: Arc<MononokeApp>,

    large_repo: Repo,
    small_repo: Repo,
    cancellation_requested: Arc<AtomicBool>,
) -> Result<(), Error> {
    let args: BacksyncerArgs = app.args()?;
    let logger = ctx.logger();
    let commit_sync_data =
        create_single_direction_commit_syncer(&ctx, &app, large_repo.clone(), small_repo.clone())
            .await?;

    info!(
        logger,
        "syncing from repoid {:?} into repoid {:?}",
        large_repo.repo_identity().id(),
        small_repo.repo_identity().id(),
    );

    let live_commit_sync_config = commit_sync_data.live_commit_sync_config.clone();

    match args.command {
        BacksyncerCommand::Once => {
            let target_repo_dbs = Arc::new(
                open_backsyncer_dbs(commit_sync_data.get_target_repo())
                    .boxed()
                    .await?,
            );

            // TODO(ikostia): why do we use discarding ScubaSample for BACKSYNC_ALL?
            backsync_latest(
                ctx.as_ref().clone(),
                commit_sync_data,
                target_repo_dbs,
                BacksyncLimit::NoLimit,
                cancellation_requested,
                CommitSyncContext::Backsyncer,
                false,
                Box::new(future::ready(())),
            )
            .boxed()
            .await?
            .await;
        }
        BacksyncerCommand::Forever => {
            let target_repo_dbs = Arc::new(
                open_backsyncer_dbs(commit_sync_data.get_target_repo())
                    .boxed()
                    .await?,
            );

            let f = backsync_forever(
                ctx.as_ref(),
                commit_sync_data,
                target_repo_dbs,
                large_repo.repo_identity().name().to_string(),
                small_repo.repo_identity().name().to_string(),
                live_commit_sync_config,
                cancellation_requested,
            )
            .boxed();
            f.await?;
        }
        BacksyncerCommand::Commits(CommitsCommandArgs {
            input_file,
            batch_size: mb_batch_size,
        }) => {
            let inputfile = File::open(input_file)?;
            let file = BufReader::new(&inputfile);
            let batch_size = mb_batch_size.unwrap_or(100);

            let source_repo = commit_sync_data.get_source_repo().clone();

            let mut hg_cs_ids = vec![];
            for line in file.lines() {
                hg_cs_ids.push(HgChangesetId::from_str(&line?)?);
            }
            let total_to_backsync = hg_cs_ids.len();
            info!(ctx.logger(), "backsyncing {} commits", total_to_backsync);

            let commit_sync_data = &commit_sync_data;

            // Before processing each commit, check if cancellation has
            // been requested and exit if that's the case.
            if cancellation_requested.load(Ordering::Relaxed) {
                info!(ctx.logger(), "sync stopping due to cancellation request");
                return Ok(());
            }
            let f = stream::iter(hg_cs_ids.clone())
                .chunks(batch_size)
                .map(Result::<_, Error>::Ok)
                .and_then({
                    cloned!(ctx);
                    move |chunk| {
                        cloned!(ctx, source_repo);
                        async move {
                            source_repo
                                .get_hg_bonsai_mapping(ctx.as_ref().clone(), chunk)
                                .await
                        }
                    }
                })
                .try_fold(0, move |backsynced_so_far, hg_bonsai_mapping| {
                    hg_bonsai_mapping
                        .into_iter()
                        .map({
                            cloned!(ctx);
                            move |(_, bonsai)| {
                                cloned!(ctx);
                                async move {
                                    // Backsyncer is always used in the large-to-small direction,
                                    // therefore there can be at most one remapped candidate,
                                    // so `CandidateSelectionHint::Only` is a safe choice

                                    sync_commit(
                                        ctx.as_ref(),
                                        bonsai.clone(),
                                        commit_sync_data,
                                        CandidateSelectionHint::Only,
                                        CommitSyncContext::Backsyncer,
                                        false,
                                    )
                                    .await?;

                                    let maybe_sync_outcome = commit_sync_data
                                        .get_commit_sync_outcome(&ctx, bonsai)
                                        .await?;

                                    info!(
                                        ctx.logger(),
                                        "{} backsynced as {:?}", bonsai, maybe_sync_outcome
                                    );

                                    let maybe_target_cs_id = extract_cs_id_from_sync_outcome(
                                        bonsai,
                                        maybe_sync_outcome,
                                    )?;

                                    derive_target_hg_changesets(
                                        &ctx,
                                        maybe_target_cs_id,
                                        commit_sync_data,
                                    )
                                    .await
                                }
                            }
                        })
                        .collect::<stream::futures_unordered::FuturesUnordered<_>>()
                        .try_fold(backsynced_so_far, {
                            cloned!(ctx);
                            move |backsynced_so_far, _| {
                                let logger = ctx.logger().clone();
                                async move {
                                    info!(
                                        logger,
                                        "backsynced so far {} out of {}",
                                        backsynced_so_far + 1,
                                        total_to_backsync
                                    );
                                    Ok::<_, Error>(backsynced_so_far + 1)
                                }
                            }
                        })
                });

            f.await?;
        }
    }

    Ok(())
}

async fn backsync_forever(
    ctx: &CoreContext,
    commit_sync_data: CommitSyncData<Repo>,
    target_repo_dbs: Arc<TargetRepoDbs>,
    source_repo_name: String,
    target_repo_name: String,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
    cancellation_requested: Arc<AtomicBool>,
) -> Result<(), Error> {
    let target_repo_id = commit_sync_data.get_target_repo_id();
    let mut commit_only_backsync_future: Box<dyn futures::Future<Output = ()> + Send + Unpin> =
        Box::new(future::ready(()));

    loop {
        // Before initiating loop, check if cancellation has been
        // requested. If yes, then exit early.
        if cancellation_requested.load(Ordering::Relaxed) {
            info!(ctx.logger(), "sync stopping due to cancellation request");
            return Ok(());
        }
        // We only care about public pushes because draft pushes are not in the bookmark
        // update log at all.
        let enabled = live_commit_sync_config
            .push_redirector_enabled_for_public(ctx, target_repo_id)
            .await?;

        if enabled {
            let delay = calculate_delay(ctx, &commit_sync_data, &target_repo_dbs).await?;
            log_delay(ctx, &delay, &source_repo_name, &target_repo_name);
            if delay.remaining_entries == 0 {
                debug!(ctx.logger(), "no entries remained");
                tokio::time::sleep(Duration::new(1, 0)).await;
            } else {
                debug!(ctx.logger(), "backsyncing...");

                commit_only_backsync_future = backsync_latest(
                    ctx.clone(),
                    commit_sync_data.clone(),
                    target_repo_dbs.clone(),
                    BacksyncLimit::NoLimit,
                    Arc::clone(&cancellation_requested),
                    CommitSyncContext::Backsyncer,
                    false,
                    commit_only_backsync_future,
                )
                .await?
            }
        } else {
            debug!(ctx.logger(), "push redirector is disabled");
            let delay = Delay::no_delay();
            log_delay(ctx, &delay, &source_repo_name, &target_repo_name);
            tokio::time::sleep(Duration::new(1, 0)).await;
        }
    }
}

fn extract_cs_id_from_sync_outcome(
    source_cs_id: ChangesetId,
    maybe_sync_outcome: Option<CommitSyncOutcome>,
) -> Result<Option<ChangesetId>, Error> {
    use CommitSyncOutcome::*;

    match maybe_sync_outcome {
        Some(RewrittenAs(cs_id, _)) => Ok(Some(cs_id)),
        Some(NotSyncCandidate(_)) => Ok(None),
        Some(EquivalentWorkingCopyAncestor(cs_id, _)) => Ok(Some(cs_id)),
        None => Err(format_err!(
            "sync outcome is not available for {}",
            source_cs_id
        )),
    }
}

async fn derive_target_hg_changesets(
    ctx: &CoreContext,
    maybe_target_cs_id: Option<ChangesetId>,
    commit_sync_data: &CommitSyncData<Repo>,
) -> Result<(), Error> {
    match maybe_target_cs_id {
        Some(target_cs_id) => {
            let hg_cs_id = commit_sync_data
                .get_target_repo()
                .derive_hg_changeset(ctx, target_cs_id)
                .await?;
            info!(
                ctx.logger(),
                "Hg cs id {} derived for {}", hg_cs_id, target_cs_id
            );
            Ok(())
        }
        None => Ok(()),
    }
}

struct Delay {
    delay_secs: i64,
    remaining_entries: u64,
}

impl Delay {
    fn no_delay() -> Self {
        Self {
            delay_secs: 0,
            remaining_entries: 0,
        }
    }
}

// Returns logs delay and returns the number of remaining bookmark update log entries
async fn calculate_delay(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<Repo>,
    target_repo_dbs: &TargetRepoDbs,
) -> Result<Delay, Error> {
    let TargetRepoDbs { counters, .. } = target_repo_dbs;
    let source_repo_id = commit_sync_data.get_source_repo().repo_identity().id();

    let counter_name = format_counter(&source_repo_id);
    let maybe_counter = counters.get_counter(ctx, &counter_name).await?;
    let counter = maybe_counter
        .ok_or_else(|| format_err!("{} counter not found", counter_name))?
        .try_into()?;
    let source_repo = commit_sync_data.get_source_repo();
    let next_entry = source_repo
        .bookmark_update_log()
        .read_next_bookmark_log_entries(ctx.clone(), counter, 1, Freshness::MostRecent)
        .try_collect::<Vec<_>>();
    let remaining_entries = source_repo
        .bookmark_update_log()
        .count_further_bookmark_log_entries(ctx.clone(), counter, None);

    let (next_entry, remaining_entries) = try_join!(next_entry, remaining_entries)?;
    let delay_secs = next_entry
        .first()
        .map_or(0, |entry| entry.timestamp.since_seconds());

    Ok(Delay {
        delay_secs,
        remaining_entries,
    })
}

fn log_delay(ctx: &CoreContext, delay: &Delay, source_repo_name: &str, target_repo_name: &str) {
    STATS::remaining_entries.set_value(
        ctx.fb,
        delay.remaining_entries as i64,
        (source_repo_name.to_owned(), target_repo_name.to_owned()),
    );
    STATS::delay_secs.set_value(
        ctx.fb,
        delay.delay_secs,
        (source_repo_name.to_owned(), target_repo_name.to_owned()),
    );
}
