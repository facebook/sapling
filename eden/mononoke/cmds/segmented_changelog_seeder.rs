/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::sync::Arc;

use anyhow::{Context, Error};
use blobrepo::BlobRepo;
use blobstore_factory::{make_metadata_sql_factory, ReadOnlyStorage};
use bookmarks::{BookmarkName, BookmarksArc};
use bytes::Bytes;
use changeset_fetcher::PrefetchedChangesetsFetcher;
use changesets::{deserialize_cs_entries, ChangesetsArc};
use clap::Arg;
use cmdlib::{
    args::{self, MononokeMatches},
    helpers,
};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream;
use metaconfig_types::MetadataDatabaseConfig;
use segmented_changelog::types::IdMapVersion;
use segmented_changelog::{SeedHead, SegmentedChangelogSeeder, SegmentedChangelogSqlConnections};
use slog::info;
use sql_ext::facebook::MyAdmin;
use sql_ext::replication::{NoReplicaLagMonitor, ReplicaLagMonitor};

const ARG_PREFETCHED_COMMITS_PATH: &str = "prefetched-commits-path";
const IDMAP_VERSION_ARG: &str = "idmap-version";
const HEAD_ARG: &str = "head";

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeAppBuilder::new("Builds a new version of segmented changelog.")
        .with_advanced_args_hidden()
        .with_fb303_args()
        .build()
        .about("Builds a new version of segmented changelog.")
        .arg(
            Arg::with_name(IDMAP_VERSION_ARG)
                .long(IDMAP_VERSION_ARG)
                .takes_value(true)
                .required(false)
                .help("What version to label the new idmap with."),
        )
        .arg(
            Arg::with_name(HEAD_ARG)
                .long(HEAD_ARG)
                .takes_value(true)
                .help("What head to use for Segmented Changelog."),
        )
        .arg(
            Arg::with_name(ARG_PREFETCHED_COMMITS_PATH)
                .long(ARG_PREFETCHED_COMMITS_PATH)
                .takes_value(true)
                .required(false)
                .help(
                    "a file with a serialized list of ChangesetEntry, \
                which can be used to speed up rebuilding of segmented changelog",
                ),
        );
    let matches = app.get_matches(fb)?;

    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    helpers::block_execute(
        run(ctx, &matches),
        fb,
        &std::env::var("TW_JOB_NAME").unwrap_or_else(|_| "segmented_changelog_seeder".to_string()),
        &logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}

async fn run<'a>(ctx: CoreContext, matches: &'a MononokeMatches<'a>) -> Result<(), Error> {
    let idmap_version_arg: Option<u64> = args::get_u64_opt(matches, IDMAP_VERSION_ARG);
    let config_store = matches.config_store();

    // This is a bit weird from the dependency point of view but I think that it is best. The
    // BlobRepo may have a SegmentedChangelog attached to it but that doesn't hurt us in any way.
    // On the other hand reconstructing the dependencies for SegmentedChangelog without BlobRepo is
    // probably prone to more problems from the maintenance perspective.
    let repo: BlobRepo = args::open_repo(ctx.fb, ctx.logger(), &matches)
        .await
        .context("opening repo")?;

    let mysql_options = matches.mysql_options();
    let (_, config) = args::get_config(config_store, matches)?;
    let storage_config = config.storage_config;
    let readonly_storage = ReadOnlyStorage(false);

    let db_address = match &storage_config.metadata {
        MetadataDatabaseConfig::Local(_) => None,
        MetadataDatabaseConfig::Remote(remote_config) => {
            Some(remote_config.primary.db_address.clone())
        }
    };
    let replica_lag_monitor: Arc<dyn ReplicaLagMonitor> = match db_address {
        None => Arc::new(NoReplicaLagMonitor()),
        Some(address) => {
            let my_admin = MyAdmin::new(ctx.fb).context("building myadmin client")?;
            Arc::new(my_admin.single_shard_lag_monitor(address))
        }
    };

    let sql_factory = make_metadata_sql_factory(
        ctx.fb,
        storage_config.metadata,
        mysql_options.clone(),
        readonly_storage,
    )
    .await
    .context("constructing metadata sql factory")?;

    let segmented_changelog_sql_connections = sql_factory
        .open::<SegmentedChangelogSqlConnections>()
        .context("error opening segmented changelog sql connections")?;

    let prefetched_commits = match matches.value_of(ARG_PREFETCHED_COMMITS_PATH) {
        Some(path) => {
            info!(ctx.logger(), "reading prefetched commits from {}", path);
            let data = tokio::fs::read(path).await?;
            deserialize_cs_entries(&Bytes::from(data))
                .with_context(|| format!("failed to parse serialized cs entries from {}", path))?
        }
        None => vec![],
    };

    let repo_id = repo.get_repoid();
    let changeset_fetcher = Arc::new(
        PrefetchedChangesetsFetcher::new(
            repo_id,
            repo.changesets_arc(),
            stream::iter(prefetched_commits.iter().filter_map(|entry| {
                if entry.repo_id == repo_id {
                    Some(Ok(entry.clone()))
                } else {
                    None
                }
            })),
        )
        .await?,
    );

    let segmented_changelog_seeder = SegmentedChangelogSeeder::new(
        repo_id,
        segmented_changelog_sql_connections,
        replica_lag_monitor,
        Arc::new(repo.get_blobstore()),
        changeset_fetcher,
        repo.bookmarks_arc(),
    );

    info!(
        ctx.logger(),
        "SegmentedChangelogSeeder initialized for repository '{}'",
        repo.name()
    );

    let (head, bonsai_changesets_to_include) = match matches.value_of(HEAD_ARG) {
        Some(head_arg) => {
            let head = helpers::csid_resolve(&ctx, repo.clone(), head_arg)
                .await
                .with_context(|| format!("resolving head csid for '{}'", head_arg))?;
            // If a head not from the config is used
            (head.into(), vec![])
        }
        None => {
            let head = config.segmented_changelog_config.master_bookmark;
            (
                head.map(BookmarkName::new).transpose()?.into(),
                config
                    .segmented_changelog_config
                    .bonsai_changesets_to_include,
            )
        }
    };

    info!(ctx.logger(), "using '{}' for head", head);
    if bonsai_changesets_to_include.len() > 0 {
        info!(
            ctx.logger(),
            "also adding {:?} to segmented changelog", bonsai_changesets_to_include
        );
    }

    let mut heads = vec![head];
    heads.extend(bonsai_changesets_to_include.into_iter().map(SeedHead::from));
    if let Some(idmap_version) = idmap_version_arg {
        segmented_changelog_seeder
            .run_with_idmap_version(&ctx, heads, IdMapVersion(idmap_version))
            .await
            .context("seeding segmented changelog")?;
    } else {
        segmented_changelog_seeder
            .run(&ctx, heads)
            .await
            .context("seeding segmented changelog")?;
    }

    info!(
        ctx.logger(),
        "successfully finished seeding SegmentedChangelog for repository '{}'",
        repo.name(),
    );

    Ok(())
}
