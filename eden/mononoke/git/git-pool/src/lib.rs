/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(never_type)]
use anyhow::anyhow;
use anyhow::Error;
use git2::Error as Git2Error;
use git2::Repository;
use r2d2::ManageConnection;
use r2d2::Pool;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

const POOL_CONNECTION_TIMEOUT_SEC: u64 = 3600;

#[derive(Debug, Clone)]
pub struct GitPool {
    pool: Pool<GitConnectionManager>,
    /// Semaphore to make sure we are not using to many "blocking" tokio tasks.
    /// Instead we wait in the calling task context before spawning.
    sem: Arc<Semaphore>,
}

impl GitPool {
    pub fn new(path: PathBuf) -> Result<Self, Error> {
        let simultaneous_open_repos = num_cpus::get();
        let manager = GitConnectionManager { path };
        let pool = Pool::builder()
            .max_size(simultaneous_open_repos as u32)
            .connection_timeout(Duration::from_secs(POOL_CONNECTION_TIMEOUT_SEC))
            .build(manager)?;
        Ok(Self {
            pool,
            sem: Arc::new(Semaphore::new(simultaneous_open_repos)),
        })
    }

    pub async fn with<F, T, E>(&self, f: F) -> Result<T, Error>
    where
        F: FnOnce(&Repository) -> Result<T, E> + Send + 'static,
        T: Send + Sync + 'static,
        E: Into<Error> + Send + Sync + 'static,
    {
        let sem = self.sem.clone();
        let pool = self.pool.clone();
        // Note - this tokio::spawn() is an attempt to fix deadlock D31541432.
        let ret = tokio::spawn(async move {
            let permit = sem.acquire_owned().await?;
            let ret = tokio_shim::task::spawn_blocking(move || {
                let result_repo = pool.get()?;
                let repo = match &*result_repo {
                    Ok(repo) => repo,
                    Err(err) => {
                        return Err(anyhow!("error while opening repo: {}", err));
                    }
                };
                let ret = f(repo).map_err(|e| e.into())?;
                drop(result_repo);
                drop(permit);
                Result::<_, Error>::Ok(ret)
            })
            .await??;
            Result::<_, Error>::Ok(ret)
        })
        .await??;

        Ok(ret)
    }
}

#[derive(Debug)]
struct GitConnectionManager {
    path: PathBuf,
}

impl ManageConnection for GitConnectionManager {
    type Connection = Result<Repository, Git2Error>;
    type Error = !;

    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        let repo = Repository::open(&self.path);
        Ok(repo)
    }

    fn is_valid(&self, _conn: &mut Self::Connection) -> Result<(), Self::Error> {
        Ok(())
    }

    fn has_broken(&self, conn: &mut Self::Connection) -> bool {
        conn.is_err()
    }
}
