/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! DrawDAG for Integration Tests
//!
//! A DrawDAG specification consists of an ASCII graph (either left-to-right
//! or bottom-to-top), and a series of comments that define actions that apply
//! to that graph.
//!
//! See documentation of `Action` for actions that affect the repository, and
//! `ChangeAction` for actions that change commits.
//!
//! Values that contain special characters can be surrounded by quotes.
//! Values that require binary data can prefix a hex string with `&`, e.g.
//! `&face` becomes a two byte string with the values `FA CE`.

use std::collections::HashMap;
use std::fmt::Display;
use std::io::Write;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blame::RootBlameV2;
use blobrepo::BlobRepo;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use changeset_info::ChangesetInfo;
use clap::Parser;
use context::CoreContext;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data_manager::BonsaiDerivable;
use fastlog::RootFastlog;
use filenodes_derivation::FilenodesOnlyPublic;
use fsnodes::RootFsnodeId;
use futures::try_join;
use mercurial_derivation::MappedHgChangesetId;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use repo_derived_data::RepoDerivedDataRef;
use skeleton_manifest::RootSkeletonManifestId;
use tests_utils::drawdag::extend_from_dag_with_actions;
use tokio::io::AsyncReadExt;
use topo_sort::sort_topological;
use unodes::RootUnodeManifestId;

/// Create commits from a drawn DAG.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    /// Disable creation of default files in each commit
    #[clap(long)]
    no_default_files: bool,

    /// Derive all derived data types for all commits
    #[clap(long)]
    derive_all: bool,

    /// Print hashes in HG format instead of bonsai
    #[clap(long)]
    print_hg_hashes: bool,
}

fn print_name_hash_pairs(pairs: impl IntoIterator<Item = (String, impl Display)>) -> Result<()> {
    for (name, id) in pairs.into_iter() {
        writeln!(std::io::stdout(), "{}={}", name, id)?;
    }
    Ok(())
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    let repo: BlobRepo = app
        .open_repo(&args.repo_args)
        .await
        .context("Failed to open repo")?;

    // Read DAG from stdin
    let mut input = String::new();
    tokio::io::stdin().read_to_string(&mut input).await?;
    if args.no_default_files {
        input.push_str("\n# default_files: false\n");
    }

    let (commits, dag) = extend_from_dag_with_actions(&ctx, &repo, &input).await?;

    let any_derivation_needed = args.derive_all | args.print_hg_hashes;
    if any_derivation_needed {
        let dag: HashMap<_, _> = dag
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect::<Vec<_>>()))
            .collect();
        let sorted = sort_topological(&dag).ok_or_else(|| anyhow!("Graph has a cycle"))?;
        let csids = sorted
            .into_iter()
            .map(|name| {
                commits
                    .get(&name)
                    .cloned()
                    .ok_or_else(|| anyhow!("No commit found for {}", name))
            })
            .collect::<Result<Vec<_>>>()?;

        if args.derive_all {
            derive_all(&ctx, &repo, &csids).await?;
        } else {
            derive::<MappedHgChangesetId>(&ctx, &repo, &csids).await?;
        }
    }

    if args.print_hg_hashes {
        let mapping: HashMap<_, _> = repo
            .bonsai_hg_mapping()
            .get(&ctx, commits.values().copied().collect::<Vec<_>>().into())
            .await?
            .into_iter()
            .map(|entry| (entry.bcs_id, entry.hg_cs_id))
            .collect();
        let commits = commits
            .into_iter()
            .map(|(name, id)| {
                mapping
                    .get(&id)
                    .ok_or_else(|| anyhow!("Couldn't translate {}={} to hg", name, id))
                    .map(|hg_id| (name, hg_id))
            })
            .collect::<Result<Vec<_>>>()?;
        print_name_hash_pairs(commits)?;
    } else {
        print_name_hash_pairs(commits)?;
    }

    Ok(())
}

async fn derive<D: BonsaiDerivable>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csids: &[ChangesetId],
) -> Result<()> {
    let mgr = repo.repo_derived_data().manager();
    mgr.derive_exactly_batch::<D>(ctx, csids.to_vec(), None)
        .await
        .with_context(|| format!("Failed to derive {}", D::NAME))?;
    Ok(())
}

async fn derive_all(ctx: &CoreContext, repo: &BlobRepo, csids: &[ChangesetId]) -> Result<()> {
    let mercurial = async {
        derive::<MappedHgChangesetId>(ctx, repo, csids).await?;
        derive::<FilenodesOnlyPublic>(ctx, repo, csids).await?;
        Ok::<_, Error>(())
    };
    let unodes = async {
        derive::<RootUnodeManifestId>(ctx, repo, csids).await?;
        try_join!(
            derive::<RootBlameV2>(ctx, repo, csids),
            derive::<RootDeletedManifestV2Id>(ctx, repo, csids),
            derive::<RootFastlog>(ctx, repo, csids),
        )?;
        Ok::<_, Error>(())
    };
    try_join!(
        mercurial,
        unodes,
        derive::<RootFsnodeId>(ctx, repo, csids),
        derive::<RootSkeletonManifestId>(ctx, repo, csids),
        derive::<ChangesetInfo>(ctx, repo, csids),
    )?;
    Ok(())
}
