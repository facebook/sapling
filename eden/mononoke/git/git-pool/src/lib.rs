/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(never_type)]
use anyhow::{anyhow, Error};
use git2::{Error as Git2Error, Repository};
use r2d2::{ManageConnection, Pool};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct GitPool {
    pool: Pool<GitConnectionManager>,
}

impl GitPool {
    pub fn new(path: PathBuf) -> Result<Self, Error> {
        // TODO: Configurable pool size?
        let manager = GitConnectionManager { path };
        let pool = Pool::builder().max_size(8).build(manager)?;
        Ok(Self { pool })
    }

    pub async fn with<F, T, E>(&self, f: F) -> Result<T, Error>
    where
        F: FnOnce(&Repository) -> Result<T, E> + Send + 'static,
        T: Send + Sync + 'static,
        E: Into<Error> + Send + Sync + 'static,
    {
        let pool = self.pool.clone();

        let ret = tokio_shim::task::spawn_blocking(move || {
            let result_repo = pool.get()?;
            let repo = match &*result_repo {
                Ok(repo) => repo,
                Err(err) => {
                    return Err(anyhow!("error while opening repo: {}", err));
                }
            };
            let ret = f(&repo).map_err(|e| e.into())?;
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
