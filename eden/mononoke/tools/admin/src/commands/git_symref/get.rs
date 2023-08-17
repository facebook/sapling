/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;

use super::Repo;

#[derive(Args)]
pub struct GetSymrefArgs {
    /// The name of the symref
    #[clap(long)]
    symref_name: String,
}

pub async fn get(repo: &Repo, get_args: GetSymrefArgs) -> Result<()> {
    match repo
        .git_symbolic_refs
        .get_ref_by_symref(get_args.symref_name.clone())
        .await?
    {
        Some(symref_entry) => println!(
            "The symbolic ref {} points to {} {}",
            symref_entry.symref_name, symref_entry.ref_type, symref_entry.ref_name
        ),
        None => println!("Symbolic ref {} not found", get_args.symref_name),
    }
    Ok(())
}
