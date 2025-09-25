/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

extern crate fbinit;
extern crate fbthrift;
extern crate proc_macro;
extern crate syn;
extern crate thrift_streaming_clients;

use fbthrift::thrift_protocol::ProtocolID;
use std::net::SocketAddr;
use std::path::PathBuf;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
};

pub use fbthrift_socket::SocketTransport;
pub use fbthrift::CompactProtocol;
use std::sync::Arc;
pub use thrift_streaming_clients::StreamingEdenServiceExt;
pub use thrift_streaming_clients::StreamingEdenServiceImpl;
pub use edenfs_error::ConnectError;

#[macro_export]
macro_rules! make_StreamingEdenServiceExt_thriftclient {
    
    ($fbinit:expr, $($key:ident = $value:expr),* $(,)?) => {
        Result::<_, anyhow::Error>::Ok(
            Arc::new(<$crate::StreamingEdenServiceImpl<$crate::CompactProtocol, $crate::SocketTransport<tokio::net::UnixStream>>>::new(
                $crate::SocketTransport::new(
                    tokio::net::UnixStream::connect("foo")
                    .await
                    .map_err(|e| ConnectError::ConnectionError(e.to_string()))?
                )
            ))
        )
    };
}
