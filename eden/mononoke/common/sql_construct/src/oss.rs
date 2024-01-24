/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroUsize;

use anyhow::Result;
use fbinit::FacebookInit;
use sql_ext::facebook::MysqlOptions;

use crate::SqlConstruct;
use crate::SqlShardedConstruct;

macro_rules! fb_unimplemented {
    () => {
        unimplemented!("This is implemented only for fbcode_build!")
    };
}

/// Construct a SQL data manager backed by Facebook infrastructure
#[async_trait::async_trait]
pub trait FbSqlConstruct: SqlConstruct + Sized + Send + Sync + 'static {
    fn with_mysql<'a>(_: FacebookInit, _: String, _: &'a MysqlOptions, _: bool) -> Result<Self> {
        fb_unimplemented!()
    }
    async fn with_oss_mysql(
        _: FacebookInit,
        _host: String,
        _port: i16,
        _database: String,
        _secret_group: String,
        _use_secret: String,
        _password_secret: String,
        _readonly: bool,
    ) -> Result<Self> {
        fb_unimplemented!()
    }
}

impl<T: SqlConstruct> FbSqlConstruct for T {}

/// Construct a sharded SQL data manager backed by Facebook infrastructure
pub trait FbSqlShardedConstruct: SqlShardedConstruct + Sized + Send + Sync + 'static {
    fn with_sharded_mysql(
        _: FacebookInit,
        _: String,
        _: NonZeroUsize,
        _: &MysqlOptions,
        _: bool,
    ) -> Result<Self> {
        fb_unimplemented!()
    }
}

impl<T: SqlShardedConstruct> FbSqlShardedConstruct for T {}
