/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod fetch;
mod is_chunked;
mod metadata;
mod store;
mod verify;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use clap::ArgGroup;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use ephemeral_blobstore::RepoEphemeralStore;
use filestore::Alias;
use filestore::FetchKey;
use filestore::FilestoreConfig;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use mononoke_types::hash::Sha1;
use mononoke_types::hash::Sha256;
use mononoke_types::ContentId;
use repo_blobstore::RepoBlobstore;

use fetch::FilestoreFetchArgs;
use is_chunked::FilestoreIsChunkedArgs;
use metadata::FilestoreMetadataArgs;
use store::FilestoreStoreArgs;
use verify::FilestoreVerifyArgs;

/// Inspect and interact with the filestore.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: FilestoreSubcommand,
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
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_ephemeral_store: RepoEphemeralStore,

    #[facet]
    filestore_config: FilestoreConfig,
}

#[derive(Subcommand)]
pub enum FilestoreSubcommand {
    /// Fetch a file from the filestore
    Fetch(FilestoreFetchArgs),
    /// Store a file in the filestore
    Store(FilestoreStoreArgs),
    /// Show metadata for a file in the filestore
    Metadata(FilestoreMetadataArgs),
    /// Show whether or not a file in the filestore is chunked
    IsChunked(FilestoreIsChunkedArgs),
    /// Verify a file is fetchable by all of its id types
    Verify(FilestoreVerifyArgs),
}

#[derive(Args)]
#[clap(group(ArgGroup::new("filestore-item-id").args(&["content-id", "content-sha1", "content-sha256"]).required(true)))]
pub struct FilestoreItemIdArgs {
    #[clap(long, short = 'i')]
    content_id: Option<ContentId>,

    #[clap(long)]
    content_sha1: Option<Sha1>,

    #[clap(long)]
    content_sha256: Option<Sha256>,
}

impl FilestoreItemIdArgs {
    fn fetch_key(&self) -> Result<FetchKey> {
        if let Some(content_id) = self.content_id {
            Ok(FetchKey::Canonical(content_id))
        } else if let Some(sha1) = self.content_sha1 {
            Ok(FetchKey::Aliased(Alias::Sha1(sha1)))
        } else if let Some(sha256) = self.content_sha256 {
            Ok(FetchKey::Aliased(Alias::Sha256(sha256)))
        } else {
            Err(anyhow!("Filestore item id required"))
        }
    }
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;

    match args.subcommand {
        FilestoreSubcommand::Fetch(fetch_args) => fetch::fetch(&ctx, &repo, fetch_args).await?,
        FilestoreSubcommand::Store(store_args) => store::store(&ctx, &repo, store_args).await?,
        FilestoreSubcommand::Metadata(metadata_args) => {
            metadata::metadata(&ctx, &repo, metadata_args).await?
        }
        FilestoreSubcommand::IsChunked(is_chunked_args) => {
            is_chunked::is_chunked(&ctx, &repo, is_chunked_args).await?
        }
        FilestoreSubcommand::Verify(verify_args) => {
            verify::verify(&ctx, &repo, verify_args).await?
        }
    }

    Ok(())
}
