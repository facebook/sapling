/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;

use anyhow::Result;
use clap::Args;
use itertools::Itertools;
use live_commit_sync_config::CfgrCurrentCommitSyncConfig;
use live_commit_sync_config::RepoGroup;
use mononoke_app::args::MultiRepoArgs;
use mononoke_app::MononokeApp;
use question::Answer;
use question::Question;
use repo_identity::RepoIdentityRef;
use repo_lock::RepoLockRef;
use repo_lock::RepoLockState;

use super::Repo;

async fn repos_in_group<'a>(
    app: &'_ MononokeApp,
    repo: &'a Repo,
    group: RepoGroup,
    action: &'_ str,
    single_repo: bool,
) -> Result<Vec<Cow<'a, Repo>>> {
    if single_repo {
        Ok(vec![Cow::Borrowed(repo)])
    } else {
        let repos = app
            .open_repos::<Repo>(&MultiRepoArgs {
                repo_id: group
                    .into_vec()
                    .into_iter()
                    .map(|repo_id| repo_id.id())
                    .collect(),
                repo_name: vec![],
            })
            .await?;
        if repos.len() > 1 {
            let q = format!(
                "{} all repos [{}]?",
                action,
                repos.iter().map(|r| r.repo_identity().name()).join(", ")
            );
            if Question::new(&q).confirm() != Answer::YES {
                anyhow::bail!("Not doing operation on user request")
            }
        }
        Ok(repos.into_iter().map(Cow::Owned).collect())
    }
}

#[derive(Args)]
pub struct RepoLockArgs {
    /// Why is the repo being locked
    #[clap(long)]
    reason: String,
    /// Lock this single repo even if it's part of a megarepo
    #[clap(long)]
    single_repo: bool,
}

pub async fn repo_lock(app: &MononokeApp, repo: &Repo, args: RepoLockArgs) -> Result<()> {
    let RepoLockArgs {
        reason,
        single_repo,
    } = args;
    let config = CfgrCurrentCommitSyncConfig::new(app.config_store())?;
    let group = config.repo_group(repo.repo_identity.id()).await?;
    let repos = repos_in_group(app, repo, group, "Lock", single_repo).await?;
    // If necessary to optimise, we need to do a single SQL query for this. A bit tricky because
    // most of our things are made for a single repo.
    for repo in repos {
        repo.repo_lock()
            .set_repo_lock(RepoLockState::Locked(reason.clone()))
            .await?;
        println!("{} locked", repo.repo_identity().name());
    }
    Ok(())
}

#[derive(Args)]
pub struct RepoUnlockArgs {
    /// USE WITH CARE. You must make sure the same path is not writable from both repos,
    /// to prevent repo divergence.
    #[clap(long)]
    allow_disabled_pushredirection: bool,
    /// Lock this single repo even if it's part of a megarepo
    #[clap(long)]
    single_repo: bool,
}

pub async fn repo_unlock(app: &MononokeApp, repo: &Repo, args: RepoUnlockArgs) -> Result<()> {
    let RepoUnlockArgs {
        allow_disabled_pushredirection,
        single_repo,
    } = args;
    let config = CfgrCurrentCommitSyncConfig::new(app.config_store())?;
    let group = config.repo_group(repo.repo_identity.id()).await?;
    if !allow_disabled_pushredirection {
        if let Some(repos) = group.small_repos_with_pushredirection_disabled(&config) {
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
    let repos = repos_in_group(app, repo, group, "Unlock", single_repo).await?;
    for repo in repos {
        repo.repo_lock()
            .set_repo_lock(RepoLockState::Unlocked)
            .await?;
        println!("{} unlocked", repo.repo_identity().name());
    }
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
