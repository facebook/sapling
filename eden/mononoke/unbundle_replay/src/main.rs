/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod hg_recording;
mod hooks;

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobrepo_factory::BlobrepoBuilder;
use blobstore::Loadable;
use bookmarks::{BookmarkName, Freshness};
use clap::{Arg, ArgMatches, SubCommand};
use cmdlib::{args, monitoring::ReadyFlagService};
use context::CoreContext;
use derived_data::BonsaiDerived;
use derived_data_filenodes::FilenodesOnlyPublic;
use fbinit::FacebookInit;
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future,
    stream::{self, Stream, StreamExt},
};
use futures_old::stream::Stream as OldStream;
use mercurial_bundles::bundle2::{Bundle2Stream, StreamEvent};
use mercurial_types::HgChangesetId;
use metaconfig_types::RepoReadOnly;
use mononoke_types::{hash::Blake2, ChangesetId, RawBundle2Id, Timestamp};
use slog::{info, Logger};
use std::collections::HashMap;
use std::convert::TryInto;
use std::io::Cursor;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use unbundle::{
    self, get_pushrebase_hooks, PostResolveAction, PostResolvePushRebase, PushrebaseBookmarkSpec,
};

use crate::hg_recording::{HgRecordingClient, HgRecordingEntry};
use crate::hooks::{Target, UnbundleReplayHook};

const SUBCOMMAND_HG_RECORDING: &str = "hg-recording";
const ARG_HG_RECORDING_ID: &str = "hg-recording-id";

const SUBCOMMAND_HG_BOOKMARK: &str = "hg-bookmark";
const ARG_HG_BOOKMARK_NAME: &str = "hg-bookmark-name";
const ARG_HG_BOOKMARK_POLL_INTERVAL: &str = "poll-interval";

const ARG_HG_BUNDLE_HELPER: &str = "hg-recording-helper";

const SUBCOMMAND_LOG_ENTRY: &str = "log-entry";
const ARG_LOG_ENTRY_ID: &str = "log-entry-id";

struct ReplaySpec {
    bundle: Vec<u8>,
    timestamps: HashMap<HgChangesetId, Timestamp>,
    onto: BookmarkName,
    onto_rev: Option<ChangesetId>,
    target: Target,
}

impl ReplaySpec {
    pub async fn from_hg_recording_entry(
        ctx: &CoreContext,
        repo: &BlobRepo,
        entry: HgRecordingEntry,
    ) -> Result<Self, Error> {
        let HgRecordingEntry {
            id,
            onto,
            onto_rev,
            bundle,
            timestamps,
            revs,
        } = entry;

        let onto_rev = repo
            .get_bonsai_from_hg(ctx.clone(), onto_rev)
            .compat()
            .await?
            .ok_or_else(|| {
                format_err!(
                    "Bonsai changeset missing for {:?} in HgRecordingEntry {}",
                    onto_rev,
                    id
                )
            })?;

        // Wrap this back into an Option, since that's what we want in ReplaySpec. It might be
        // a little weird to unwrap the option then wrap it back, but those are different
        // options: None above means we are missing the Bonsai, None here would mean we want to
        // create the bookmark (which this doesn't support right now).
        let onto_rev = Some(onto_rev);

        let target = Target::hg(
            *revs
                .last()
                .ok_or_else(|| format_err!("Missing target in HgRecordingEntry {}", id))?,
        );

        Ok(ReplaySpec {
            bundle,
            timestamps,
            onto,
            onto_rev,
            target,
        })
    }
}

async fn get_replay_stream<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    matches: &'a ArgMatches<'a>,
) -> Result<impl Stream<Item = Result<ReplaySpec, Error>> + 'a, Error> {
    match matches.subcommand() {
        (SUBCOMMAND_HG_RECORDING, Some(sub)) => {
            let bundle_helper = sub.value_of(ARG_HG_BUNDLE_HELPER).unwrap();
            let bundle_id: i64 = sub.value_of(ARG_HG_RECORDING_ID).unwrap().parse()?;

            let client = HgRecordingClient::new(ctx.fb, bundle_helper, matches).await?;

            let entry = client
                .next_entry_by_id(ctx, bundle_id - 1)
                .await?
                .ok_or_else(|| format_err!("Entry with id {} does not exist", bundle_id))?;

            if entry.id != bundle_id {
                return Err(format_err!("Entry with id {} does not exist", bundle_id));
            }

            let spec = ReplaySpec::from_hg_recording_entry(ctx, repo, entry).await?;

            Ok(stream::once(async { Ok(spec) }).boxed())
        }
        (SUBCOMMAND_HG_BOOKMARK, Some(sub)) => {
            let bundle_helper = sub.value_of(ARG_HG_BUNDLE_HELPER).unwrap();
            let onto = sub.value_of(ARG_HG_BOOKMARK_NAME).unwrap().try_into()?;
            let poll_interval = sub
                .value_of(ARG_HG_BOOKMARK_POLL_INTERVAL)
                .map(|i| i.parse())
                .transpose()?
                .map(Duration::from_secs);

            let client = HgRecordingClient::new(ctx.fb, bundle_helper, matches).await?;

            let onto_rev = repo
                .get_bookmark(ctx.clone(), &onto)
                .compat()
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
                                tokio::time::delay_for(poll_interval).await;
                                continue;
                            }
                        }
                    };

                    match entry {
                        Some(entry) => {
                            let next_onto_rev = *entry.revs.last().ok_or_else(|| {
                                format_err!("Missing target in HgRecordingEntry {}", entry.id)
                            })?;

                            let spec =
                                ReplaySpec::from_hg_recording_entry(ctx, repo, entry).await?;

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
            .boxed())
        }
        (SUBCOMMAND_LOG_ENTRY, Some(sub)) => {
            let id: u64 = sub.value_of(ARG_LOG_ENTRY_ID).unwrap().parse()?;

            info!(ctx.logger(), "Fetching bundle from log entry: {}", id);

            let entry = repo
                .get_bookmarks_object()
                .read_next_bookmark_log_entries(
                    ctx.clone(),
                    id - 1,
                    repo.get_repoid(),
                    1,
                    Freshness::MostRecent,
                )
                .compat()
                .next()
                .await
                .ok_or_else(|| format_err!("Entry with id {} does not exist", id))??;

            if entry.id as u64 != id {
                return Err(format_err!("Entry with id {} does not exist", id));
            }

            let replay_data = entry
                .reason
                .into_bundle_replay_data()
                .ok_or_else(|| format_err!("Entry has replay data"))?;

            info!(
                ctx.logger(),
                "Fetching raw bundle: {}", replay_data.bundle_handle
            );

            let bundle = Blake2::from_str(&replay_data.bundle_handle)
                .map(RawBundle2Id::new)?
                .load(ctx.clone(), repo.blobstore())
                .compat()
                .await?
                .into_bytes()
                .to_vec();

            let spec =
                ReplaySpec {
                    bundle,
                    timestamps: replay_data.commit_timestamps,
                    onto: entry.bookmark_name,
                    onto_rev: entry.from_changeset_id,
                    target: Target::bonsai(entry.to_changeset_id.ok_or_else(|| {
                        format_err!("Replaying bookmark deletions is not supported")
                    })?),
                };

            Ok(stream::once(async { Ok(spec) }).boxed())
        }
        (name, _) => Err(format_err!("Invalid subcommand: {:?}", name)),
    }
}

async fn do_main(
    fb: FacebookInit,
    matches: &ArgMatches<'_>,
    logger: &Logger,
    service: &ReadyFlagService,
) -> Result<(), Error> {
    let mysql_options = args::parse_mysql_options(&matches);
    let blobstore_options = args::parse_blobstore_options(&matches);
    let readonly_storage = args::parse_readonly_storage(&matches);
    let caching = args::init_cachelib(fb, &matches, None);

    let repo_id = args::get_repo_id(fb, matches)?;
    let (repo_name, repo_config) = args::get_config_by_repoid(fb, &matches, repo_id)?;

    info!(
        logger,
        "Loading repository: {} (id = {})", repo_name, repo_id
    );

    let repo = BlobrepoBuilder::new(
        fb,
        repo_name,
        &repo_config,
        mysql_options,
        caching,
        None, // We don't need to log redacted access from here
        readonly_storage,
        blobstore_options,
        &logger,
    )
    .build()
    .await?;

    service.set_ready();

    let mut scuba = args::get_scuba_sample_builder(fb, &matches)?;
    scuba.add_common_server_data();

    // TODO: Would want Scuba and such here.
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let mut stream = get_replay_stream(&ctx, &repo, matches).await?;

    loop {
        let entry = match stream.next().await {
            Some(entry) => entry?,
            None => {
                break;
            }
        };

        let ReplaySpec {
            bundle,
            timestamps,
            onto,
            onto_rev,
            target,
        } = entry;

        info!(
            ctx.logger(),
            "Replaying {}: {:?} -> {:?}", onto, onto_rev, target
        );

        let bundle = Cursor::new(bundle);

        let bundle_stream = Bundle2Stream::new(logger.clone(), bundle).filter_map(|e| match e {
            StreamEvent::Next(item) => Some(item),
            StreamEvent::Done(..) => None,
        });

        let resolution = unbundle::resolve(
            ctx.clone(),
            repo.clone(),
            false, // infinitepush_writes_allowed
            Box::new(bundle_stream),
            RepoReadOnly::ReadWrite,
            None,  // maybe_full_content
            false, // pure_push_allowed
            repo_config.pushrebase.flags,
        )
        .await?;

        // TODO: Run hooks here (this is where repo_client would run them).

        let action = match resolution {
            PostResolveAction::PushRebase(action) => action,
            _ => return Err(format_err!("Unsupported post-resolve action!")),
        };

        let PostResolvePushRebase {
            any_merges: _,
            bookmark_push_part_id: _,
            bookmark_spec,
            maybe_hg_replay_data: _,
            maybe_pushvars: _,
            commonheads: _,
            uploaded_bonsais: changesets,
            uploaded_hg_changeset_ids: _,
        } = action;

        let onto_params = match bookmark_spec {
            PushrebaseBookmarkSpec::NormalPushrebase(onto_params) => onto_params,
            _ => return Err(format_err!("Unsupported bookmark spec")),
        };

        if onto_params.bookmark != onto {
            return Err(format_err!(
                "Expected pushrebase for bookmark {:?}, found {:?}",
                onto,
                onto_params.bookmark
            ));
        }

        let current_cs_id = repo
            .get_bonsai_bookmark(ctx.clone(), &onto_params.bookmark)
            .compat()
            .await?;

        if current_cs_id != onto_rev {
            return Err(format_err!(
                "Expected cs_id for {:?} at {:?}, found {:?}",
                onto_params.bookmark,
                onto_rev,
                current_cs_id
            ));
        }

        // At this point, the commits have have been imported so we can map the timestamps we have to
        // the ones we want.

        let timestamps = future::try_join_all(timestamps.into_iter().map(|(hg_cs_id, ts)| {
            let repo = &repo;
            let ctx = &ctx;
            async move {
                let bonsai_cs_id = repo
                    .get_bonsai_from_hg(ctx.clone(), hg_cs_id)
                    .compat()
                    .await?
                    .ok_or(format_err!(
                        "Hg Changeset is missing after unbundle: {:?}",
                        hg_cs_id
                    ))?;
                Result::<_, Error>::Ok((bonsai_cs_id, ts))
            }
        }))
        .await?
        .into_iter()
        .collect();

        let mut pushrebase_hooks = get_pushrebase_hooks(&repo, &repo_config.pushrebase);

        pushrebase_hooks.push(UnbundleReplayHook::new(
            repo.clone(),
            Arc::new(timestamps),
            target,
        ));

        let head = pushrebase::do_pushrebase_bonsai(
            &ctx,
            &repo,
            &repo_config.pushrebase.flags,
            &onto_params,
            &changesets,
            &None,
            pushrebase_hooks.as_ref(),
        )
        .await?
        .head;

        let cs = head.load(ctx.clone(), repo.blobstore()).compat().await?;

        info!(
            ctx.logger(),
            "Pushrebase completed. New bookmark: {:?}. Age: {}s",
            head,
            Timestamp::from(*cs.author_date()).since_seconds()
        );

        FilenodesOnlyPublic::derive(ctx.clone(), repo.clone(), head)
            .compat()
            .await?;

        info!(ctx.logger(), "Filenodes derived");
    }

    Ok(())
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeApp::new("Mononoke Local Replay")
        .with_advanced_args_hidden()
        .with_fb303_args()
        .with_scuba_logging_args()
        .build()
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

    let matches = app.get_matches();

    let logger = args::init_logging(fb, &matches);
    let service = ReadyFlagService::new();

    let main = do_main(fb, &matches, &logger, &service);

    cmdlib::helpers::block_execute(
        main,
        fb,
        "unbundle_replay",
        &logger,
        &matches,
        service.clone(),
    )?;

    Ok(())
}
