/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use clap::Args;
use context::CoreContext;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;

use super::Repo;

/// Args for inspecting bundle-uri's state for a repo
#[derive(Args)]
pub struct InspectArgs {
    /// The Mononoke repo for which to inspect bundle-uri's state
    #[clap(flatten)]
    repo: RepoArgs,
}

pub async fn inspect_bundle_uri(
    ctx: &CoreContext,
    app: &MononokeApp,
    args: InspectArgs,
) -> Result<()> {
    let repo: Arc<Repo> = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;

    // Get all available bundle lists for the repo
    let bundle_lists = repo.git_bundle_uri.get_bundle_lists(ctx).await?;

    if bundle_lists.is_empty() {
        println!(
            "No bundle lists found for repo {}",
            repo.repo_identity.name()
        );
        return Ok(());
    }

    println!(
        "Available bundle lists for repo {}:",
        repo.repo_identity.name()
    );
    for bundle_list in bundle_lists {
        println!("Bundle list #{}", bundle_list.bundle_list_num);
        println!("  Contains {} bundles:", bundle_list.bundles.len());

        for bundle in bundle_list.bundles {
            println!("    - Handle: {}", bundle.handle);
            println!("      Fingerprint: {}", bundle.fingerprint);
            println!("      Order: {}", bundle.in_bundle_list_order);
            println!(
                "      Generation timestamp: {}",
                bundle.generation_start_timestamp
            );
        }
        println!();
    }

    Ok(())
}
