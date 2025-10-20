/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use blobstore::Blobstore;
use clap::Parser;
use context::SessionClass;
use git_types::CGDMComponents;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use mononoke_types::ThriftConvert;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;

#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    /// Blobstore key for the CGDMComponents blob.
    #[clap(long)]
    blobstore_key: String,
}

#[derive(Clone)]
#[facet::container]
pub struct Repo {
    #[facet]
    repo_blobstore: RepoBlobstore,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let mut ctx = app.new_basic_context();
    // Force this binary to write to all blobstores
    ctx.session_mut()
        .override_session_class(SessionClass::Background);

    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;

    let cgdm_components = match repo.repo_blobstore().get(&ctx, &args.blobstore_key).await? {
        Some(bytes) => CGDMComponents::from_bytes(bytes.as_raw_bytes())?,
        None => Default::default(),
    };

    for component in cgdm_components.components {
        println!("{:?}", component);
    }

    Ok(())
}
