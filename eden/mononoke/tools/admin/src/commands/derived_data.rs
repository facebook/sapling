/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod backfill_enqueue;
mod backfill_status;
mod count_underived;
mod derive;
mod derive_slice;
mod exists;
mod fetch;
mod list_manifest;
mod slice;
mod verify_manifests;

use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::Bookmarks;
use clap::Parser;
use clap::Subcommand;
use commit_graph::CommitGraph;
use filenodes::Filenodes;
use filestore::FilestoreConfig;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use mononoke_types::DerivableType;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_factory::RepoFactory;
use repo_identity::RepoIdentity;
use strum::IntoEnumIterator;

use self::backfill_enqueue::BackfillEnqueueArgs;
use self::backfill_enqueue::backfill_enqueue;
use self::backfill_status::BackfillStatusArgs;
use self::backfill_status::backfill_status;
use self::count_underived::CountUnderivedArgs;
use self::count_underived::count_underived;
use self::derive::DeriveArgs;
use self::derive::derive;
use self::derive_slice::DeriveSliceArgs;
use self::derive_slice::derive_slice;
use self::exists::ExistsArgs;
use self::exists::exists;
use self::fetch::FetchArgs;
use self::fetch::fetch;
use self::list_manifest::ListManifestArgs;
use self::list_manifest::list_manifest;
use self::slice::SliceArgs;
use self::slice::slice;
use self::verify_manifests::VerifyManifestsArgs;
use self::verify_manifests::verify_manifests;

#[facet::container]
struct Repo {
    #[facet]
    repo_identity: RepoIdentity,
    #[facet]
    repo_derived_data: RepoDerivedData,
    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,
    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,
    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,
    #[facet]
    bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,
    #[facet]
    repo_blobstore: RepoBlobstore,
    #[facet]
    bookmarks: dyn Bookmarks,
    #[facet]
    commit_graph: CommitGraph,
    #[facet]
    filenodes: dyn Filenodes,
    #[facet]
    filestore_config: FilestoreConfig,
}

/// Request information about derived data
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    /// The derived data config name to use. If not specified, the enabled config will be used
    #[clap(short, long)]
    config_name: Option<String>,

    /// Whether to bypass redaction when deriving and querying derived data.
    #[clap(long)]
    bypass_redaction: bool,

    #[clap(subcommand)]
    subcommand: DerivedDataSubcommand,
}

#[derive(Subcommand)]
enum DerivedDataSubcommand {
    /// Enqueue derived data backfill work via async requests
    BackfillEnqueue(BackfillEnqueueArgs),
    /// Show status of derive backfill jobs
    BackfillStatus(BackfillStatusArgs),
    /// Count how many ancestors of a given commit weren't derived
    CountUnderived(CountUnderivedArgs),
    /// Actually derive data
    Derive(DeriveArgs),
    /// Derive data for a slice of commits
    DeriveSlice(DeriveSliceArgs),
    /// Check if derived data has been generated
    Exists(ExistsArgs),
    /// Fetch previously derived data for the given commits
    Fetch(FetchArgs),
    /// List the contents of a manifest at a given path
    ListManifest(ListManifestArgs),
    /// Slice underived ancestors of given commits
    Slice(SliceArgs),
    /// Compare different manifest types to ensure they are equivalent
    VerifyManifests(VerifyManifestsArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let mut ctx = app.new_basic_context();

    // BackfillStatus doesn't require opening a repo
    if let DerivedDataSubcommand::BackfillStatus(backfill_status_args) = args.subcommand {
        let sql_queue = async_requests_client::open_sql_connection(ctx.fb, &app).await?;
        return backfill_status(&ctx, sql_queue, backfill_status_args).await;
    }

    let bypass_redaction = args.bypass_redaction;
    let config_name_for_enqueue = args.config_name.clone();

    let repo: Repo = match &args.subcommand {
        DerivedDataSubcommand::BackfillStatus(_) => {
            unreachable!("BackfillStatus handled above")
        }
        DerivedDataSubcommand::Exists(_)
        | DerivedDataSubcommand::Fetch(_)
        | DerivedDataSubcommand::CountUnderived(_)
        | DerivedDataSubcommand::VerifyManifests(_)
        | DerivedDataSubcommand::ListManifest(_)
        | DerivedDataSubcommand::Slice(_) => {
            open_repo_for_derive(&app, &args.repo, false, args.bypass_redaction)
                .await
                .context("Failed to open repo")?
        }
        DerivedDataSubcommand::Derive(DeriveArgs { rederive, .. })
        | DerivedDataSubcommand::DeriveSlice(DeriveSliceArgs { rederive, .. })
        | DerivedDataSubcommand::BackfillEnqueue(BackfillEnqueueArgs { rederive, .. }) => {
            open_repo_for_derive(&app, &args.repo, *rederive, args.bypass_redaction)
                .await
                .context("Failed to open repo")?
        }
    };

    let manager = if let Some(config_name) = args.config_name {
        repo.repo_derived_data().manager_for_config(&config_name)?
    } else {
        repo.repo_derived_data().manager()
    };

    let is_read_only = matches!(
        &args.subcommand,
        DerivedDataSubcommand::Exists(_)
            | DerivedDataSubcommand::Fetch(_)
            | DerivedDataSubcommand::CountUnderived(_)
            | DerivedDataSubcommand::ListManifest(_)
            | DerivedDataSubcommand::Slice(_)
    );

    let manager = if is_read_only {
        let mut config = manager.config().clone();
        config.types = DerivableType::iter().collect();
        manager.with_replaced_config(manager.config_name(), config)
    } else {
        manager.clone()
    };

    match args.subcommand {
        DerivedDataSubcommand::BackfillStatus(_) => {
            unreachable!("BackfillStatus handled above")
        }
        DerivedDataSubcommand::BackfillEnqueue(args) => {
            let queue = async_requests_client::build(ctx.fb, &app, None)
                .await
                .context("acquiring the async requests queue")?;
            backfill_enqueue(
                &ctx,
                &app,
                &repo,
                queue,
                args,
                config_name_for_enqueue.as_deref(),
                bypass_redaction,
            )
            .await?
        }
        DerivedDataSubcommand::Exists(args) => exists(&ctx, &repo, &manager, args).await?,
        DerivedDataSubcommand::Fetch(args) => fetch(&ctx, &repo, &manager, args).await?,
        DerivedDataSubcommand::CountUnderived(args) => {
            count_underived(&ctx, &repo, &manager, args).await?
        }
        DerivedDataSubcommand::VerifyManifests(args) => verify_manifests(&ctx, &repo, args).await?,
        DerivedDataSubcommand::ListManifest(args) => {
            list_manifest(&ctx, &repo, &manager, args).await?
        }
        DerivedDataSubcommand::Derive(args) => derive(&mut ctx, &repo, &manager, args).await?,
        DerivedDataSubcommand::Slice(args) => slice(&ctx, &repo, &manager, args).await?,
        DerivedDataSubcommand::DeriveSlice(args) => {
            derive_slice(&ctx, &repo, &manager, args).await?
        }
    }

    Ok(())
}

async fn open_repo_for_derive(
    app: &MononokeApp,
    repo: &RepoArgs,
    rederive: bool,
    bypass_redaction: bool,
) -> Result<Repo> {
    let repo_customization: Box<dyn Fn(&mut RepoFactory) -> &mut RepoFactory + Send> = if rederive {
        Box::new(|repo_factory| repo_factory.with_bonsai_hg_mapping_override())
    } else {
        Box::new(|repo_factory| repo_factory)
    };

    if bypass_redaction {
        app.open_repo_unredacted_with_factory_customization(repo, repo_customization)
            .await
    } else {
        app.open_repo_with_factory_customization(repo, repo_customization)
            .await
    }
}
