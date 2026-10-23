/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! The parts of the client that need internal crates. oss.rs provides the
//! same interface for open-source builds.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use edenfs_error::ConnectError;
use futures::future::FutureExt;
use thrift_streaming_thriftclients::make_StreamingEdenServiceExt_thriftclient;

use crate::client::connector::StreamingEdenFsThriftClient;
use crate::client::connector::StreamingEdenFsThriftClientFuture;

pub fn connect(
    socket_file: PathBuf,
    conn_timeout: Duration,
    recv_timeout: Duration,
) -> StreamingEdenFsThriftClientFuture {
    let fb = fbinit::expect_init();

    tokio::task::spawn(async move {
        tracing::info!(
            "Creating a new EdenFS streaming connection via `{}`",
            socket_file.display()
        );

        // get future for the connection
        let client: StreamingEdenFsThriftClient = make_StreamingEdenServiceExt_thriftclient!(
            fb,
            protocol = CompactProtocol,
            from_path = &socket_file,
            with_conn_timeout = conn_timeout.as_millis() as u32,
            with_recv_timeout = recv_timeout.as_millis() as u32,
            with_secure = false,
        )
        .with_context(|| "Unable to create an EdenFS streaming thrift client")
        .map_err(|e| ConnectError::ConnectionError(e.to_string()))?;

        Ok(client)
    })
    .map(|r| match r {
        Ok(r) => r,
        Err(e) => Err(ConnectError::ConnectionError(e.to_string())),
    })
    .boxed()
    .shared()
}

pub fn supports_vpnless() -> bool {
    cpe::x2p::supports_vpnless()
}

pub fn proxy_url_http1() -> String {
    cpe::x2p::proxy_url_http1()
}
