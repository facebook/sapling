/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod hg_recording;
mod hooks;
mod replay_spec;

use ::hooks::{hook_loader::load_hooks, HookManager};
use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bookmarks::{BookmarkName, Freshness};
use bytes::Bytes;
use clap::{Arg, SubCommand};
use cmdlib::{
    args::{self, MononokeMatches},
    monitoring::ReadyFlagService,
};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{
    compat::Stream01CompatExt,
    future,
    stream::{self, Stream, StreamExt, TryStreamExt},
};
use futures_old::stream::Stream as OldStream;
use futures_stats::{FutureStats, TimedFutureExt};
use hooks_content_stores::blobrepo_text_only_fetcher;
use mercurial_bundles::bundle2::{Bundle2Stream, StreamEvent};
use metaconfig_types::{BookmarkAttrs, RepoConfig};
use mononoke_types::{BonsaiChangeset, ChangesetId, Timestamp};
use repo_factory::RepoFactory;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::{info, warn, Logger};
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;
use time_ext::DurationExt;
use tokio::{task, time};
use unbundle::{
    self, run_hooks, CrossRepoPushSource, PostResolveAction, PostResolvePushRebase,
    PushrebaseBookmarkSpec,
};

use crate::hg_recording::HgRecordingClient;
use crate::hooks::{Target, UnbundleReplayHook};
use crate::replay_spec::{OntoRev, PushrebaseSpec, ReplaySpec};

const ARG_UNBUNDLE_CONCURRENCY: &str = "unbundle-concurrency";
const ARG_RUN_HOOKS: &str = "run-hooks";

const SUBCOMMAND_HG_RECORDING: &str = "hg-recording";
const ARG_HG_RECORDING_ID: &str = "hg-recording-id";

const SUBCOMMAND_HG_BOOKMARK: &str = "hg-bookmark";
const ARG_HG_BOOKMARK_NAME: &str = "hg-bookmark-name";
const ARG_HG_BOOKMARK_POLL_INTERVAL: &str = "poll-interval";

const ARG_HG_BUNDLE_HELPER: &str = "hg-recording-helper";

const SUBCOMMAND_LOG_ENTRY: &str = "log-entry";
const ARG_LOG_ENTRY_ID: &str = "log-entry-id";

async fn get_replay_stream<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    matches: &'a MononokeMatches<'a>,
) -> Result<impl Stream<Item = Result<ReplaySpec, Error>> + 'a, Error> {
    let config_store = matches.config_store();

    match matches.subcommand() {
        (SUBCOMMAND_HG_RECORDING, Some(sub)) => {
            let bundle_helper: String = sub.value_of(ARG_HG_BUNDLE_HELPER).unwrap().into();
            let bundle_id: i64 = sub.value_of(ARG_HG_RECORDING_ID).unwrap().parse()?;

            let client = HgRecordingClient::new(ctx.fb, config_store, matches)?;

            let entry = client
                .next_entry_by_id(ctx, bundle_id - 1)
                .await?
                .ok_or_else(|| format_err!("Entry with id {} does not exist", bundle_id))?;

            if entry.id != bundle_id {
                return Err(format_err!("Entry with id {} does not exist", bundle_id));
            }

            let spec = ReplaySpec::from_hg_recording_entry(bundle_helper, entry)?;
            Ok(stream::once(future::ok(spec)).right_stream())
        }
        (SUBCOMMAND_HG_BOOKMARK, Some(sub)) => {
            let bundle_helper: String = sub.value_of(ARG_HG_BUNDLE_HELPER).unwrap().into();
            let onto: BookmarkName = sub.value_of(ARG_HG_BOOKMARK_NAME).unwrap().try_into()?;
            let poll_interval: Option<Duration> = sub
                .value_of(ARG_HG_BOOKMARK_POLL_INTERVAL)
                .map(|i| i.parse())
                .transpose()?
                .map(Duration::from_secs);

            let client = HgRecordingClient::new(ctx.fb, config_store, matches)?;

            let onto_rev = repo
                .get_bookmark(ctx.clone(), &onto)
                .await?
                .ok_or_else(|| format_err!("Bookmark does not exist: {}", onto))?;

            info!(
                ctx.logger(),
                "Loading hg bookmark updates for bookmark {}, starting at {}", onto, onto_rev
            );

            let state = Arc::new((client, onto));

            Ok(stream::try_unfold(onto_rev, move |onto_rev| {
                // NOTE: We need to wrap the state in an Arc here, because while our stream itself
                // can have a lifetime bound by 'a, the futures we return from this closure cannot
                // have their lifetime constrained by that of said closure, which effectively means
                // we have nowhere to put client and onto (we'd normally want to put them in the
                // closure, but we can't do that because then they wouldn't live enough for the
                // futures we put in -- this is why we can have &ctx and &repo as pointers but need
                // an Arc for those). If this wasn't a stream, we'd just use `async move { ... }`,
                // but there isn't an equivalent for streams. Besides, considering that the future
                // returned by `next()` on a stream doesn't have a lifetime bound by the lifetime
                // of the stream, it seems like this might be simply not possible.
                let state = state.clone();
                let bundle_helper = bundle_helper.clone();
                async move {
                    let (client, onto) = state.as_ref();
                    let entry = loop {
                        let entry = client.next_entry_by_onto(&ctx, &onto, &onto_rev).await?;

                        match (poll_interval, entry) {
                            (None, entry) => {
                                // If we have no poll interval, then return the entry, regardless
                                // of whether we have one.
                                break entry;
                            }
                            (_, Some(entry)) => {
                                // If we do have an entry, then it doesn't matter what the poll
                                // interval is, we can go with that.
                                break Some(entry);
                            }
                            (Some(poll_interval), None) => {
                                // If we have a poll interval, but no entry, then let's wait.
                                info!(
                                    ctx.logger(),
                                    "Waiting {:?} for hg bookmark update for bookmark {} at {}",
                                    poll_interval,
                                    onto,
                                    onto_rev
                                );
                                time::sleep(poll_interval).await;
                                continue;
                            }
                        }
                    };

                    match entry {
                        Some(entry) => {
                            let next_onto_rev = *entry.revs.last().ok_or_else(|| {
                                format_err!("Missing target in HgRecordingEntry {}", entry.id)
                            })?;

                            let spec = ReplaySpec::from_hg_recording_entry(bundle_helper, entry)?;

                            Ok(Some((spec, next_onto_rev)))
                        }
                        None => {
                            info!(
                                ctx.logger(),
                                "No further hg bookmark updates for bookmark {} at {}",
                                onto,
                                onto_rev
                            );
                            Ok(None)
                        }
                    }
                }
            })
            .left_stream())
        }
        (SUBCOMMAND_LOG_ENTRY, Some(sub)) => {
            let id: u64 = sub.value_of(ARG_LOG_ENTRY_ID).unwrap().parse()?;

            info!(ctx.logger(), "Fetching bundle from log entry: {}", id);

            let entry = repo
                .bookmark_update_log()
                .read_next_bookmark_log_entries(ctx.clone(), id - 1, 1, Freshness::MostRecent)
                .next()
                .await
                .ok_or_else(|| format_err!("Entry with id {} does not exist", id))??;

            if entry.id as u64 != id {
                return Err(format_err!("Entry with id {} does not exist", id));
            }

            let spec = ReplaySpec::from_bookmark_update_log_entry(entry)?;

            Ok(stream::once(future::ok(spec)).right_stream())
        }
        (name, _) => Err(format_err!("Invalid subcommand: {:?}", name)),
    }
}

struct UnbundleComplete {
    onto_bookmark: BookmarkName,
    onto_rev: Option<OntoRev>,
    target: Target,
    timestamps: HashMap<ChangesetId, Timestamp>,
    changesets: HashSet<BonsaiChangeset>,
    unbundle_stats: FutureStats,
    hooks_outcome: Option<(FutureStats, Option<Error>)>,
    recorded_duration: Option<Duration>,
}

enum UnbundleOutcome {
    /// This unbundle has completed, and can be pushrebased.
    Complete(UnbundleComplete),
    /// This unbundle failed, likely because it depended on commits that haven't been pushrebased
    /// yet. Re-run it before starting pushrebasee.
    Deferred(Bytes, PushrebaseSpec, Error),
}

async fn maybe_unbundle(
    ctx: &CoreContext,
    repo: &BlobRepo,
    repo_config: &RepoConfig,
    hook_manager: &Option<Arc<HookManager>>,
    bundle: Bytes,
    pushrebase_spec: PushrebaseSpec,
) -> Result<UnbundleOutcome, Error> {
    info!(
        ctx.logger(),
        "Unbundle starting: {}: {:?} -> {:?}",
        pushrebase_spec.onto,
        pushrebase_spec.onto_rev,
        pushrebase_spec.target
    );

    let bundle_stream = Bundle2Stream::new(ctx.logger().clone(), Cursor::new(bundle.clone()))
        .filter_map(|e| match e {
            StreamEvent::Next(item) => Some(item),
            StreamEvent::Done(..) => None,
        });

    let (unbundle_stats, resolution) = task::spawn({
        let ctx = ctx.clone();
        let repo = repo.clone();
        let pushrebase_flags = repo_config.pushrebase.flags;
        async move {
            unbundle::resolve(
                &ctx,
                &repo,
                false, // infinitepush_writes_allowed
                bundle_stream.compat().boxed(),
                None,  // maybe_full_content
                false, // pure_push_allowed
                pushrebase_flags,
                None, // No backup repo source,
            )
            .await
        }
    })
    .timed()
    .await;

    let resolution = match resolution {
        Ok(Ok(resolution)) => resolution,
        Ok(Err(e)) => return Ok(UnbundleOutcome::Deferred(bundle, pushrebase_spec, e.into())),
        Err(e) => return Err(e.into()),
    };

    let hooks_outcome = match hook_manager {
        Some(hook_manager) => {
            // Note: the use of `NativeToThisRepo` below means that we don't
            //       support `unbundle_replay` in repos with push-redirection,
            //       as these might have accepted push-redirected commits, which
            //       would've been rejected by the `NativeToThisRepo` hooks
            let (hook_stats, hooks_outcome) = run_hooks(
                ctx,
                repo,
                hook_manager,
                &resolution,
                CrossRepoPushSource::NativeToThisRepo,
            )
            .timed()
            .await;

            Some((hook_stats, hooks_outcome.err().map(Error::from)))
        }
        None => None,
    };

    let PushrebaseSpec {
        onto,
        onto_rev,
        target,
        timestamps,
        recorded_duration,
    } = pushrebase_spec;

    // TODO: Run hooks here (this is where repo_client would run them).

    let action = match resolution {
        PostResolveAction::PushRebase(action) => action,
        _ => return Err(format_err!("Unsupported post-resolve action!")),
    };

    let PostResolvePushRebase {
        bookmark_push_part_id: _,
        bookmark_spec,
        maybe_hg_replay_data: _,
        maybe_pushvars: _,
        commonheads: _,
        uploaded_bonsais: changesets,
        hook_rejection_remapper: _,
    } = action;

    let onto_bookmark = match bookmark_spec {
        PushrebaseBookmarkSpec::NormalPushrebase(onto_bookmark) => onto_bookmark,
        _ => return Err(format_err!("Unsupported bookmark spec")),
    };

    if onto_bookmark != onto {
        return Err(format_err!(
            "Expected pushrebase for bookmark {:?}, found {:?}",
            onto,
            onto_bookmark
        ));
    }

    // At this point, the Hg commits have have been imported so we can map the timestamps we have
    // (Hg) to the ones we want (Bnsai).

    let timestamps = stream::iter(
        timestamps
            .into_iter()
            .map(|(hg_cs_id, ts)| async move {
                let bonsai_cs_id = repo
                    .bonsai_hg_mapping()
                    .get_bonsai_from_hg(ctx, hg_cs_id)
                    .await?
                    .ok_or(format_err!(
                        "Hg Changeset is missing after unbundle: {:?}",
                        hg_cs_id
                    ))?;
                Result::<_, Error>::Ok((bonsai_cs_id, ts))
            })
            .map(Ok),
    )
    .try_buffer_unordered(10)
    .try_collect()
    .await?;

    Ok(UnbundleOutcome::Complete(UnbundleComplete {
        onto_bookmark,
        onto_rev,
        target,
        timestamps,
        changesets,
        unbundle_stats,
        hooks_outcome,
        recorded_duration,
    }))
}

async fn do_main(
    fb: FacebookInit,
    matches: &MononokeMatches<'_>,
    logger: &Logger,
    service: &ReadyFlagService,
) -> Result<(), Error> {
    // TODO: Would want Scuba and such here.
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let config_store = matches.config_store();

    let unbundle_concurrency = matches
        .value_of(ARG_UNBUNDLE_CONCURRENCY)
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(1_usize);

    let repo_id = args::get_repo_id(config_store, matches)?;
    let (repo_name, repo_config) = args::get_config_by_repoid(config_store, &matches, repo_id)?;

    info!(
        logger,
        "Loading repository: {} (id = {})", repo_name, repo_id
    );
    let config = args::load_repo_configs(&config_store, &matches)?;

    let repo_factory = RepoFactory::new(matches.environment().clone(), &config.common);

    let repo: BlobRepo = repo_factory.build(repo_name, repo_config.clone()).await?;

    let hook_manager = if matches.is_present(ARG_RUN_HOOKS) {
        info!(logger, "Creating HookManager");
        let mut hook_manager = HookManager::new(
            ctx.fb,
            blobrepo_text_only_fetcher(repo.clone(), repo_config.hook_max_file_size),
            repo_config.hook_manager_params.clone().unwrap_or_default(),
            MononokeScubaSampleBuilder::with_discard(),
            repo.name().clone(),
        )
        .await?;

        info!(logger, "Loading hooks");
        load_hooks(fb, &mut hook_manager, repo_config.clone(), &HashSet::new()).await?;
        Some(Arc::new(hook_manager))
    } else {
        None
    };

    service.set_ready();

    let scuba = matches.scuba_sample_builder();

    let ctx = &ctx;
    let scuba = &scuba;
    let repo = &repo;
    let repo_config = &repo_config;
    let hook_manager = &hook_manager;

    get_replay_stream(&ctx, &repo, matches)
        .await?
        .map(|spec| async move {
            match spec {
                Ok(spec) => {
                    let ReplaySpec {
                        bundle,
                        pushrebase_spec,
                    } = spec;

                    let bundle = bundle.load(ctx, repo).await?;

                    maybe_unbundle(
                        ctx,
                        repo,
                        repo_config,
                        hook_manager,
                        bundle,
                        pushrebase_spec,
                    )
                    .await
                }
                Err(e) => Err(e),
            }
        })
        .buffered(unbundle_concurrency)
        .and_then(|outcome| async move {
            // Our unbundle will hopefully have succeeded, but if it depended on any data being
            // produced by a previous pushrebase, it could have failed. In that case, retry the
            // deferred unbundle now, in the unbuffered pushrebase section of the stream.
            let unbundle_complete = match outcome {
                UnbundleOutcome::Complete(c) => (c),
                UnbundleOutcome::Deferred(bundle, pushrebase_spec, _) => {
                    warn!(
                        ctx.logger(),
                        "Retrying deferred unbundle: {}: {:?} -> {:?}",
                        pushrebase_spec.onto,
                        pushrebase_spec.onto_rev,
                        pushrebase_spec.target
                    );

                    match maybe_unbundle(
                        ctx,
                        repo,
                        repo_config,
                        hook_manager,
                        bundle,
                        pushrebase_spec,
                    )
                    .await?
                    {
                        UnbundleOutcome::Complete(c) => c,
                        UnbundleOutcome::Deferred(_, _, err) => {
                            return Err(err);
                        }
                    }
                }
            };

            let UnbundleComplete {
                onto_bookmark,
                onto_rev,
                target,
                timestamps,
                changesets,
                unbundle_stats,
                hooks_outcome,
                recorded_duration,
            } = unbundle_complete;

            let onto_rev = match onto_rev {
                Some(onto_rev) => Some(onto_rev.into_cs_id(ctx, repo).await?),
                None => None,
            };

            let current_cs_id = repo
                .get_bonsai_bookmark(ctx.clone(), &onto_bookmark)
                .await?;

            if current_cs_id != onto_rev {
                return Err(format_err!(
                    "Expected cs_id for {:?} at {:?}, found {:?}",
                    onto_bookmark,
                    onto_rev,
                    current_cs_id
                ));
            }

            info!(
                ctx.logger(),
                "Pushrebase starting: {}: {:?} -> {:?}", onto_bookmark, onto_rev, target
            );

            let bookmark_attrs = BookmarkAttrs::new(fb, repo_config.bookmarks.clone()).await?;
            let mut pushrebase_hooks = bookmarks_movement::get_pushrebase_hooks(
                ctx,
                repo,
                &onto_bookmark,
                &bookmark_attrs,
                &repo_config.pushrebase,
            )?;

            pushrebase_hooks.push(UnbundleReplayHook::new(
                repo.clone(),
                Arc::new(timestamps),
                target,
            ));

            let (pushrebase_stats, res) = pushrebase::do_pushrebase_bonsai(
                ctx,
                repo,
                &repo_config.pushrebase.flags,
                &onto_bookmark,
                &changesets,
                None,
                pushrebase_hooks.as_ref(),
            )
            .timed()
            .await;

            let head = res?.head;

            let cs = head.load(&ctx, repo.blobstore()).await?;

            let age = Timestamp::from(*cs.author_date()).since_seconds();

            let file_count = changesets
                .iter()
                .fold(0, |acc, c| acc + c.file_changes_map().len());

            let mut scuba = scuba.clone();
            scuba.add("unbundle_file_count", file_count);
            scuba.add("unbundle_changeset_count", changesets.len());
            scuba.add(
                "unbundle_completion_time_us",
                unbundle_stats.completion_time.as_micros_unchecked(),
            );
            scuba.add(
                "pushrebase_completion_time_us",
                pushrebase_stats.completion_time.as_micros_unchecked(),
            );
            scuba.add("age_s", age);
            scuba.add("bookmark", onto_bookmark.to_string());
            scuba.add("to_cs_id", head.to_string());
            if let Some(current_cs_id) = current_cs_id {
                scuba.add("from_cs_id", current_cs_id.to_string());
            }
            if let Some(recorded_duration) = recorded_duration {
                scuba.add(
                    "pushrebase_recorded_time_us",
                    recorded_duration.as_micros_unchecked(),
                );
            }
            if let Some((hooks_stats, maybe_hooks_err)) = hooks_outcome {
                scuba.add(
                    "hooks_execution_time_us",
                    hooks_stats.completion_time.as_micros_unchecked(),
                );
                if let Some(hooks_err) = maybe_hooks_err {
                    scuba.add("hooks_error", hooks_err.to_string());
                }
            }
            scuba.log();

            info!(
                ctx.logger(),
                "Pushrebase completed: {}: {:?} -> {:?} (age: {}s)",
                onto_bookmark,
                current_cs_id,
                head,
                age,
            );

            Ok(())
        })
        .try_for_each(|()| future::ready(Ok(())))
        .await?;

    Ok(())
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeAppBuilder::new("Mononoke Local Replay")
        .with_advanced_args_hidden()
        .with_fb303_args()
        .with_scuba_logging_args()
        .build()
        .arg(
            Arg::with_name(ARG_UNBUNDLE_CONCURRENCY)
                .help("How many unbundles to attempt to process in parallel")
                .long(ARG_UNBUNDLE_CONCURRENCY)
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_RUN_HOOKS)
                .help("Whether to run hooks")
                .long(ARG_RUN_HOOKS)
                .takes_value(false)
                .required(false),
        )
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_HG_RECORDING)
                .about("Replay a single bundle, from hg")
                .arg(
                    Arg::with_name(ARG_HG_BUNDLE_HELPER)
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name(ARG_HG_RECORDING_ID)
                        .takes_value(true)
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_HG_BOOKMARK)
                .about(
                    "Replay a single bookmark, from hg. This is not \
                    guaranteed to work if there are multiple bookmarks \
                    (if commit A is introduced in another bookmark, \
                    then depended on by commit B that is in this bookmark, \
                    it will fail).",
                )
                .arg(
                    Arg::with_name(ARG_HG_BOOKMARK_POLL_INTERVAL)
                        .help(
                            "How frequently to poll for updates if none are found, in seconds. \
                             If unset, the sync will exit once no more entries are found.",
                        )
                        .long(ARG_HG_BOOKMARK_POLL_INTERVAL)
                        .takes_value(true)
                        .required(false),
                )
                .arg(
                    Arg::with_name(ARG_HG_BUNDLE_HELPER)
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name(ARG_HG_BOOKMARK_NAME)
                        .takes_value(true)
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_LOG_ENTRY)
                .about("Replay a single bundle, from a bookmark updates log log entry")
                .arg(
                    Arg::with_name(ARG_LOG_ENTRY_ID)
                        .takes_value(true)
                        .required(true),
                ),
        );

    let matches = app.get_matches(fb)?;

    let logger = matches.logger();
    let service = ReadyFlagService::new();

    let main = do_main(fb, &matches, logger, &service);

    cmdlib::helpers::block_execute(
        main,
        fb,
        "unbundle_replay",
        logger,
        &matches,
        service.clone(),
    )?;

    Ok(())
}
