/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::anyhow;
use anyhow::Result;
use clap::Parser;
use metaconfig_types::RepoReadOnly;
use mononoke_app::args::MultiRepoArgs;
use mononoke_app::args::RepoArg;
use mononoke_app::MononokeApp;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;
use repo_lock::RepoLock;
use repo_lock::RepoLockRef;
use repo_lock::RepoLockState;

#[derive(Parser)]
pub struct LockingStatusArgs {
    #[clap(flatten)]
    repo: MultiRepoArgs,

    /// Only show locked repos and omit all the unlocked ones
    #[clap(long)]
    only_locked: bool,
}

#[facet::container]
struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_lock: dyn RepoLock,
}

pub async fn locking_status(app: &MononokeApp, args: LockingStatusArgs) -> Result<()> {
    let repos = args.repo.ids_or_names()?;
    let repo_configs = app.repo_configs();

    let repo_ids = if repos.is_empty() {
        repo_configs
            .repos
            .values()
            .filter_map(|config| {
                (config.readonly == RepoReadOnly::ReadWrite).then_some(config.repoid)
            })
            .collect()
    } else {
        let mut repo_ids = HashSet::with_capacity(repos.len());
        for repo in repos {
            let repo_id = match repo {
                RepoArg::Id(id) => id,
                RepoArg::Name(name) => {
                    repo_configs
                        .repos
                        .get(&name)
                        .ok_or_else(|| anyhow!("Invalid repo name: {name}"))?
                        .repoid
                }
            };
            repo_ids.insert(repo_id);
        }
        repo_ids
    };

    let repos = app
        .open_repos::<Repo>(&MultiRepoArgs {
            repo_id: repo_ids.into_iter().map(|repo_id| repo_id.id()).collect(),
            repo_name: vec![],
        })
        .await?;

    let mut all_status = Vec::with_capacity(repos.len());

    for repo in repos {
        let state = repo.repo_lock().check_repo_lock().await?;
        if !args.only_locked || matches!(state, RepoLockState::Locked(_)) {
            all_status.push((repo.repo_identity().name().to_owned(), state));
        }
    }

    // Let's be consistent about the order we print stuff, to reduce confusion
    all_status.sort_unstable_by_key(|(name, _)| name.clone());

    for (name, state) in all_status {
        println!("{:20} {:?}", name, state);
    }
    Ok(())
}
