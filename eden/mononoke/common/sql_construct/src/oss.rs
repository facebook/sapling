/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use fbinit::FacebookInit;
use sql_ext::facebook::MysqlOptions;

use crate::{SqlConstruct, SqlShardedConstruct};

macro_rules! fb_unimplemented {
    () => {
        unimplemented!("This is implemented only for fbcode_build!")
    };
}

/// Construct a SQL data manager backed by Facebook infrastructure
#[async_trait]
pub trait FbSqlConstruct: SqlConstruct + Sized + Send + Sync + 'static {
    fn with_mysql<'a>(_: FacebookInit, _: String, _: &'a MysqlOptions, _: bool) -> Result<Self> {
        fb_unimplemented!()
    }
}

impl<T: SqlConstruct> FbSqlConstruct for T {}

/// Construct a sharded SQL data manager backed by Facebook infrastructure
#[async_trait]
pub trait FbSqlShardedConstruct: SqlShardedConstruct + Sized + Send + Sync + 'static {
    fn with_sharded_mysql(
        _: FacebookInit,
        _: String,
        _: usize,
        _: &MysqlOptions,
        _: bool,
    ) -> Result<Self> {
        fb_unimplemented!()
    }
}

impl<T: SqlShardedConstruct> FbSqlShardedConstruct for T {}
