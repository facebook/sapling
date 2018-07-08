// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Provide SqliteConnInner and MysqlConnInner logic to share

#![deny(warnings)]
#![feature(try_from, never_type)]

extern crate diesel;
extern crate failure_ext as failure;
extern crate heapsize;
extern crate tokio;

extern crate db;
extern crate lazy_static;

use std::result;
use std::sync::{Arc, Mutex, MutexGuard};

use diesel::{Connection, MysqlConnection, SqliteConnection};
use diesel::connection::SimpleConnection;
use diesel::r2d2::{ConnectionManager, Pool, PooledConnection};
use failure::{Error, Result};

use db::{get_connection_params, ConnectionParams, InstanceRequirement, ProxyRequirement};

#[derive(Clone)]
pub struct SqliteConnInner {
    connection: Arc<Mutex<SqliteConnection>>,
}

impl SqliteConnInner {
    /// Open a SQLite database. This is synchronous because the SQLite backend hits local
    /// disk or memory.
    pub fn open<P: AsRef<str>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let conn = SqliteConnection::establish(path)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(conn)),
        })
    }

    fn create_tables(&mut self, up_query: &str) -> Result<()> {
        self.connection
            .lock()
            .expect("lock poisoned")
            .batch_execute(up_query)?;

        Ok(())
    }

    /// Create a new SQLite database.
    pub fn create<P: AsRef<str>>(path: P, up_query: &str) -> Result<Self> {
        let mut conn = Self::open(path)?;

        conn.create_tables(up_query)?;

        Ok(conn)
    }

    /// Open a SQLite database, and create the tables if they are missing
    pub fn open_or_create<P: AsRef<str>>(path: P, up_query: &str) -> Result<Self> {
        let mut conn = Self::open(path)?;

        let _ = conn.create_tables(up_query);

        Ok(conn)
    }

    /// Create a new in-memory empty database. Great for tests.
    pub fn in_memory(up_query: &str) -> Result<Self> {
        Self::create(":memory:", up_query)
    }

    pub fn get_conn(&self) -> result::Result<MutexGuard<SqliteConnection>, !> {
        Ok(self.connection.lock().expect("lock poisoned"))
    }

    pub fn get_master_conn(&self) -> result::Result<MutexGuard<SqliteConnection>, !> {
        Ok(self.connection.lock().expect("lock poisoned"))
    }
}

#[derive(Clone)]
pub struct MysqlConnInner {
    pool: Pool<ConnectionManager<MysqlConnection>>,
    master_pool: Pool<ConnectionManager<MysqlConnection>>,
}

impl MysqlConnInner {
    pub fn open(db_address: &str) -> Result<Self> {
        let local_connection_params = get_connection_params(
            db_address.to_string(),
            InstanceRequirement::Closest,
            None,
            Some(ProxyRequirement::Forbidden),
        )?;

        let master_connection_params = get_connection_params(
            db_address.to_string(),
            InstanceRequirement::Master,
            None,
            Some(ProxyRequirement::Forbidden),
        )?;

        Self::open_with_params(&local_connection_params, &master_connection_params)
    }

    pub fn open_with_params(
        local_connection_params: &ConnectionParams,
        master_connection_params: &ConnectionParams,
    ) -> Result<Self> {
        let local_url = local_connection_params.to_diesel_url()?;
        let master_url = master_connection_params.to_diesel_url()?;

        let pool = Pool::builder()
            .max_size(10)
            .min_idle(Some(1))
            .build(ConnectionManager::new(local_url.clone()))?;
        let master_pool = Pool::builder()
            .max_size(1)
            .min_idle(Some(1))
            .build(ConnectionManager::new(master_url.clone()))?;
        Ok(Self { pool, master_pool })
    }

    pub fn create_test_db<P: AsRef<str>>(prefix: P, up_query: &str) -> Result<Self> {
        let params = db::create_test_db(prefix)?;
        Self::create(&params, up_query)
    }

    pub fn create(params: &ConnectionParams, up_query: &str) -> Result<Self> {
        let me = Self::from(MysqlConnInner::open_with_params(params, params)?);
        me.get_master_conn()?.batch_execute(up_query)?;
        Ok(me)
    }

    pub fn get_conn(&self) -> Result<PooledConnection<ConnectionManager<MysqlConnection>>> {
        self.pool.get().map_err(Error::from)
    }

    pub fn get_master_conn(&self) -> Result<PooledConnection<ConnectionManager<MysqlConnection>>> {
        self.master_pool.get().map_err(Error::from)
    }
}
