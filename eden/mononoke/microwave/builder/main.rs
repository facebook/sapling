/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod changesets;
mod filenodes;

use std::path::PathBuf;
use std::sync::Arc;

use ::changesets::ArcChangesets;
use ::filenodes::ArcFilenodes;
use anyhow::format_err;
use anyhow::Error;
use blobrepo_override::DangerousOverride;
use blobstore_factory::BlobstoreArgDefaults;
use blobstore_factory::PutBehaviour;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::BookmarksRef;
use cache_warmup::CacheWarmupRequest;
use cache_warmup::CacheWarmupTarget;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use cloned::cloned;
use context::CoreContext;
use context::SessionContainer;
use derived_data_filenodes::FilenodesOnlyPublic;
use fbinit::FacebookInit;
use futures::channel::mpsc;
use futures::future;
use mercurial_derived_data::MappedHgChangesetId;
use metaconfig_types::CacheWarmupParams;
use microwave::Snapshot;
use microwave::SnapshotLocation;
use mononoke_api_types::InnerRepo;
use mononoke_app::fb303::AliveService;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use repo_derived_data::RepoDerivedDataArc;
use slog::info;
use slog::o;
use warm_bookmarks_cache::create_derived_data_warmer;
use warm_bookmarks_cache::find_all_underived_and_latest_derived;
use warm_bookmarks_cache::LatestDerivedBookmarkEntry;

use crate::changesets::MicrowaveChangesets;
use crate::filenodes::MicrowaveFilenodes;

async fn cache_warmup_target(
    ctx: &CoreContext,
    repo: &InnerRepo,
    bookmark: &BookmarkName,
) -> Result<CacheWarmupTarget, Error> {
    let warmers = vec![
        create_derived_data_warmer::<MappedHgChangesetId>(ctx, repo.repo_derived_data_arc()),
        create_derived_data_warmer::<FilenodesOnlyPublic>(ctx, repo.repo_derived_data_arc()),
    ];

    match find_all_underived_and_latest_derived(
        ctx,
        repo.bookmarks(),
        repo.bookmark_update_log(),
        bookmark,
        &warmers,
    )
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

async fn async_main(app: MononokeApp) -> Result<(), Error> {
    let env = app.environment();
    let logger = app.logger();
    let args: MononokeMicrowaveArgs = app.args()?;

    let repo_factory = Arc::clone(app.repo_factory());
    let scuba = env.scuba_sample_builder.clone();

    let repos = app.repo_configs().repos.clone();

    let location = match &args.command {
        Commands::LocalPath(local_path_args) => {
            let path = &local_path_args.local_path;
            info!(logger, "Writing to path {}", path.display());
            SnapshotLocation::SharedLocalPath(path.as_path())
        }
        Commands::Blobstore => SnapshotLocation::Blobstore,
    };
    let common_config = &app.repo_configs().common;
    let futs = repos
        .into_iter()
        .map(|(name, config)| {
            cloned!(repo_factory, mut scuba, common_config);
            async move {
                let logger = logger.new(o!("repo" => name.clone()));
                let ctx = {
                    scuba.add("reponame", name.clone());
                    let session = SessionContainer::new_with_defaults(app.fb);
                    session.new_context(logger.clone(), scuba)
                };

                let (filenodes_sender, filenodes_receiver) = mpsc::channel(1000);
                let (changesets_sender, changesets_receiver) = mpsc::channel(1000);
                let warmup_ctx = ctx.clone();

                let warmup = async move {
                    let cache_warmup = config.cache_warmup.clone();
                    let repo: InnerRepo = repo_factory.build(name, config, common_config).await?;

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

#[derive(Parser)]
#[clap(name = "Mononoke Local Replay")]
struct MononokeMicrowaveArgs {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[clap(name = "local-path", about = "Write cache priming data to path")]
    LocalPath(LocalPath),
    #[clap(
        name = "blobstore",
        about = "Write cache priming data to the repository blobstore"
    )]
    Blobstore,
}

#[derive(Args)]
struct LocalPath {
    #[clap(name = "local-path", value_parser)]
    local_path: PathBuf,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = MononokeAppBuilder::new(fb)
        .with_app_extension(Fb303AppExtension {})
        .with_arg_defaults(BlobstoreArgDefaults {
            put_behaviour: Some(PutBehaviour::Overwrite),
            ..Default::default()
        })
        .build::<MononokeMicrowaveArgs>()?;

    app.run_with_monitoring_and_logging(async_main, "microwave", AliveService)
}
