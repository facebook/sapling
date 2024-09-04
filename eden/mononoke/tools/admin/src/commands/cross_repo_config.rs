/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use context::CoreContext;
use itertools::Itertools;
use metaconfig_types::CommitSyncConfig;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_cross_repo::RepoCrossRepo;
use repo_cross_repo::RepoCrossRepoRef;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;

/// Query available CommitSyncConfig versions for the repo
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: ConfigSubcommand,
}

#[derive(Subcommand)]
pub enum ConfigSubcommand {
    /// Print info about a particular version of CommitSyncConfig
    ByVersion(ByVersionArgs),
    /// List all available CommitSyncConfig versions for the repo
    List(ListArgs),
}

#[derive(Args)]
pub struct ByVersionArgs {
    /// Commit sync config version name to query
    version_name: String,
}

#[derive(Args)]
pub struct ListArgs {
    /// Print the body of the configs not just their version names
    #[clap(long)]
    with_contents: bool,
}

#[facet::container]
#[derive(Clone)]
pub struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_cross_repo: RepoCrossRepo,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    let repo: Repo = app.open_repo(&args.repo).await?;

    match args.subcommand {
        ConfigSubcommand::ByVersion(args) => by_version(&ctx, &repo, args).await,
        ConfigSubcommand::List(args) => list(&ctx, &repo, args).await,
    }
}

async fn by_version(_ctx: &CoreContext, repo: &Repo, args: ByVersionArgs) -> Result<()> {
    let commit_sync_config = repo
        .repo_cross_repo()
        .live_commit_sync_config()
        .get_commit_sync_config_by_version(
            repo.repo_identity().id(),
            &CommitSyncConfigVersion(args.version_name),
        )
        .await?;

    print_commit_sync_config(commit_sync_config, "");

    Ok(())
}

async fn list(_ctx: &CoreContext, repo: &Repo, args: ListArgs) -> Result<()> {
    let commit_sync_configs = repo
        .repo_cross_repo()
        .live_commit_sync_config()
        .get_all_commit_sync_config_versions(repo.repo_identity().id())
        .await?;

    for (version_name, commit_sync_config) in commit_sync_configs
        .into_iter()
        .sorted_by_key(|(vn, _)| vn.clone())
    {
        if args.with_contents {
            println!("{}:", version_name);
            print_commit_sync_config(commit_sync_config, "  ");
            println!("\n");
        } else {
            println!("{}", version_name);
        }
    }

    Ok(())
}

fn print_commit_sync_config(csc: CommitSyncConfig, line_prefix: &str) {
    println!("{}large repo: {}", line_prefix, csc.large_repo_id);
    println!(
        "{}common pushrebase bookmarks: {:?}",
        line_prefix, csc.common_pushrebase_bookmarks
    );
    println!("{}version name: {}", line_prefix, csc.version_name);
    for (small_repo_id, small_repo_config) in csc
        .small_repos
        .into_iter()
        .sorted_by_key(|(small_repo_id, _)| *small_repo_id)
    {
        println!("{}  small repo: {}", line_prefix, small_repo_id);
        println!(
            "{}  default action: {:?}",
            line_prefix, small_repo_config.default_action
        );
        println!("{}  prefix map:", line_prefix);
        for (from, to) in small_repo_config
            .map
            .into_iter()
            .sorted_by_key(|(from, _)| from.clone())
        {
            println!("{}    {}->{}", line_prefix, from, to);
        }
    }
}
