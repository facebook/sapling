/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use clap::Args;
use git_symbolic_refs::GitSymbolicRefsEntry;

use super::Repo;

#[derive(Args)]
pub struct UpdateSymrefArgs {
    /// The name of the symref
    #[clap(long)]
    symref_name: String,
    /// The name of the ref that the symref points to
    #[clap(long)]
    ref_name: String,
    /// The type of the ref that the symref points to
    #[clap(long)]
    ref_type: String,
}

pub async fn update(repo: &Repo, update_args: UpdateSymrefArgs) -> Result<()> {
    let success_msg = format!(
        "Symbolic ref {} pointing to {} {} has been updated",
        update_args.symref_name, update_args.ref_type, update_args.ref_name
    );
    let entry = GitSymbolicRefsEntry::new(
        update_args.symref_name,
        update_args.ref_name,
        update_args.ref_type,
    )
    .context("Error in creating GitSymbolicRefsEntry from provided input")?;

    repo.git_symbolic_refs
        .add_or_update_entries(vec![entry])
        .await?;
    println!("{}", success_msg);
    Ok(())
}
