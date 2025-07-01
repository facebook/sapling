/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Args;
use context::CoreContext;
use git_types::GitIdentifier;
use git_types::HeaderState;
use git_types::fetch_git_object;
use git_types::fetch_git_object_bytes;
use git_types::fetch_non_blob_git_object;
use gix_object::ObjectRef::Blob;
use gix_object::ObjectRef::Commit;
use gix_object::ObjectRef::Tag;
use gix_object::ObjectRef::Tree;
use mononoke_types::hash::GitSha1;
use mononoke_types::hash::RichGitSha1;

use super::Repo;

#[derive(Args)]
pub struct FetchArgs {
    /// The Git SHA1 object id (in hex form) of the object that is to be fetched
    #[clap(long)]
    id: GitSha1,
    /// Store the raw bytes of the git object in the specified file instead of printing the parsed object
    #[clap(long)]
    raw_bytes_file: Option<PathBuf>,
    /// The type of the git object to be fetched. Required if the object can be git blob
    #[clap(long, requires = "size")]
    ty: Option<String>,
    /// The size of the git object to be fetched. Required if the object can be git blob
    #[clap(long, requires = "ty")]
    size: Option<u64>,
}

pub async fn fetch(repo: &Repo, ctx: &CoreContext, fetch_args: FetchArgs) -> Result<()> {
    if fetch_args.raw_bytes_file.is_some() {
        fetch_bytes(repo, ctx, fetch_args).await?;
    } else {
        fetch_object(repo, ctx, fetch_args).await?;
    }
    Ok(())
}

async fn fetch_object(repo: &Repo, ctx: &CoreContext, mut fetch_args: FetchArgs) -> Result<()> {
    let ty = fetch_args.ty.take();
    let size = fetch_args.size.take();
    let git_object = match (ty, size) {
        (Some(ty), Some(size)) => {
            let git_ident =
                GitIdentifier::Rich(RichGitSha1::from_sha1(fetch_args.id, ty.leak(), size));
            fetch_git_object(ctx, repo.repo_blobstore.clone(), &git_ident).await?
        }
        _ => {
            let git_hash = fetch_args
                .id
                .to_object_id()
                .with_context(|| format!("Invalid object id {}", fetch_args.id))?;
            fetch_non_blob_git_object(ctx, &repo.repo_blobstore, git_hash.as_ref()).await?
        }
    };
    git_object.with_parsed(|object| match object {
        Tree(tree) => println!("The object is a Git Tree\n\n{:#?}", tree),
        Blob(blob) => println!(
            "The object is a Git Blob\n\n{:#?}",
            String::from_utf8_lossy(blob.data)
        ),
        Commit(commit) => println!("The object is a Git Commit\n\n{:#?}", commit),
        Tag(tag) => println!("The object is a Git Tag\n\n{:#?}", tag),
    });
    Ok(())
}

async fn fetch_bytes(repo: &Repo, ctx: &CoreContext, fetch_args: FetchArgs) -> Result<()> {
    let git_object_bytes = fetch_git_object_bytes(
        ctx,
        repo.repo_blobstore.clone(),
        &GitIdentifier::Basic(fetch_args.id),
        HeaderState::Included,
    )
    .await?;
    let mut file = File::create(
        fetch_args
            .raw_bytes_file
            .ok_or_else(|| anyhow::anyhow!("Path is required for writing raw output bytes"))?,
    )?;
    file.write_all(&git_object_bytes)?;
    Ok(())
}
