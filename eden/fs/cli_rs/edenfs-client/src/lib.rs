/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use futures::stream::BoxStream;
#[cfg(fbcode_build)]
use thrift_streaming::EdenStartStatusUpdate;
#[cfg(fbcode_build)]
use thrift_streaming_clients::errors::StreamStartStatusStreamError;
#[cfg(fbcode_build)]
use thrift_streaming_clients::StreamingEdenService;
#[cfg(fbcode_build)]
use thrift_types::edenfs_clients::EdenServiceExt;
#[cfg(fbcode_build)]
use thriftclient::ThriftChannel;

pub mod changes_since;
pub mod checkout;
pub mod client;
pub mod daemon_info;
pub mod fsutil;
pub mod instance;
pub mod journal;
mod mounttable;
pub mod redirect;
pub mod sapling;
pub mod utils;

pub use instance::DaemonHealthy;
pub use instance::EdenFsInstance;

#[cfg(fbcode_build)]
pub type EdenFsThriftClient = Arc<dyn EdenServiceExt<ThriftChannel> + Send + Sync + 'static>;
#[cfg(fbcode_build)]
pub type StreamingEdenFsThriftClient = Arc<dyn StreamingEdenService + Sync>;
#[cfg(fbcode_build)]
pub type StartStatusStream =
    BoxStream<'static, Result<EdenStartStatusUpdate, StreamStartStatusStreamError>>;
