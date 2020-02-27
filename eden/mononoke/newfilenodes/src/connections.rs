/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use sql::Connection;
use stats::prelude::*;
use std::time::Instant;
use time_ext::DurationExt;
use tokio_preview::sync::{Semaphore, SemaphorePermit};

use crate::structs::PathWithHash;

define_stats! {
    prefix = "mononoke.filenodes";
    filenodes_conn_latency_ms: histogram(10, 0, 1_000, Average, Count; P 5; P 25; P 50; P 75; P 95; P 99; P 100),
    history_conn_latency_ms: histogram(10, 0, 1_000, Average, Count; P 5; P 25; P 50; P 75; P 95; P 99; P 100),
    paths_conn_latency_ms: histogram(10, 0, 1_000, Average, Count; P 5; P 25; P 50; P 75; P 95; P 99; P 100),
}

pub struct ConnectionGuard<'a> {
    _permit: SemaphorePermit<'a>,
    connection: &'a Connection,
}

impl<'a> AsRef<Connection> for ConnectionGuard<'a> {
    fn as_ref(&self) -> &Connection {
        &self.connection
    }
}

#[derive(Copy, Clone)]
pub enum AcquireReason {
    Filenodes,
    History,
    Paths,
}

struct SemaphoredConnection {
    filenodes: Semaphore,
    history: Semaphore,
    paths: Semaphore,
    connection: Connection,
}

impl SemaphoredConnection {
    fn new(connection: Connection) -> Self {
        Self {
            filenodes: Semaphore::new(1),
            history: Semaphore::new(1),
            paths: Semaphore::new(1),
            connection,
        }
    }

    async fn acquire<'a>(&'a self, reason: AcquireReason) -> ConnectionGuard<'a> {
        use AcquireReason::*;

        let semaphore = match reason {
            Filenodes => &self.filenodes,
            History => &self.history,
            Paths => &self.paths,
        };

        let now = Instant::now();
        let permit = semaphore.acquire().await;
        let elapsed = now.elapsed().as_millis_unchecked() as i64;

        match reason {
            Filenodes => STATS::filenodes_conn_latency_ms.add_value(elapsed),
            History => STATS::history_conn_latency_ms.add_value(elapsed),
            Paths => STATS::paths_conn_latency_ms.add_value(elapsed),
        };

        ConnectionGuard {
            _permit: permit,
            connection: &self.connection,
        }
    }
}

pub struct Connections {
    connections: Vec<SemaphoredConnection>,
}

impl Connections {
    pub fn new(connections: Vec<Connection>) -> Self {
        let connections = connections
            .iter()
            .cloned()
            .map(SemaphoredConnection::new)
            .collect();

        Self { connections }
    }
}

impl Connections {
    pub fn len(&self) -> usize {
        self.connections.len()
    }

    pub async fn acquire<'a>(
        &'a self,
        pwh: &PathWithHash<'_>,
        reason: AcquireReason,
    ) -> ConnectionGuard<'a> {
        let shard = pwh.shard_number(self.connections.len());
        self.acquire_by_shard_number(shard, reason).await
    }

    pub async fn acquire_by_shard_number<'a>(
        &'a self,
        shard: usize,
        reason: AcquireReason,
    ) -> ConnectionGuard<'a> {
        let conn = &self.connections[shard];
        conn.acquire(reason).await
    }
}
