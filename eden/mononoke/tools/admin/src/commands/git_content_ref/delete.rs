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
pub struct DeleteContentRefArgs {
    /// The names of the content refs to be deleted
    #[clap(long, value_delimiter = ',')]
    ref_names: Vec<String>,
}

pub async fn delete(repo: &Repo, delete_args: DeleteContentRefArgs) -> Result<()> {
    let success_msg = format!(
        "Successfully deleted content refs {:?}",
        delete_args.ref_names
    );
    repo.git_ref_content_mapping
        .delete_mappings_by_name(delete_args.ref_names)
        .await?;
    println!("{}", success_msg);
    Ok(())
}
