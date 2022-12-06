/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use live_commit_sync_config::CfgrCurrentCommitSyncConfig;
use mononoke_app::MononokeApp;
use repo_identity::RepoIdentityRef;
use repo_lock::RepoLockRef;
use repo_lock::RepoLockState;

use super::Repo;

#[derive(Args)]
pub struct RepoLockArgs {
    /// Why is the repo being locked
    #[clap(long)]
    reason: String,
}

pub async fn repo_lock(_app: &MononokeApp, repo: &Repo, args: RepoLockArgs) -> Result<()> {
    let RepoLockArgs { reason } = args;
    repo.repo_lock()
        .set_repo_lock(RepoLockState::Locked(reason))
        .await?;
    println!("Repo locked :)");
    Ok(())
}

#[derive(Args)]
pub struct RepoUnlockArgs {
    #[clap(long)]
    allow_disabled_pushredirection: bool,
}

pub async fn repo_unlock(app: &MononokeApp, repo: &Repo, args: RepoUnlockArgs) -> Result<()> {
    let RepoUnlockArgs {
        allow_disabled_pushredirection,
    } = args;
    let config = CfgrCurrentCommitSyncConfig::new(app.config_store())?;
    let repo_group = config.repo_group(repo.repo_identity.id()).await?;
    if !allow_disabled_pushredirection {
        if let Some(repos) = repo_group.small_repos_with_pushredirection_disabled(&config) {
            anyhow::bail!(
                concat!(
                    "The following repos have pushredirection config set but disabled: {:?}\n",
                    "Be careful it does not lead to repo divergence. If this is expected, ",
                    "re-run this command with --allow-disabled-pushredirection"
                ),
                repos
            )
        }
    }
    repo.repo_lock()
        .set_repo_lock(RepoLockState::Unlocked)
        .await?;
    println!("Repo unlocked :)");
    Ok(())
}

#[derive(Args)]
pub struct RepoShowLockArgs {}

pub async fn repo_show_lock(_app: &MononokeApp, repo: &Repo, args: RepoShowLockArgs) -> Result<()> {
    let RepoShowLockArgs {} = args;
    let state = repo.repo_lock().check_repo_lock().await?;
    let state = match state {
        RepoLockState::Unlocked => "unlocked".to_string(),
        RepoLockState::Locked(reason) => format!("locked with reason: {}", reason),
    };
    println!("{} is {}", repo.repo_identity().name(), state);
    println!("Consider using `newadmin repos show-locks` to see locks on all repos");
    Ok(())
}
