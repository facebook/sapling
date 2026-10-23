/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Open-source builds connect through the public thrift runtime and have no
//! X2P proxy.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use edenfs_error::ConnectError;
use fbthrift_socket::SocketTransport;
use futures::future::FutureExt;
use thrift_streaming_clients::StreamingEdenService;
use thrift_types::fbthrift::binary_protocol::BinaryProtocol;
use tokio_io_timeout::TimeoutStream;
use tokio_uds_compat::UnixStream;

use crate::client::connector::StreamingEdenFsThriftClient;
use crate::client::connector::StreamingEdenFsThriftClientFuture;

// The public Rust Thrift runtime has no Rocket transport, so this speaks
// framed binary over the socket. Streaming methods fail.
pub fn connect(
    socket_file: PathBuf,
    conn_timeout: Duration,
    recv_timeout: Duration,
) -> StreamingEdenFsThriftClientFuture {
    async move {
        let stream = tokio::time::timeout(conn_timeout, UnixStream::connect(&socket_file))
            .await
            .context("Timed out connecting to EdenFS")
            .map_err(|error| ConnectError::ConnectionError(error.to_string()))?
            .context("Unable to connect to EdenFS")
            .map_err(|error| ConnectError::ConnectionError(error.to_string()))?;
        let mut stream = TimeoutStream::new(stream);
        stream.set_read_timeout(Some(recv_timeout));
        let transport = SocketTransport::new_with_error_handler(Box::pin(stream), |error| {
            tracing::error!(target: "transport_errors", thrift_transport_error = ?error);
        });
        let client: StreamingEdenFsThriftClient =
            <dyn StreamingEdenService>::new(BinaryProtocol, transport);
        Ok(client)
    }
    .boxed()
    .shared()
}

pub fn supports_vpnless() -> bool {
    false
}

pub fn proxy_url_http1() -> String {
    String::new()
}
