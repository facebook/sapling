/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use clap::Parser;
use futures_stats::TimedFutureExt;
use maplit::btreeset;
use mononoke_api::ChangesetDiffItem;
use mononoke_api::ChangesetFileOrdering;
use mononoke_api::ChangesetId;
use mononoke_api::CopyInfo;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::RepoArgs;
use mononoke_types::path::MPath;
use repo_authorization::AuthorizationContext;
use repo_identity::RepoIdentityRef;

/// Diff two commits through the Mononoke API.
///
/// Runs the same `ChangesetContext::diff` the SCS server uses for
/// `commit_compare`, in this process, so the diff path can be profiled against
/// real repo data. Output matches `scsc diff --paths-only`.
///
/// Which manifest type backs the diff is decided by
/// `scm/mononoke:derived_data_use_content_manifests`, exactly as in production.
/// To compare backends, override it with the global
/// `--just-knobs-config-path`.
///
/// Pass the two commits oldest first: `-i FROM -i TO`.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    /// Restrict the diff to these paths and their descendants
    #[clap(long, short = 'p')]
    path: Vec<String>,

    /// Show copies and moves as adds and deletes
    #[clap(long)]
    skip_copies_renames: bool,

    /// Compare against the source of subtree copies
    #[clap(long)]
    compare_with_subtree_copy_sources: bool,

    /// Include directories as well as files
    #[clap(long)]
    trees: bool,

    /// Generate the diff in repository order, which weights manifest
    /// replacements and so takes a different path through the manifest code
    #[clap(long, short = 'O')]
    ordered: bool,

    /// Resume the ordered diff after this path
    #[clap(long, requires = "ordered")]
    after: Option<String>,

    /// Stop after this many entries
    #[clap(long, short = 'l')]
    limit: Option<usize>,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    let mononoke = app
        .open_managed_repo_arg::<Repo>(&args.repo_args)
        .await
        .context("Failed to open repo")?
        .make_mononoke_api()?;
    let repo_name = mononoke
        .repos()
        .next()
        .ok_or_else(|| anyhow!("No repo was opened"))?
        .repo_identity()
        .name()
        .to_string();
    let repo = mononoke
        .repo(ctx.clone(), &repo_name)
        .await?
        .ok_or_else(|| anyhow!("Repo not found: {repo_name}"))?
        .with_authorization_context(AuthorizationContext::new_bypass_access_control())
        .build()
        .await?;

    let changesets = args
        .changeset_args
        .resolve_changesets(&ctx, repo.repo())
        .await
        .context("Failed to resolve changesets")?;
    let [from, to] = <[ChangesetId; 2]>::try_from(changesets)
        .map_err(|_| anyhow!("Exactly two commits must be provided: -i FROM -i TO"))?;
    let from = repo
        .changeset(from)
        .await?
        .ok_or_else(|| anyhow!("Changeset not found: {from}"))?;
    let to = repo
        .changeset(to)
        .await?
        .ok_or_else(|| anyhow!("Changeset not found: {to}"))?;

    let paths = if args.path.is_empty() {
        None
    } else {
        Some(
            args.path
                .iter()
                .map(|path| MPath::try_from(path.as_str()))
                .collect::<Result<Vec<_>, _>>()?,
        )
    };
    let mut diff_items: BTreeSet<_> = btreeset! { ChangesetDiffItem::FILES };
    if args.trees {
        diff_items.insert(ChangesetDiffItem::TREES);
    }

    let ordering = if args.ordered {
        ChangesetFileOrdering::Ordered {
            after: args.after.as_deref().map(MPath::try_from).transpose()?,
        }
    } else {
        ChangesetFileOrdering::Unordered
    };

    // `diff` reports changes to its receiver, so `to` is the base.
    let (stats, diff) = to
        .diff(
            &from,
            !args.skip_copies_renames,
            args.compare_with_subtree_copy_sources,
            paths,
            diff_items,
            ordering,
            args.limit,
        )
        .timed()
        .await;
    let diff = diff?;

    for entry in &diff {
        let path = entry.path();
        match (entry.get_old_content(), entry.get_new_content()) {
            (None, Some(_)) => println!("A {path}"),
            (Some(_), None) => println!("D {path}"),
            (Some(old), Some(_)) => match entry.copy_info() {
                CopyInfo::None => println!("M {path}"),
                CopyInfo::Move => println!("R {} -> {path}", old.path()),
                CopyInfo::Copy => println!("C {} -> {path}", old.path()),
            },
            (None, None) => return Err(anyhow!("Empty diff entry for {path}")),
        }
    }

    eprintln!("{} entries in {:?}", diff.len(), stats.completion_time);
    // Names match the `mononoke_diff_service` scuba columns.
    eprintln!(
        "completion_time_us={} poll_time_us={} max_poll_time_us={} poll_count={}",
        stats.completion_time.as_micros(),
        stats.poll_time.as_micros(),
        stats.max_poll_time.as_micros(),
        stats.poll_count,
    );
    Ok(())
}
