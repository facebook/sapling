/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Result;
use clap::Args;
use context::CoreContext;
use ephemeral_blobstore::BubbleId;
use mononoke_types::ChangesetId;

use super::Repo;

#[derive(Args)]
/// Subcommand to describe metadata associated with a bubble within the ephemeral store.
pub struct EphemeralStoreInfoArgs {
    /// The ID of any one of the changesets for which the bubble metadata is
    /// requested.
    #[clap(long, short = 'i', conflicts_with = "bubble_id")]
    changeset_id: Option<String>,

    /// The ID of the bubble for which the metadata is requested.
    #[clap(long, short = 'b', conflicts_with = "changeset_id")]
    bubble_id: Option<BubbleId>,
}

pub async fn bubble_info(
    ctx: &CoreContext,
    repo: &Repo,
    args: EphemeralStoreInfoArgs,
) -> Result<()> {
    let bubble_id = match (&args.bubble_id, &args.changeset_id) {
        (None, Some(id)) => repo
            .repo_ephemeral_store
            .bubble_from_changeset(ctx, &ChangesetId::from_str(id)?)
            .await?
            .ok_or_else(|| anyhow!("No bubble exists for changeset ID {}", id)),
        (Some(id), _) => Ok(*id),
        (_, _) => Err(anyhow!(
            "Need to provide either changeset ID or bubble ID as input"
        )),
    }?;
    let changeset_ids = match &args.changeset_id {
        None => {
            repo.repo_ephemeral_store
                .changesets_from_bubble(ctx, &bubble_id)
                .await?
        }
        Some(id) => vec![ChangesetId::from_str(id)?],
    };
    let bubble = repo
        .repo_ephemeral_store
        .open_bubble_raw(ctx, bubble_id)
        .await?;
    println!(
        "BubbleID: {}\nChangesetIDs: {:?}\nRepoID: {}\nExpiryDate: {}\nStatus: {}\nBlobstorePrefix: {}",
        bubble_id,
        &changeset_ids,
        repo.repo_identity.id(),
        bubble.expires_at(),
        bubble.expired(),
        bubble_id.prefix(),
    );
    Ok(())
}
