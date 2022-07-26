/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod graph;
mod list_ancestors;

use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use changeset_fetcher::ChangesetFetcher;
use changesets::Changesets;
use clap::Parser;
use clap::Subcommand;
use metaconfig_types::RepoConfig;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_blobstore::RepoBlobstore;
use repo_identity::RepoIdentity;

use self::graph::ChangelogGraphArgs;
use self::list_ancestors::ChangelogListAncestorsArgs;

/// Manipulate changelogs
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    #[clap(subcommand)]
    subcommand: ChangelogSubcommand,
}

#[facet::container]
pub struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_config: RepoConfig,

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
    changeset_fetcher: dyn ChangesetFetcher,
}

#[derive(Subcommand)]
pub enum ChangelogSubcommand {
    /// Display parts of the commit DAG
    Graph(ChangelogGraphArgs),

    /// List ancestors of a commit
    ListAncestors(ChangelogListAncestorsArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    let repo: Repo = app
        .open_repo(&args.repo_args)
        .await
        .context("Failed to open repo")?;

    match args.subcommand {
        ChangelogSubcommand::Graph(graph_args) => graph::graph(&ctx, &repo, graph_args).await?,
        ChangelogSubcommand::ListAncestors(list_ancestors_args) => {
            list_ancestors::list_ancestors(&ctx, &repo, list_ancestors_args).await?
        }
    }

    Ok(())
}
