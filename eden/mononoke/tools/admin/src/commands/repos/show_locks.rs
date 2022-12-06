/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::Result;
use clap::Parser;
use metaconfig_types::MetadataDatabaseConfig;
use metaconfig_types::RepoReadOnly;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use mononoke_types::RepositoryId;
use repo_lock::RepoLock;
use repo_lock::RepoLockState;

#[derive(Parser)]
pub struct ReposShowLocksArgs {
    /// Only show locked repos and omit all the unlocked ones
    #[clap(long)]
    only_locked: bool,
}

#[facet::container]
struct Repo {
    #[facet]
    lock: dyn RepoLock,
}

pub async fn repos_show_locks(app: MononokeApp, args: ReposShowLocksArgs) -> Result<()> {
    let ReposShowLocksArgs { only_locked } = args;

    let mut dbs = HashSet::new();
    let mut repos_with_unique_dbs = vec![];

    for (name, config) in app.repo_configs().repos.iter() {
        if config.readonly == RepoReadOnly::ReadWrite {
            if let MetadataDatabaseConfig::Remote(remote) = &config.storage_config.metadata {
                if dbs.insert(&remote.primary) {
                    repos_with_unique_dbs.push(name.clone());
                }
            }
        }
    }

    let mut id_to_name: HashMap<RepositoryId, String> = app
        .repo_configs()
        .repos
        .iter()
        .map(|(name, config)| (config.repoid, name.clone()))
        .collect();

    let mut all_status = vec![];

    for name in repos_with_unique_dbs {
        let repo: Repo = app.open_repo(&RepoArgs::from_repo_name(name)).await?;
        for (repo_id, state) in repo.lock.all_repos_lock().await? {
            if !only_locked || matches!(state, RepoLockState::Locked(_)) {
                let repo_name = id_to_name
                    .remove(&repo_id)
                    .unwrap_or_else(|| format!("Repo id {}", repo_id));
                all_status.push((repo_name, state))
            }
        }
    }
    // Let's be consistent about the order we print stuff, to reduce confusion
    all_status.sort_unstable_by_key(|(name, _)| name.clone());
    for (name, state) in all_status {
        println!("{:20} {:?}", name, state);
    }
    Ok(())
}
