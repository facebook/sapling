/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use bonsai_tag_mapping::Freshness;
use bookmarks::BookmarkCategory;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkName;
use bookmarks::BookmarksRef;
use clap::Args;
use commit_id::IdentityScheme;
use commit_id::print_commit_id;
use context::CoreContext;

use super::Repo;

#[derive(Args)]
pub struct BookmarksGetArgs {
    /// Name of the bookmark to get
    name: BookmarkName,

    /// Category of the bookmark to get
    #[clap(long, default_value = "branch")]
    category: BookmarkCategory,

    /// Commit identity schemes to display
    #[clap(long, short='S', value_enum, default_values = &["bonsai"], use_value_delimiter = true)]
    schemes: Vec<IdentityScheme>,
}

pub async fn get(ctx: &CoreContext, repo: &Repo, get_args: BookmarksGetArgs) -> Result<()> {
    let key = BookmarkKey::with_name_and_category(get_args.name, get_args.category);
    let bookmark_value = repo
        .bookmarks()
        .get(ctx.clone(), &key, bookmarks::Freshness::MostRecent)
        .await
        .with_context(|| format!("Failed to resolve bookmark '{}'", key))?;
    match bookmark_value {
        None => println!("(not set)"),
        Some(cs_id) => {
            // If the bookmark is a tag, print the ID of the changeset containing the
            // metadata associated with the tag along with the changeset that it points to.
            if key.is_tag() {
                let metadata_changeset = repo
                    .bonsai_tag_mapping
                    .get_entry_by_tag_name(
                        ctx,
                        key.name().clone().into_string(),
                        Freshness::MaybeStale,
                    )
                    .await?
                    .map(|entry| entry.changeset_id);
                match metadata_changeset {
                    Some(metadata_changeset) => {
                        println!("Metadata changeset for tag bookmark {}: ", key.name());
                        print_commit_id(ctx, repo, &[], metadata_changeset).await?;
                    }
                    None => println!(
                        "Metadata changeset doesn't exist for tag bookmark {}",
                        key.name()
                    ),
                }
                println!("Changeset pointed to by the tag bookmark {}", key.name());
            }
            print_commit_id(ctx, repo, &get_args.schemes, cs_id).await?
        }
    }

    Ok(())
}
