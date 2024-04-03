/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod count_underived;
mod derive;
mod derive_bulk;
mod derive_slice;
mod exists;
mod list_manifests;
mod slice;
mod verify_manifests;

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::Bookmarks;
use cacheblob::dummy::DummyLease;
use changesets::Changesets;
use clap::Parser;
use clap::Subcommand;
use commit_graph::CommitGraph;
use filenodes::Filenodes;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;

use self::count_underived::count_underived;
use self::count_underived::CountUnderivedArgs;
use self::derive::derive;
use self::derive::DeriveArgs;
use self::derive_bulk::derive_bulk;
use self::derive_bulk::DeriveBulkArgs;
use self::derive_slice::derive_slice;
use self::derive_slice::DeriveSliceArgs;
use self::exists::exists;
use self::exists::ExistsArgs;
use self::list_manifests::list_manifests;
use self::list_manifests::ListManifestsArgs;
use self::slice::slice;
use self::slice::SliceArgs;
use self::verify_manifests::verify_manifests;
use self::verify_manifests::VerifyManifestsArgs;

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
    changesets: dyn Changesets,
    #[facet]
    repo_blobstore: RepoBlobstore,
    #[facet]
    bookmarks: dyn Bookmarks,
    #[facet]
    commit_graph: CommitGraph,
    #[facet]
    filenodes: dyn Filenodes,
}

/// Request information about derived data
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: DerivedDataSubcommand,
}

#[derive(Subcommand)]
enum DerivedDataSubcommand {
    /// Count how many ancestors of a given commit weren't derived
    CountUnderived(CountUnderivedArgs),
    /// Actually derive data
    Derive(DeriveArgs),
    /// Backfill derived data for public commits
    DeriveBulk(DeriveBulkArgs),
    /// Derive data for a slice of commits
    DeriveSlice(DeriveSliceArgs),
    /// Check if derived data has been generated
    Exists(ExistsArgs),
    /// Inspect manifests for a given path
    ListManifests(ListManifestsArgs),
    /// Slice underived ancestors of given commits
    Slice(SliceArgs),
    /// Compare check if derived data has been generated
    VerifyManifests(VerifyManifestsArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let mut ctx = app.new_basic_context();

    let repo: Repo = match &args.subcommand {
        DerivedDataSubcommand::Exists(_)
        | DerivedDataSubcommand::CountUnderived(_)
        | DerivedDataSubcommand::VerifyManifests(_)
        | DerivedDataSubcommand::ListManifests(_)
        | DerivedDataSubcommand::Slice(_) => app
            .open_repo(&args.repo)
            .await
            .context("Failed to open repo")?,
        DerivedDataSubcommand::Derive(DeriveArgs { rederive, .. })
        | DerivedDataSubcommand::DeriveSlice(DeriveSliceArgs { rederive, .. }) => {
            open_repo_for_derive(&app, &args.repo, rederive)
                .await
                .context("Failed to open repo")?
        }
        DerivedDataSubcommand::DeriveBulk(_) => open_repo_for_derive(&app, &args.repo, &false)
            .await
            .context("Failed to open repo")?,
    };

    match args.subcommand {
        DerivedDataSubcommand::Exists(args) => exists(&ctx, &repo, args).await?,
        DerivedDataSubcommand::CountUnderived(args) => count_underived(&ctx, &repo, args).await?,
        DerivedDataSubcommand::VerifyManifests(args) => verify_manifests(&ctx, &repo, args).await?,
        DerivedDataSubcommand::ListManifests(args) => list_manifests(&ctx, &repo, args).await?,
        DerivedDataSubcommand::Derive(args) => derive(&mut ctx, &repo, args).await?,
        DerivedDataSubcommand::Slice(args) => slice(&ctx, &repo, args).await?,
        DerivedDataSubcommand::DeriveSlice(args) => derive_slice(&ctx, &repo, args).await?,
        DerivedDataSubcommand::DeriveBulk(args) => derive_bulk(&mut ctx, &repo, args).await?,
    }

    Ok(())
}

async fn open_repo_for_derive(app: &MononokeApp, repo: &RepoArgs, rederive: &bool) -> Result<Repo> {
    if *rederive {
        app.open_repo_with_factory_customization(repo, |repo_factory| {
            repo_factory
                .with_lease_override(|_| Arc::new(DummyLease {}))
                .with_bonsai_hg_mapping_override()
        })
        .await
    } else {
        app.open_repo_with_factory_customization(repo, |repo_factory| {
            repo_factory.with_lease_override(|_| Arc::new(DummyLease {}))
        })
        .await
    }
}

mod args {
    use std::sync::Arc;

    use anyhow::Result;
    use clap::builder::PossibleValuesParser;
    use clap::Args;
    use context::CoreContext;
    use derived_data_utils::derived_data_utils;
    use derived_data_utils::derived_data_utils_for_config;
    use derived_data_utils::DerivedUtils;
    use derived_data_utils::DEFAULT_BACKFILLING_CONFIG_NAME;
    use derived_data_utils::POSSIBLE_DERIVED_TYPE_NAMES;
    use mononoke_types::DerivableType;

    use super::Repo;

    #[derive(Args)]
    pub(super) struct DerivedUtilsArgs {
        /// Use backfilling config rather than enabled config
        #[clap(long)]
        pub(super) backfill: bool,
        /// Sets the name for backfilling derived data types config
        #[clap(long, default_value = DEFAULT_BACKFILLING_CONFIG_NAME)]
        pub(super) backfill_config_name: String,
        /// Type of derived data
        #[clap(long, short = 'T', value_parser = PossibleValuesParser::new(POSSIBLE_DERIVED_TYPE_NAMES))]
        pub(super) derived_data_type: String,
    }

    impl DerivedUtilsArgs {
        pub(super) fn derived_utils(
            self,
            ctx: &CoreContext,
            repo: &Repo,
        ) -> Result<Arc<dyn DerivedUtils>> {
            if self.backfill {
                derived_data_utils_for_config(
                    ctx.fb,
                    repo,
                    DerivableType::from_name(&self.derived_data_type)?,
                    self.backfill_config_name,
                )
            } else {
                derived_data_utils(
                    ctx.fb,
                    &repo,
                    DerivableType::from_name(&self.derived_data_type)?,
                )
            }
        }
    }
}
