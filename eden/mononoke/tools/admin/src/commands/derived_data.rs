/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod count_underived;
mod derive;
mod derive_slice;
mod exists;
mod slice;
mod verify_manifests;

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
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
use self::derive_slice::derive_slice;
use self::derive_slice::DeriveSliceArgs;
use self::exists::exists;
use self::exists::ExistsArgs;
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
    /// Check if derived data has been generated
    Exists(ExistsArgs),
    /// Count how many ancestors of a given commit weren't derived
    CountUnderived(CountUnderivedArgs),
    /// Compare check if derived data has been generated
    VerifyManifests(VerifyManifestsArgs),
    /// Actually derive data
    Derive(DeriveArgs),
    /// Slice underived ancestors of given commits
    Slice(SliceArgs),
    /// Derive data for a slice of commits
    DeriveSlice(DeriveSliceArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let mut ctx = app.new_basic_context();

    let repo: Repo = match &args.subcommand {
        DerivedDataSubcommand::Exists(_)
        | DerivedDataSubcommand::CountUnderived(_)
        | DerivedDataSubcommand::VerifyManifests(_)
        | DerivedDataSubcommand::Slice(_) => app
            .open_repo(&args.repo)
            .await
            .context("Failed to open repo")?,
        DerivedDataSubcommand::Derive(DeriveArgs { rederive, .. })
        | DerivedDataSubcommand::DeriveSlice(DeriveSliceArgs { rederive, .. }) => if *rederive {
            app.open_repo_with_factory_customization(&args.repo, |repo_factory| {
                repo_factory
                    .with_lease_override(|_| Arc::new(DummyLease {}))
                    .with_bonsai_hg_mapping_override()
            })
            .await
        } else {
            app.open_repo_with_factory_customization(&args.repo, |repo_factory| {
                repo_factory.with_lease_override(|_| Arc::new(DummyLease {}))
            })
            .await
        }
        .context("Failed to open repo")?,
    };

    match args.subcommand {
        DerivedDataSubcommand::Exists(args) => exists(&ctx, &repo, args).await?,
        DerivedDataSubcommand::CountUnderived(args) => count_underived(&ctx, &repo, args).await?,
        DerivedDataSubcommand::VerifyManifests(args) => verify_manifests(&ctx, &repo, args).await?,
        DerivedDataSubcommand::Derive(args) => derive(&mut ctx, &repo, args).await?,
        DerivedDataSubcommand::Slice(args) => slice(&ctx, &repo, args).await?,
        DerivedDataSubcommand::DeriveSlice(args) => derive_slice(&ctx, &repo, args).await?,
    }

    Ok(())
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
    use derived_data_utils::POSSIBLE_DERIVED_TYPES;

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
        #[clap(long, short = 'T', value_parser = PossibleValuesParser::new(POSSIBLE_DERIVED_TYPES))]
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
                    self.derived_data_type,
                    self.backfill_config_name,
                )
            } else {
                derived_data_utils(ctx.fb, &repo, self.derived_data_type)
            }
        }
    }
}
