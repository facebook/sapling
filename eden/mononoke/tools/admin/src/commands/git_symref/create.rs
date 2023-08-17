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
pub struct CreateSymrefArgs {
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

pub async fn create(repo: &Repo, create_args: CreateSymrefArgs) -> Result<()> {
    // Check if the symref being added already exists
    let symrefs = repo.git_symbolic_refs.clone();
    if let Some(symref_entry) = symrefs
        .get_ref_by_symref(create_args.symref_name.clone())
        .await?
    {
        anyhow::bail!(
            "The symbolic ref {} already exists and it points to {} {}",
            symref_entry.symref_name,
            symref_entry.ref_name,
            symref_entry.ref_type
        );
    }
    // If the symref doesn't exist, then create it
    let success_msg = format!(
        "Symbolic ref {} pointing to {} {} has been added",
        create_args.symref_name, create_args.ref_type, create_args.ref_name
    );
    let entry = GitSymbolicRefsEntry::new(
        create_args.symref_name,
        create_args.ref_name,
        create_args.ref_type,
    )
    .context("Error in creating GitSymbolicRefsEntry from provided input")?;

    symrefs.add_or_update_entries(vec![entry]).await?;
    println!("{}", success_msg);
    Ok(())
}
