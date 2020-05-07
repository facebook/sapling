/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use fbinit::FacebookInit;
use sql_ext::facebook::{MysqlOptions, ReadConnectionType};

use crate::{SqlConstruct, SqlShardedConstruct};

macro_rules! fb_unimplemented {
    () => {
        unimplemented!("This is implemented only for fbcode_build!")
    };
}

/// Construct a SQL data manager backed by Facebook infrastructure
#[async_trait]
pub trait FbSqlConstruct: SqlConstruct + Sized + Send + Sync + 'static {
    fn with_myrouter(_: String, _: u16, _: ReadConnectionType, _: bool) -> Self {
        fb_unimplemented!()
    }

    async fn with_raw_xdb_tier(
        _: FacebookInit,
        _: String,
        _: ReadConnectionType,
        _: bool,
    ) -> Result<Self> {
        fb_unimplemented!()
    }

    async fn with_xdb(_: FacebookInit, _: String, _: MysqlOptions, _: bool) -> Result<Self> {
        fb_unimplemented!()
    }
}

impl<T: SqlConstruct> FbSqlConstruct for T {}

/// Construct a sharded SQL data manager backed by Facebook infrastructure
#[async_trait]
pub trait FbSqlShardedConstruct: SqlShardedConstruct + Sized + Send + Sync + 'static {
    fn with_sharded_myrouter(_: String, _: usize, _: u16, _: ReadConnectionType, _: bool) -> Self {
        fb_unimplemented!()
    }

    async fn with_sharded_raw_xdb_tier(
        _: FacebookInit,
        _: String,
        _: usize,
        _: ReadConnectionType,
        _: bool,
    ) -> Result<Self> {
        fb_unimplemented!()
    }

    async fn with_sharded_xdb(
        _: FacebookInit,
        _: String,
        _: usize,
        _: MysqlOptions,
        _: bool,
    ) -> Result<Self> {
        fb_unimplemented!()
    }
}

impl<T: SqlShardedConstruct> FbSqlShardedConstruct for T {}
