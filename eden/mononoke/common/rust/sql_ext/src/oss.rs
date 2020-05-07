/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{facebook::*, *};

use anyhow::Error;
use fbinit::FacebookInit;
use futures_ext::BoxFuture;
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

pub fn myrouter_ready(_: Option<String>, _: MysqlOptions, _: Logger) -> BoxFuture<(), Error> {
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
