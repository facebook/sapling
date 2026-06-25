/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use clap::Args;
use commit_id::IdentityScheme;
use commit_id::print_commit_id;
use context::CoreContext;
use futures::stream::TryStreamExt;
use metaconfig_types::RepoConfigRef;
use repo_identity::RepoIdentityRef;
use tokio::process::Command;

use super::Repo;
use crate::bookmark_log_entry::BookmarkLogEntry;

// Everything is rendered as bonsai -- the native changeset id, and what the AWS
// side prints too, so the two sides line up for an eyeball comparison.
const BONSAI: &[IdentityScheme] = &[IdentityScheme::Bonsai];

// Modern sync mirrors each repo into a shadow repo named "<repo>_shadow"
// (configerator `source/scm/mononoke/repos/repos/aws.cinc`).
const SHADOW_REPO_SUFFIX: &str = "_shadow";

// Fixed AWS deployment coordinates for the shadow mononoke server. These have
// not changed in practice; if the deployment ever moves, update them here.
const AWS_CLOUD: &str = "mononoke-cloud";
const AWS_REGION: &str = "us-west-2";
const AWS_NAMESPACE: &str = "mononoke-prod";
const AWS_K8S_NAMESPACE: &str = "default";
const AWS_DEPLOYMENT: &str = "mononoke-server";
const AWS_CONTAINER: &str = "server";

#[derive(Args)]
pub struct StatusArgs {
    /// The bookmark modern sync mirrors (it only syncs this one bookmark)
    #[clap(long, default_value = "master")]
    bookmark: BookmarkKey,
}

pub async fn status(ctx: &CoreContext, repo: &Repo, args: StatusArgs) -> Result<()> {
    let repo_name = repo.repo_identity().name();
    println!("Modern sync status for repo '{repo_name}'");
    println!();

    // --- Enablement gate ---
    // This is exactly what the sync job gates on (`sync.rs` / `sync_sharded.rs`
    // bail with "No modern sync config found"). If there is no config, the repo
    // is not mirrored to AWS, so there is nothing to compare.
    if repo.repo_config().modern_sync_config.is_none() {
        println!("Modern sync is NOT configured for this repo; nothing to compare.");
        return Ok(());
    }

    let shadow_repo = format!("{repo_name}{SHADOW_REPO_SUFFIX}");
    let bookmark = args.bookmark.to_string();
    let kubeconfig_ok = ensure_aws_kubeconfig().await;

    // --- bookmark (internal vs AWS) ---
    println!("== {} ==", args.bookmark);
    let internal_master = repo
        .bookmarks()
        .get(ctx.clone(), &args.bookmark, Freshness::MostRecent)
        .await
        .with_context(|| format!("Failed to resolve bookmark '{}'", args.bookmark))?;
    print!("  internal: ");
    match internal_master {
        Some(cs_id) => print_commit_id(ctx, repo, BONSAI, cs_id).await?,
        None => println!("(not set)"),
    }
    print_aws_value(kubeconfig_ok, &shadow_repo, &["get", &bookmark]).await;
    println!();

    // --- latest movement (internal vs AWS) ---
    println!("== latest movement of '{}' ==", args.bookmark);
    print!("  internal: ");
    print_internal_latest_movement(ctx, repo, &args.bookmark).await?;
    print_aws_value(
        kubeconfig_ok,
        &shadow_repo,
        &["log", &bookmark, "--limit", "1"],
    )
    .await;

    Ok(())
}

/// Print the most recent `bookmark_update_log` entry for the bookmark on the
/// internal (prod) side.
async fn print_internal_latest_movement(
    ctx: &CoreContext,
    repo: &Repo,
    bookmark: &BookmarkKey,
) -> Result<()> {
    let latest = repo
        .bookmark_update_log()
        .list_bookmark_log_entries(
            ctx.clone(),
            bookmark.clone(),
            1,
            None,
            Freshness::MostRecent,
        )
        .try_next()
        .await
        .context("Failed to read latest bookmark log entry")?;
    match latest {
        None => println!("(no log entries)"),
        Some((entry_id, cs_id, reason, timestamp)) => {
            let rendered = BookmarkLogEntry::new(
                ctx,
                repo,
                timestamp,
                bookmark.clone(),
                reason,
                cs_id,
                Some(entry_id),
                BONSAI,
            )
            .await?;
            println!("{rendered}");
        }
    }
    Ok(())
}

/// Point kubectl at the AWS shadow cluster. Returns false (and prints a note) if
/// the `cloud` CLI is missing or fails, so the rest of the report still shows.
async fn ensure_aws_kubeconfig() -> bool {
    match run(
        "cloud",
        &[
            "eks",
            "update-kubeconfig",
            AWS_CLOUD,
            AWS_REGION,
            AWS_NAMESPACE,
        ],
    )
    .await
    {
        Ok(_) => true,
        Err(e) => {
            println!("AWS access unavailable ({e:#}); showing internal side only.");
            println!();
            false
        }
    }
}

/// Print one AWS-side value by exec-ing `monad bookmarks <monad_args>` inside the
/// mononoke-server pod, and return the raw value. On failure, prints the command
/// to run manually instead and returns None.
async fn print_aws_value(
    kubeconfig_ok: bool,
    shadow_repo: &str,
    monad_args: &[&str],
) -> Option<String> {
    let value = aws_monad(kubeconfig_ok, shadow_repo, monad_args).await;
    match &value {
        Some(v) => println!("  aws:      {v}"),
        None => println!(
            "  aws:      (unavailable) run: kubectl exec deploy/{AWS_DEPLOYMENT} -c {AWS_CONTAINER} -n {AWS_K8S_NAMESPACE} -- monad bookmarks --repo-name {shadow_repo} {}",
            monad_args.join(" ")
        ),
    }
    value
}

/// Exec `monad bookmarks <monad_args>` inside the mononoke-server pod and return
/// the trimmed stdout, or None if AWS is unavailable or the command fails.
async fn aws_monad(kubeconfig_ok: bool, shadow_repo: &str, monad_args: &[&str]) -> Option<String> {
    if !kubeconfig_ok {
        return None;
    }
    let deployment = format!("deploy/{AWS_DEPLOYMENT}");
    let mut argv = vec![
        "exec",
        deployment.as_str(),
        "-c",
        AWS_CONTAINER,
        "-n",
        AWS_K8S_NAMESPACE,
        "--",
        "monad",
        "bookmarks",
        "--repo-name",
        shadow_repo,
    ];
    argv.extend_from_slice(monad_args);
    run("kubectl", &argv)
        .await
        .ok()
        .map(|s| s.trim().to_owned())
}

/// Run a command and return its stdout. Fails if the program can't be spawned or
/// exits non-zero.
async fn run(program: &str, argv: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(argv)
        .output()
        .await
        .with_context(|| format!("failed to spawn `{program}` (is it installed and on PATH?)"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "`{program}` exited with {}: {}",
            output.status,
            stderr.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
