/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{facebook::*, *};

use anyhow::Error;
use fbinit::FacebookInit;
use futures_ext::{BoxFuture, FutureExt};
use futures_old::future::ok;
use slog::Logger;

macro_rules! fb_unimplemented {
    () => {
        unimplemented!("This is implemented only for fbcode_build!")
    };
}

impl PoolSizeConfig {
    pub fn for_regular_connection() -> Self {
        fb_unimplemented!()
    }

    pub fn for_sharded_connection() -> Self {
        fb_unimplemented!()
    }
}

pub fn create_myrouter_connections(
    _: String,
    _: Option<usize>,
    _: u16,
    _: ReadConnectionType,
    _: PoolSizeConfig,
    _: String,
    _: bool,
) -> SqlConnections {
    fb_unimplemented!()
}

pub fn myrouter_ready(
    db_addr_opt: Option<String>,
    mysql_options: MysqlOptions,
    _: Logger,
) -> BoxFuture<(), Error> {
    if db_addr_opt.is_none() || mysql_options.myrouter_port.is_none() {
        return ok(()).boxify();
    };

    fb_unimplemented!()
}

pub fn create_raw_xdb_connections(
    _: FacebookInit,
    _: String,
    _: ReadConnectionType,
    _: bool,
) -> BoxFuture<SqlConnections, Error> {
    fb_unimplemented!()
}
