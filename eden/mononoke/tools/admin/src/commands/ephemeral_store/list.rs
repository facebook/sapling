/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::Repo;
use anyhow::anyhow;
use anyhow::Result;
use clap::Args;
use context::CoreContext;
use ephemeral_blobstore::BubbleId;
use mononoke_types::ChangesetId;
use std::str::FromStr;

// By default, at most 100 blob keys will be listed for a bubble.
const DEFAULT_MAX_KEYS_FOR_LIST: u32 = 100;

#[derive(Args)]
/// Subcommand to list out the keys of the blobs in a bubble within the ephemeral store.
pub struct EphemeralStoreListArgs {
    /// The ID of any one of the changesets for which the bubble blob data needs to
    /// be listed.
    #[clap(long, short = 'i', conflicts_with = "bubble-id")]
    changeset_id: Option<String>,

    /// The ID of the bubble for which the blob data needs to be listed.
    #[clap(long, short = 'b', conflicts_with = "changeset-id")]
    bubble_id: Option<BubbleId>,

    /// The maximum number of blob keys listed in the output. Defaults to 100.
    #[clap(long, short = 'l', default_value_t = DEFAULT_MAX_KEYS_FOR_LIST)]
    limit: u32,

    /// If specified, the search range starts from this key.
    #[clap(long)]
    start_from: Option<String>,

    /// If specified, the blob keys are returned in sorted order.
    #[clap(long)]
    ordered: bool,
}

pub async fn list_keys(ctx: &CoreContext, repo: &Repo, args: EphemeralStoreListArgs) -> Result<()> {
    let bubble_id = match (&args.bubble_id, &args.changeset_id) {
        (None, Some(id)) => repo
            .repo_ephemeral_store
            .bubble_from_changeset(&ChangesetId::from_str(id)?)
            .await?
            .ok_or_else(|| anyhow!("No bubble exists for changeset ID {}", id)),
        (Some(id), _) => Ok(*id),
        (_, _) => Err(anyhow!(
            "Need to provide either changeset ID or bubble ID as input"
        )),
    }?;
    let mut keys = repo
        .repo_ephemeral_store
        .keys_in_bubble(bubble_id, ctx, args.start_from, args.limit)
        .await?;
    if args.ordered {
        keys.sort();
    }
    for key in keys.iter() {
        println!("{}{}", bubble_id.prefix(), key);
    }
    Ok(())
}
