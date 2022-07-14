/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod changesets;
mod filenodes;

use ::changesets::ArcChangesets;
use ::filenodes::ArcFilenodes;
use anyhow::format_err;
use anyhow::Error;
use blobrepo_override::DangerousOverride;
use blobstore_factory::PutBehaviour;
use bookmarks::BookmarkName;
use cache_warmup::CacheWarmupRequest;
use cache_warmup::CacheWarmupTarget;
use clap::Arg;
use clap::SubCommand;
use cloned::cloned;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::monitoring::AliveService;
use context::CoreContext;
use context::SessionContainer;
use derived_data_filenodes::FilenodesOnlyPublic;
use fbinit::FacebookInit;
use futures::channel::mpsc;
use futures::future;
use mercurial_derived_data::MappedHgChangesetId;
use metaconfig_parser::RepoConfigs;
use metaconfig_types::CacheWarmupParams;
use microwave::Snapshot;
use microwave::SnapshotLocation;
use mononoke_api_types::InnerRepo;
use repo_factory::RepoFactory;
use slog::info;
use slog::o;
use slog::Logger;
use std::path::Path;
use std::sync::Arc;
use warm_bookmarks_cache::create_derived_data_warmer;
use warm_bookmarks_cache::find_all_underived_and_latest_derived;
use warm_bookmarks_cache::LatestDerivedBookmarkEntry;

use crate::changesets::MicrowaveChangesets;
use crate::filenodes::MicrowaveFilenodes;

const SUBCOMMAND_LOCAL_PATH: &str = "local-path";
const ARG_LOCAL_PATH: &str = "local-path";

const SUBCOMMAND_BLOBSTORE: &str = "blobstore";

async fn cache_warmup_target(
    ctx: &CoreContext,
    repo: &InnerRepo,
    bookmark: &BookmarkName,
) -> Result<CacheWarmupTarget, Error> {
    let warmers = vec![
        create_derived_data_warmer::<MappedHgChangesetId, _>(ctx, repo),
        create_derived_data_warmer::<FilenodesOnlyPublic, _>(ctx, repo),
    ];

    match find_all_underived_and_latest_derived(ctx, repo, bookmark, &warmers)
        .await?
        .0
    {
        LatestDerivedBookmarkEntry::Found(Some((cs_id, _))) => {
            Ok(CacheWarmupTarget::Changeset(cs_id))
        }
        LatestDerivedBookmarkEntry::Found(None) => {
            Err(format_err!("Bookmark {} has no derived data", bookmark))
        }
        LatestDerivedBookmarkEntry::NotFound => Err(format_err!(
            "Bookmark {} has too many underived commits",
            bookmark
        )),
    }
}

async fn do_main<'a>(
    fb: FacebookInit,
    matches: &MononokeMatches<'a>,
    logger: &Logger,
) -> Result<(), Error> {
    let scuba = matches.scuba_sample_builder();

    let config_store = matches.config_store();

    let RepoConfigs { repos, common } = args::load_repo_configs(config_store, matches)?;

    let location = match matches.subcommand() {
        (SUBCOMMAND_LOCAL_PATH, Some(sub)) => {
            let path = Path::new(sub.value_of_os(ARG_LOCAL_PATH).unwrap());
            info!(logger, "Writing to path {}", path.display());
            SnapshotLocation::SharedLocalPath(path)
        }
        (SUBCOMMAND_BLOBSTORE, Some(_)) => SnapshotLocation::Blobstore,
        (name, _) => return Err(format_err!("Invalid subcommand: {:?}", name)),
    };

    let repo_factory = Arc::new(RepoFactory::new(matches.environment().clone(), &common));

    let futs = repos
        .into_iter()
        .map(|(name, config)| {
            cloned!(repo_factory, mut scuba);

            async move {
                let logger = logger.new(o!("repo" => name.clone()));

                let ctx = {
                    scuba.add("reponame", name.clone());
                    let session = SessionContainer::new_with_defaults(fb);
                    session.new_context(logger.clone(), scuba)
                };

                let (filenodes_sender, filenodes_receiver) = mpsc::channel(1000);
                let (changesets_sender, changesets_receiver) = mpsc::channel(1000);
                let warmup_ctx = ctx.clone();

                let warmup = async move {
                    let cache_warmup = config.cache_warmup.clone();
                    let repo: InnerRepo = repo_factory.build(name, config).await?;

                    // Rewind bookmarks to the point where we have derived data. Cache
                    // warmup requires filenodes and hg changesets to be present.
                    let req = match cache_warmup {
                        Some(params) => {
                            let CacheWarmupParams {
                                bookmark,
                                commit_limit,
                                microwave_preload,
                            } = params;

                            let target = cache_warmup_target(&warmup_ctx, &repo, &bookmark).await?;

                            Some(CacheWarmupRequest {
                                target,
                                commit_limit,
                                microwave_preload,
                            })
                        }
                        None => None,
                    };

                    let warmup_repo = repo
                        .blob_repo
                        .dangerous_override(|inner| -> ArcFilenodes {
                            Arc::new(MicrowaveFilenodes::new(filenodes_sender, inner))
                        })
                        .dangerous_override(|inner| -> ArcChangesets {
                            Arc::new(MicrowaveChangesets::new(changesets_sender, inner))
                        });

                    cache_warmup::cache_warmup(&warmup_ctx, &warmup_repo, req).await?;

                    Result::<_, Error>::Ok(repo)
                };

                let handle = tokio::task::spawn(warmup);
                let snapshot = Snapshot::build(filenodes_receiver, changesets_receiver).await;

                // Make sure cache warmup has succeeded before committing this snapshot, and get
                // the repo back.
                let repo = handle.await??;

                snapshot.commit(&ctx, &repo.blob_repo, location).await?;

                Result::<_, Error>::Ok(())
            }
        })
        .collect::<Vec<_>>();

    future::try_join_all(futs).await?;

    Ok(())
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeAppBuilder::new("Mononoke Local Replay")
        .with_advanced_args_hidden()
        .with_fb303_args()
        .with_all_repos()
        .with_scuba_logging_args()
        .with_special_put_behaviour(PutBehaviour::Overwrite)
        .build()
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_LOCAL_PATH)
                .about("Write cache priming data to path")
                .arg(
                    Arg::with_name(ARG_LOCAL_PATH)
                        .takes_value(true)
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_BLOBSTORE)
                .about("Write cache priming data to the repository blobstore"),
        );

    let matches = app.get_matches(fb)?;

    let logger = matches.logger();

    let main = do_main(fb, &matches, logger);

    cmdlib::helpers::block_execute(main, fb, "microwave", logger, &matches, AliveService)?;

    Ok(())
}
