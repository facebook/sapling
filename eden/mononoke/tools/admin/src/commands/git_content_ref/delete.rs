/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use context::CoreContext;
use gix_hash::Kind;
use gix_hash::ObjectId;
use repo_update_logger::GitContentRefInfo;
use repo_update_logger::log_git_content_ref;

use super::Repo;

#[derive(Args)]
pub struct DeleteContentRefArgs {
    /// The names of the content refs to be deleted
    #[clap(long, value_delimiter = ',')]
    ref_names: Vec<String>,
}

pub async fn delete(
    repo: &Repo,
    ctx: &CoreContext,
    delete_args: DeleteContentRefArgs,
) -> Result<()> {
    let success_msg = format!(
        "Successfully deleted content refs {:?}",
        delete_args.ref_names
    );
    repo.git_ref_content_mapping
        .delete_mappings_by_name(ctx, delete_args.ref_names.clone())
        .await?;
    for ref_name in delete_args.ref_names {
        let info = GitContentRefInfo {
            repo_name: repo.repo_identity.name().to_string(),
            git_hash: ObjectId::null(Kind::Sha1).to_hex().to_string(),
            object_type: "NA".to_string(),
            ref_name,
        };
        log_git_content_ref(ctx, &repo, &info).await;
    }
    println!("{}", success_msg);
    Ok(())
}
