/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Ok;
use anyhow::Result;
use arc_swap::ArcSwap;
use parking_lot::Mutex;

/// Set of repos currently associated with an instance of Mononoke
/// service or command. This type doesn't derive clone and thus
/// sharing of MononokeRepo should occur under Arc / Rc clones.
pub struct MononokeRepos<R> {
    name_to_repo_map: ArcSwap<HashMap<String, Arc<R>>>,
    id_to_name_map: ArcSwap<HashMap<i32, String>>,
    update_lock: Arc<Mutex<()>>, // Dedicated lock for guarding update operations.
}

impl<R> MononokeRepos<R> {
    /// Creates a new instance of MononokeRepos that starts out
    /// with zero repos.
    pub fn new() -> Self {
        Self {
            name_to_repo_map: ArcSwap::from_pointee(HashMap::new()),
            id_to_name_map: ArcSwap::from_pointee(HashMap::new()),
            update_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Get the repo corresponding to the repo-name if the repo
    /// has been loaded for the service/command, else return None.
    pub fn get_by_name(&self, repo_name: &str) -> Option<Arc<R>> {
        self.name_to_repo_map.load().get(repo_name).map(Arc::clone)
    }

    /// Get the repo corresponding to the repo-id if the repo
    /// has been loaded for the service/command, else return None.
    pub fn get_by_id(&self, repo_id: i32) -> Option<Arc<R>> {
        self.id_to_name_map
            .load()
            .get(&repo_id)
            .and_then(|repo_name| self.name_to_repo_map.load().get(repo_name).map(Arc::clone))
    }

    /// Returns an iterator over the set of repos currently loaded
    /// for the service/command.
    pub fn iter(&self) -> impl Iterator<Item = Arc<R>> {
        let result: Vec<_> = self
            .name_to_repo_map
            .load()
            .iter()
            .map(|(_, repo)| Arc::clone(repo))
            .collect();
        result.into_iter()
    }

    /// Returns an iterator over the set of repo-names corresponding
    /// to the repos currently loaded for the service / command.
    pub fn iter_names(&self) -> impl Iterator<Item = String> {
        let result: Vec<_> = self
            .id_to_name_map
            .load()
            .iter()
            .map(|(_, name)| name.to_string())
            .collect();
        result.into_iter()
    }

    /// Returns an iterator over the set of repo-ids corresponding
    /// to the repos currently loaded for the service / command.
    pub fn iter_ids(&self) -> impl Iterator<Item = i32> {
        let result: Vec<_> = self
            .id_to_name_map
            .load()
            .iter()
            .map(|(id, _)| *id)
            .collect();
        result.into_iter()
    }

    /// Private method that performs the add operations without lock-related
    /// logic. The public accessors to this method ensure that the lock is
    /// acquired before this method is invoked.
    fn add_inner(&self, repo_name: &str, repo_id: i32, repo: R) {
        // First, add the repo-id to repo-name mapping since the actual
        // repo addition should be the last step.
        let id_to_name_map = self.id_to_name_map.load();
        let mut new_id_to_name_map = HashMap::from_iter(
            id_to_name_map
                .iter()
                .map(|(id, name)| (*id, name.to_string())),
        );
        new_id_to_name_map.insert(repo_id, repo_name.to_string());
        self.id_to_name_map.store(Arc::new(new_id_to_name_map));

        // Add the repo-name to repo mapping.
        let name_to_repo_map = self.name_to_repo_map.load();
        let mut new_name_to_repo_map = HashMap::from_iter(
            name_to_repo_map
                .iter()
                .map(|(name, repo)| (name.to_string(), Arc::clone(repo))),
        );
        new_name_to_repo_map.insert(repo_name.to_string(), Arc::new(repo));
        self.name_to_repo_map.store(Arc::new(new_name_to_repo_map));
    }

    /// Adds a new repo corresponding to the provided repo-name
    /// and repo-id. If a repo already exists for that combination,
    /// then it is replaced by the passed in new repo.
    /// NOTE: This is a mutex guarded operation that can induce wait
    /// times for the caller thread. If this isn't desired, use try_add
    /// instead.
    pub fn add(&self, repo_name: &str, repo_id: i32, repo: R) {
        // Acquire the lock to avoid race conditions during update.
        let lock = self.update_lock.lock();
        self.add_inner(repo_name, repo_id, repo);
        // Drop the lock to allow other threads to update the repos.
        drop(lock);
    }

    /// Attempts to add a new repo corresponding to the provided repo-name
    /// and repo-id. If a repo already exists for that combination, then
    /// it is replaced by the passed in new repo.
    /// NOTE: Repo changes are guarded by a mutex. This method attempts
    /// to acquire the lock if it is available, without getting blocked
    /// on the lock.
    pub fn try_add(&self, repo_name: &str, repo_id: i32, repo: R) -> Result<()> {
        // Attempt to acquire the lock before add, to avoid race condition.
        match self.update_lock.try_lock() {
            // Lock acquired, add repo.
            Some(lock) => {
                self.add_inner(repo_name, repo_id, repo);
                drop(lock);
                Ok(())
            }
            // Someone else has the lock, bail.
            None => bail!("Lock could not be acquired for repo {}", repo_name),
        }
    }

    /// Private method that performs the remove operations without lock-related
    /// logic. The public accessors to this method ensure that the lock is
    /// acquired before this method is invoked.
    fn remove_inner(&self, repo_name: &str) {
        // First, remove the repo-id to repo-name mapping that exists
        // for this repo-name.
        let id_to_name_map = self.id_to_name_map.load();
        let new_id_to_name_map =
            HashMap::from_iter(id_to_name_map.iter().filter_map(|(id, name)| {
                if name != repo_name {
                    Some((*id, name.to_string()))
                } else {
                    None
                }
            }));
        self.id_to_name_map.store(Arc::new(new_id_to_name_map));
        // Remove the repo-name to repo mapping.
        let name_to_repo_map = self.name_to_repo_map.load();
        let new_name_to_repo_map =
            HashMap::from_iter(name_to_repo_map.iter().filter_map(|(name, repo)| {
                if name != repo_name {
                    Some((name.to_string(), Arc::clone(repo)))
                } else {
                    None
                }
            }));
        self.name_to_repo_map.store(Arc::new(new_name_to_repo_map));
    }

    /// Removes an existing repo if that repo exists. If it doesn't
    /// then this method is essentially a no-op.
    /// NOTE: This is a mutex guarded operation that can induce wait
    /// times for the caller thread. If this isn't desired, use
    /// try_remove instead.
    pub fn remove(&self, repo_name: &str) {
        // Acquire the lock to avoid race conditions during update.
        let lock = self.update_lock.lock();
        self.remove_inner(repo_name);
        // Drop the lock to allow other threads to update the repos.
        drop(lock);
    }

    /// Attempts to remove an existing repo if that repo exists. If it
    /// doesn't then this method is essentially a no-op.
    /// NOTE: Repo changes are guarded by a mutex. This method attempts
    /// to acquire the lock if it is available, without getting blocked
    /// on the lock.    
    pub fn try_remove(&self, repo_name: &str) -> Result<()> {
        // Attempt to acquire the lock before remove, to avoid race condition.
        match self.update_lock.try_lock() {
            // Lock acquired, remove repo.
            Some(lock) => {
                self.remove_inner(repo_name);
                drop(lock);
                Ok(())
            }
            // Someone else has the lock, bail.
            None => bail!("Lock could not be acquired for repo {}", repo_name),
        }
    }

    /// Method responsible for bulk populating MononokeRepos from an
    /// input iterator of Repos. This method completely discards any previous
    /// repos that were part of MononokeRepos and uses the input to generate
    /// a new collection. Do not use for partial updates.
    /// NOTE: This is a mutex guarded operation that can induce wait
    /// times for the caller thread.
    pub fn populate<I>(&self, repos: I)
    where
        I: IntoIterator<Item = (i32, String, R)>,
    {
        // Acquire the lock to avoid race conditions during update.
        let lock = self.update_lock.lock();
        let mut id_to_name_map: HashMap<i32, String> = HashMap::new();
        let mut name_to_repo_map: HashMap<String, Arc<R>> = HashMap::new();
        for (id, name, repo) in repos.into_iter() {
            id_to_name_map.insert(id, name.to_string());
            name_to_repo_map.insert(name, Arc::new(repo));
        }
        self.id_to_name_map.store(Arc::new(id_to_name_map));
        self.name_to_repo_map.store(Arc::new(name_to_repo_map));
        // Drop the lock to allow other threads to update the repos.
        drop(lock);
    }
}
