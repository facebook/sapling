/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::Context;
use anyhow::Error;
use blobrepo::BlobRepo;
use bytes::Bytes;
use changesets::deserialize_cs_entries;
use clap::Parser;
use cmdlib::helpers;
use context::SessionContainer;
use fbinit::FacebookInit;
use futures::future::join_all;
use futures::stream;
use mononoke_app::args::MultiRepoArgs;
use mononoke_app::args::RepoArg;
use mononoke_app::fb303::AliveService;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use segmented_changelog::seedheads_from_config;
use segmented_changelog::OperationMode;
use segmented_changelog::SegmentedChangelogTailer;
use slog::info;
use slog::o;

/// Updates segmented changelog assets
#[derive(Parser)]
struct SegmentedChangelogTailerArgs {
    /// Repository to warm-up
    #[clap(flatten)]
    repos: MultiRepoArgs,
    /// Repository name to warm-up. Deprecated, use --repo-name/--repo-id instead
    // Deprecated, use repos instead
    #[clap(long = "repo")]
    repo_names: Vec<String>,
    /// When set, the tailer will perform a single incremental build run. If no previous version exists it will perform full reseed instead
    #[clap(long)]
    once: bool,
    /// A file with a serialized list of ChangesetEntry, which can be used to speed up rebuilding of segmented changelog
    #[clap(long)]
    prefetched_commits_path: Option<String>,
    /// What heads to use for Segmented Changelog. If not provided, tailer will use the config to obtain heads
    #[clap(long)]
    head: Vec<String>,
    /// Force use of the configured heads, as well as any specified on the command line
    #[clap(long)]
    include_config_heads: bool,
    /// When set, the tailer will perform a single full reseed run
    #[clap(long, conflicts_with = "once")]
    force_reseed: bool,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = MononokeAppBuilder::new(fb)
        .with_app_extension(Fb303AppExtension {})
        .build::<SegmentedChangelogTailerArgs>()?;

    app.run_with_monitoring_and_logging(async_main, "segmented_changelog_tailer", AliveService)
}

async fn async_main(app: MononokeApp) -> Result<(), Error> {
    let args: SegmentedChangelogTailerArgs = app.args()?;

    let repos = MultiRepoArgs {
        repo_id: args.repos.repo_id,
        repo_name: args
            .repos
            .repo_name
            .into_iter()
            .chain(args.repo_names.into_iter())
            .collect(),
    };

    // This is a bit weird from the dependency point of view but I think that it is best. The
    // BlobRepo may have a SegmentedChangelog attached to it but that doesn't hurt us in any
    // way.  On the other hand reconstructing the dependencies for SegmentedChangelog without
    // BlobRepo is probably prone to more problems from the maintenance perspective.
    let blobrepos: Vec<BlobRepo> = app.open_repos(&repos).await?;

    let prefetched_commits = match args.prefetched_commits_path {
        Some(path) => {
            info!(app.logger(), "reading prefetched commits from {}", path);
            let data = tokio::fs::read(&path).await?;
            deserialize_cs_entries(&Bytes::from(data))
                .with_context(|| format!("failed to parse serialized cs entries from {}", path))?
        }
        None => vec![],
    };

    let (env, logger) = (app.environment(), app.logger());
    let session = SessionContainer::new_with_defaults(env.fb);
    let ctx = session.new_context(logger.clone(), env.scuba_sample_builder.clone());

    let mut tasks = Vec::new();
    for (index, blobrepo) in blobrepos.into_iter().enumerate() {
        let repo_id = blobrepo.get_repoid();
        let (repo_name, config) = app.repo_config(&RepoArg::Id(repo_id))?;
        info!(
            ctx.logger(),
            "repo name '{}' translates to id {}", repo_name, repo_id
        );

        let prefetched_commits = stream::iter(prefetched_commits.iter().filter_map(|entry| {
            if entry.repo_id == repo_id {
                Some(Ok(entry.clone()))
            } else {
                None
            }
        }));

        let ctx = ctx.clone_with_logger(ctx.logger().new(o!("repo_id" => repo_id.to_string())));

        let seed_heads = {
            let mut heads = if args.head.is_empty() || args.include_config_heads {
                let mut heads = seedheads_from_config(
                    &ctx,
                    &config.segmented_changelog_config,
                    segmented_changelog::JobType::Background,
                )?;
                heads.reserve(args.head.len());
                heads
            } else {
                Vec::with_capacity(args.head.len())
            };

            for head_arg in &args.head {
                let head = helpers::csid_resolve(&ctx, blobrepo.clone(), head_arg)
                    .await
                    .with_context(|| {
                        format!("resolving head csid '{}' for repo {}", head_arg, repo_id)
                    })?;
                info!(ctx.logger(), "using '{}' for head", head);
                heads.push(head.into());
            }
            heads
        };

        let segmented_changelog_tailer = SegmentedChangelogTailer::build_from(
            &ctx,
            &blobrepo,
            &config.storage_config.metadata,
            app.mysql_options(),
            seed_heads,
            prefetched_commits,
            None,
        )
        .await?;

        info!(ctx.logger(), "SegmentedChangelogTailer initialized",);

        if args.once {
            segmented_changelog_tailer
                .run(&ctx, OperationMode::SingleIncrementalUpdate)
                .await;
            info!(ctx.logger(), "SegmentedChangelogTailer is done",);
        } else if args.force_reseed {
            segmented_changelog_tailer
                .run(&ctx, OperationMode::ForceReseed)
                .await;
            info!(ctx.logger(), "SegmentedChangelogTailer is done",);
        } else if let Some(period) = config.segmented_changelog_config.tailer_update_period {
            // spread out update operations, start updates on another repo after 7 seconds
            let wait_to_start = Duration::from_secs(7 * index as u64);
            let ctx = ctx.clone();
            tasks.push(async move {
                tokio::time::sleep(wait_to_start).await;
                segmented_changelog_tailer
                    .run(&ctx, OperationMode::ContinousIncrementalUpdate(period))
                    .await;
            });
        }
    }

    join_all(tasks).await;

    Ok(())
}
