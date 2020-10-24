/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Error};
use clap::{Arg, ArgMatches};
use futures::compat::Future01CompatExt;
use slog::info;

use blobstore_factory::{make_metadata_sql_factory, ReadOnlyStorage};
use bookmarks::BookmarkName;
use bulkops::PublicChangesetBulkFetch;
use cmdlib::{args, helpers};
use context::CoreContext;
use fbinit::FacebookInit;
use metaconfig_types::MetadataDatabaseConfig;
use mononoke_types::ChangesetId;
use segmented_changelog::SegmentedChangelogBuilder;
use sql_ext::facebook::MyAdmin;
use sql_ext::replication::{NoReplicaLagMonitor, ReplicaLagMonitor};

const IDMAP_VERSION_ARG: &str = "idmap-version";
const HEAD_BOOKMARK_ARG: &str = "head-bookmark";
const HEAD_CS_ID_ARG: &str = "head-cs-id";

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeApp::new("Builds a new version of segmented changelog.")
        .with_advanced_args_hidden()
        .with_fb303_args()
        .build()
        .version("0.0.0")
        .about("Builds a new version of segmented changelog.")
        .arg(
            Arg::with_name(IDMAP_VERSION_ARG)
                .long(IDMAP_VERSION_ARG)
                .takes_value(true)
                .required(false)
                .help("What version to label the new idmap with."),
        )
        .arg(
            Arg::with_name(HEAD_BOOKMARK_ARG)
                .long(HEAD_BOOKMARK_ARG)
                .takes_value(true)
                .required(false)
                .help("What bookmark to use as the head of the Segmented Changelog."),
        )
        .arg(
            Arg::with_name(HEAD_CS_ID_ARG)
                .long(HEAD_CS_ID_ARG)
                .takes_value(true)
                .required(false)
                .help("What changeset id to use as the head of the Segmented Changelog."),
        );
    let matches = app.get_matches();

    let logger = args::init_logging(fb, &matches);
    args::init_config_store(fb, &logger, &matches)?;
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

async fn run<'a>(ctx: CoreContext, matches: &'a ArgMatches<'a>) -> Result<(), Error> {
    let idmap_version_arg: Option<u64> = args::get_u64_opt(&matches, IDMAP_VERSION_ARG);
    let config_store = args::init_config_store(ctx.fb, ctx.logger(), matches)?;

    // This is a bit weird from the dependency point of view but I think that it is best. The
    // BlobRepo may have a SegmentedChangelog attached to it but that doesn't hurt us in any way.
    // On the other hand reconstructing the dependencies for SegmentedChangelog without BlobRepo is
    // probably prone to more problems from the maintenance perspective.
    let repo = args::open_repo(ctx.fb, ctx.logger(), &matches)
        .compat()
        .await
        .context("opening repo")?;

    let mysql_options = args::parse_mysql_options(matches);
    let (_, config) = args::get_config(config_store, &matches)?;
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
        mysql_options,
        readonly_storage,
        ctx.logger().clone(),
    )
    .compat()
    .await
    .context("constructing metadata sql factory")?;

    let changeset_bulk_fetch = PublicChangesetBulkFetch::new(
        repo.get_repoid(),
        repo.get_changesets_object(),
        repo.get_phases(),
    );

    let mut segmented_changelog_builder = sql_factory
        .open::<SegmentedChangelogBuilder>()
        .compat()
        .await
        .context("constructing segmented changelog builder")?;

    if let Some(idmap_version) = idmap_version_arg {
        segmented_changelog_builder = segmented_changelog_builder.with_idmap_version(idmap_version);
    }

    let segmented_changelog_seeder = segmented_changelog_builder
        .with_repo_id(repo.get_repoid())
        .with_replica_lag_monitor(replica_lag_monitor)
        .with_changeset_bulk_fetch(Arc::new(changeset_bulk_fetch))
        .with_blobstore(Arc::new(repo.get_blobstore()))
        .build_seeder(&ctx)
        .await
        .context("building SegmentedChangelogSeeder")?;

    info!(
        ctx.logger(),
        "SegmentedChangelogBuilder initialized for repository '{}'",
        repo.name()
    );

    let head = match (
        matches.value_of(HEAD_BOOKMARK_ARG),
        matches.value_of(HEAD_CS_ID_ARG),
    ) {
        (b @ None, None) | (b @ Some(_), None) => {
            let head_bookmark = b.unwrap_or("master");
            let cs_id = repo
                .bookmarks()
                .get(
                    ctx.clone(),
                    &BookmarkName::new(&head_bookmark)
                        .with_context(|| format!("invalid bookmark name: {}", head_bookmark))?,
                )
                .await
                .context("fetching master changesetid")?
                .ok_or_else(|| anyhow!("'{}' bookmark could not be found", head_bookmark))?;
            info!(
                ctx.logger(),
                "resolved bookmark '{}' to '{}'", head_bookmark, cs_id
            );
            cs_id
        }
        (None, Some(id)) => ChangesetId::from_str(id)
            .with_context(|| format!("invalid changeset id passed to `{}`", HEAD_CS_ID_ARG))?,
        (Some(_), Some(_)) => bail!(
            "Invalid input. Both '{}' and '{}' arguments specified",
            HEAD_BOOKMARK_ARG,
            HEAD_CS_ID_ARG
        ),
    };

    segmented_changelog_seeder
        .run(&ctx, head)
        .await
        .context("seeding segmented changelog")?;

    info!(
        ctx.logger(),
        "successfully finished seeding SegmentedChangelog for repository '{}'",
        repo.name(),
    );

    Ok(())
}
