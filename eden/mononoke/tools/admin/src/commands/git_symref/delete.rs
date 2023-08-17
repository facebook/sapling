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
pub struct DeleteSymrefArgs {
    /// The names of the symrefs to be deleted
    #[clap(long, value_delimiter = ',')]
    symref_names: Vec<String>,
}

pub async fn delete(repo: &Repo, delete_args: DeleteSymrefArgs) -> Result<()> {
    let success_msg = format!(
        "Successfully deleted symrefs {:?}",
        delete_args.symref_names
    );
    repo.git_symbolic_refs
        .delete_symrefs(delete_args.symref_names)
        .await?;
    println!("{}", success_msg);
    Ok(())
}
