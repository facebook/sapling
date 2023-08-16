/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;
use repo_lock::RepoLock;
use repo_lock::RepoLockRef;
use repo_lock::RepoLockState;

#[derive(Args)]
pub struct LockingLockArgs {
    #[clap(flatten)]
    repo: RepoArgs,
    /// Why is the repo being locked
    #[clap(long)]
    reason: String,
}

#[derive(Clone)]
#[facet::container]
pub struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_lock: dyn RepoLock,
}

pub async fn locking_lock(app: &MononokeApp, args: LockingLockArgs) -> Result<()> {
    let repo: Repo = app.open_repo(&args.repo).await?;
    repo.repo_lock()
        .set_repo_lock(RepoLockState::Locked(args.reason.clone()))
        .await?;
    println!("{} locked", repo.repo_identity().name());
    Ok(())
}

#[derive(Args)]
pub struct LockingUnlockArgs {
    #[clap(flatten)]
    repo: RepoArgs,
}

pub async fn locking_unlock(app: &MononokeApp, args: LockingUnlockArgs) -> Result<()> {
    let repo: Repo = app.open_repo(&args.repo).await?;
    repo.repo_lock()
        .set_repo_lock(RepoLockState::Unlocked)
        .await?;
    println!("{} unlocked", repo.repo_identity().name());
    Ok(())
}
