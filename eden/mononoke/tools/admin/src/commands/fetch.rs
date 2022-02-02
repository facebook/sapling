/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::num::NonZeroU64;

use anyhow::{anyhow, Context, Result};
use blobstore::Loadable;
use bonsai_hg_mapping::{BonsaiHgMapping, BonsaiHgMappingRef};
use bookmarks::Bookmarks;
use clap::{ArgEnum, Parser};
use cmdlib_displaying::{display_content, display_hg_manifest, DisplayChangeset};
use context::CoreContext;
use ephemeral_blobstore::{BubbleId, RepoEphemeralStore, RepoEphemeralStoreRef};
use manifest::{Entry, ManifestOps};
use mononoke_app::args::{ChangesetArgs, RepoArgs};
use mononoke_app::MononokeApp;
use mononoke_types::{ChangesetId, MPath};
use repo_blobstore::{RepoBlobstore, RepoBlobstoreRef};
use repo_identity::{RepoIdentity, RepoIdentityRef};

/// Fetch commit, tree or file data.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    /// Fetch from within this ephemeral bubble
    #[clap(long)]
    bubble: Option<NonZeroU64>,

    /// Path of the tree or file to fetch
    #[clap(long, short = 'p')]
    path: Option<String>,

    /// Format as JSON. Currently works only for changesets.
    #[clap(long)]
    json: bool,

    /// Manifest type to use to find trees or files.
    #[clap(long, short = 'k', arg_enum, default_value_t = ManifestKind::Hg)]
    manifest_kind: ManifestKind,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, ArgEnum)]
pub enum ManifestKind {
    Hg,
    // TODO: Add unode manifest, fsnode and skmf support
}

#[facet::container]
struct Repo {
    #[facet]
    repo_identity: RepoIdentity,
    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,
    #[facet]
    bookmarks: dyn Bookmarks,
    #[facet]
    repo_blobstore: RepoBlobstore,
    #[facet]
    repo_ephemeral_store: RepoEphemeralStore,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    let repo: Repo = app
        .open_repo(&args.repo_args)
        .await
        .context("Failed to open repo")?;

    let changeset_id = args
        .changeset_args
        .resolve_changeset(&ctx, &repo)
        .await
        .context("Failed to resolve changeset")?
        .ok_or_else(|| anyhow!("Changeset not found"))?;

    let blobstore = match args.bubble {
        None => repo.repo_blobstore().clone(),
        Some(bubble_id) => {
            let bubble = repo
                .repo_ephemeral_store()
                .open_bubble(BubbleId::new(bubble_id))
                .await
                .with_context(|| format!("Failed to open bubble {}", bubble_id))?;
            bubble.wrap_repo_blobstore(repo.repo_blobstore().clone())
        }
    };

    match &args.path {
        None => {
            let cs = changeset_id
                .load(&ctx, &blobstore)
                .await
                .with_context(|| format!("Failed to load changeset {}", changeset_id))?;
            let display_cs =
                DisplayChangeset::try_from(&cs).context("Failed to display changeset")?;

            if args.json {
                let json_cs =
                    serde_json::to_string(&display_cs).context("Failed to convert to JSON")?;
                println!("{}", json_cs);
            } else {
                println!("{}", display_cs);
            }
        }

        Some(path) => match args.manifest_kind {
            ManifestKind::Hg => {
                display_hg_entry(&ctx, &repo, &blobstore, changeset_id, path).await?;
            }
        },
    }

    Ok(())
}

async fn display_hg_entry(
    ctx: &CoreContext,
    repo: &Repo,
    blobstore: &RepoBlobstore,
    changeset_id: ChangesetId,
    path: &str,
) -> Result<()> {
    let hg_cs_id = repo
        .bonsai_hg_mapping()
        .get_hg_from_bonsai(ctx, repo.repo_identity().id(), changeset_id)
        .await
        .context("Failed to get corresponding Hg changeset")?
        .ok_or_else(|| anyhow!("No Hg changeset for {}", changeset_id))?;
    let hg_cs = hg_cs_id
        .load(ctx, blobstore)
        .await
        .context("Failed to load Hg changeset")?;
    let entry = if path.is_empty() {
        Entry::Tree(hg_cs.manifestid())
    } else {
        let mpath = MPath::new(path).with_context(|| format!("Invalid path: {}", path))?;
        hg_cs
            .manifestid()
            .find_entry(ctx.clone(), blobstore.clone(), Some(mpath))
            .await
            .context("Failed to traverse manifest")?
            .ok_or_else(|| anyhow!("Path does not exist: {}", path))?
    };
    match entry {
        Entry::Leaf((file_type, id)) => {
            let envelope = id
                .load(ctx, blobstore)
                .await
                .context("Failed to load envelope")?;
            let metadata = filestore::get_metadata(blobstore, ctx, &envelope.content_id().into())
                .await
                .context("Failed to load metadata")?
                .ok_or_else(|| {
                    anyhow!(
                        "Content id {} for file {} in {} not found",
                        id,
                        path,
                        hg_cs_id
                    )
                })?;
            writeln!(std::io::stdout(), "File-Type: {}", file_type)?;
            writeln!(std::io::stdout(), "Size: {}", metadata.total_size)?;
            writeln!(std::io::stdout(), "Content-Id: {}", metadata.content_id)?;
            writeln!(std::io::stdout(), "Sha1: {}", metadata.sha1)?;
            writeln!(std::io::stdout(), "Sha256: {}", metadata.sha256)?;
            writeln!(std::io::stdout(), "Git-Sha1: {}", metadata.git_sha1)?;

            let content = filestore::fetch_concat(blobstore, ctx, envelope.content_id())
                .await
                .context("Failed to load content")?;
            display_content(std::io::stdout(), content)?;
        }
        Entry::Tree(id) => {
            let manifest = id
                .load(ctx, blobstore)
                .await
                .context("Failed to load manifest")?;
            display_hg_manifest(std::io::stdout(), &manifest)?;
        }
    }
    Ok(())
}
