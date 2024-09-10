/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use blobstore::Loadable;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::Bookmarks;
use clap::Parser;
use clap::ValueEnum;
use cmdlib_displaying::display_content;
use cmdlib_displaying::display_fsnode_manifest;
use cmdlib_displaying::display_hg_manifest;
use cmdlib_displaying::DisplayChangeset;
use context::CoreContext;
use ephemeral_blobstore::BubbleId;
use ephemeral_blobstore::RepoEphemeralStore;
use ephemeral_blobstore::RepoEphemeralStoreRef;
use filestore::FetchKey;
use fsnodes::RootFsnodeId;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_types::HgChangesetId;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use mononoke_types::path::MPath;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;

/// Fetch commit, tree or file data.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    /// Fetch from within this ephemeral bubble
    #[clap(long)]
    bubble_id: Option<BubbleId>,

    /// Path of the tree or file to fetch
    #[clap(long, short = 'p')]
    path: Option<String>,

    /// Format as JSON. Currently works only for changesets.
    #[clap(long)]
    json: bool,

    /// Manifest type to use to find trees or files.
    #[clap(long, short = 'k', value_enum, default_value_t = ManifestKind::Hg)]
    manifest_kind: ManifestKind,
}

#[facet::container]
pub struct Repo {
    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_ephemeral_store: RepoEphemeralStore,

    #[facet]
    repo_derived_data: RepoDerivedData,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, ValueEnum)]
pub enum ManifestKind {
    Hg,
    Fsnode,
    // TODO: Add unode manifest and skmf support
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    let repo: Repo = app
        .open_repo(&args.repo_args)
        .await
        .context("Failed to open repo")?;

    let changeset_id = args
        .changeset_args
        .resolve_changeset(&ctx, &repo)
        .await
        .context("Failed to resolve changeset")?;

    let blobstore = match args.bubble_id {
        None => repo.repo_blobstore().clone(),
        Some(bubble_id) => {
            let bubble = repo
                .repo_ephemeral_store()
                .open_bubble(&ctx, bubble_id)
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
                let hg_changeset_id = repo
                    .bonsai_hg_mapping()
                    .get_hg_from_bonsai(&ctx, changeset_id)
                    .await
                    .context("Failed to get corresponding Hg changeset")?
                    .ok_or_else(|| anyhow!("No Hg changeset for {}", changeset_id))?;
                display_hg_entry(&ctx, &blobstore, hg_changeset_id, path).await?;
            }
            ManifestKind::Fsnode => {
                display_fsnode(&ctx, &repo, &blobstore, changeset_id, path).await?;
            }
        },
    }

    Ok(())
}

async fn display_content_info(
    ctx: &CoreContext,
    blobstore: &RepoBlobstore,
    fetch_key: &FetchKey,
) -> Result<()> {
    let metadata = filestore::get_metadata(blobstore, ctx, fetch_key)
        .await
        .context("Failed to load metadata")?
        .ok_or_else(|| anyhow!("Content for file {} not found", fetch_key))?;
    writeln!(std::io::stdout(), "Size: {}", metadata.total_size)?;
    writeln!(std::io::stdout(), "Content-Id: {}", metadata.content_id)?;
    writeln!(std::io::stdout(), "Sha1: {}", metadata.sha1)?;
    writeln!(std::io::stdout(), "Sha256: {}", metadata.sha256)?;
    writeln!(std::io::stdout(), "Git-Sha1: {}", metadata.git_sha1)?;

    let content = filestore::fetch_concat(blobstore, ctx, fetch_key.clone())
        .await
        .context("Failed to load content")?;
    display_content(std::io::stdout(), content)?;
    Ok(())
}

async fn display_fsnode(
    ctx: &CoreContext,
    repo: &Repo,
    blobstore: &RepoBlobstore,
    cs_id: ChangesetId,
    path: &str,
) -> Result<()> {
    let root_fsnode_id = repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, cs_id)
        .await?;
    let fsnode = if path.is_empty() {
        Entry::Tree(root_fsnode_id.fsnode_id().clone())
    } else {
        let mpath = MPath::new(path).with_context(|| format!("Invalid path: {}", path))?;
        root_fsnode_id
            .fsnode_id()
            .find_entry(ctx.clone(), blobstore.clone(), mpath)
            .await?
            .ok_or_else(|| anyhow!("Path does not exist: {}", path))?
    };
    match fsnode {
        Entry::Leaf(fsnode_file) => {
            writeln!(std::io::stdout(), "File-Type: {}", fsnode_file.file_type())?;
            display_content_info(ctx, blobstore, &fsnode_file.content_id().clone().into()).await?;
        }
        Entry::Tree(id) => {
            let manifest = id
                .load(ctx, blobstore)
                .await
                .context("Failed to load manifest")?;
            display_fsnode_manifest(std::io::stdout(), &manifest)?;
        }
    }
    Ok(())
}

async fn display_hg_entry(
    ctx: &CoreContext,
    blobstore: &RepoBlobstore,
    hg_changeset_id: HgChangesetId,
    path: &str,
) -> Result<()> {
    let hg_cs = hg_changeset_id
        .load(ctx, blobstore)
        .await
        .context("Failed to load Hg changeset")?;
    let entry = if path.is_empty() {
        Entry::Tree(hg_cs.manifestid())
    } else {
        let mpath = MPath::new(path).with_context(|| format!("Invalid path: {}", path))?;
        hg_cs
            .manifestid()
            .find_entry(ctx.clone(), blobstore.clone(), mpath)
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
            writeln!(std::io::stdout(), "File-Type: {}", file_type)?;
            display_content_info(ctx, blobstore, &envelope.content_id().into()).await?;
        }
        Entry::Tree(id) => {
            let manifest = id
                .load(ctx, blobstore)
                .await
                .context("Failed to load manifest")?;
            display_hg_manifest(ctx, blobstore, std::io::stdout(), &manifest).await?;
        }
    }
    Ok(())
}
