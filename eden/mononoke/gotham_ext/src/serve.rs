/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context as _, Error};
use cloned::cloned;
use futures::{
    future::{self, TryFutureExt},
    stream::StreamExt,
};
use gotham::handler::Handler;
use hyper::server::conn::Http;
use openssl::ssl::SslAcceptor;
use permission_checker::MononokeIdentitySet;
use quiet_stream::QuietShutdownStream;
use slog::{warn, Logger};
use std::panic::RefUnwindSafe;
use std::sync::Arc;
use tokio::net::TcpListener;

use crate::handler::MononokeHttpHandler;
use crate::socket_data::TlsSocketData;

pub async fn https<H>(
    logger: Logger,
    mut listener: TcpListener,
    acceptor: SslAcceptor,
    capture_session_data: bool,
    trusted_proxy_idents: MononokeIdentitySet,
    handler: MononokeHttpHandler<H>,
) where
    H: Handler + Clone + Send + Sync + 'static + RefUnwindSafe,
{
    let trusted_proxy_idents = Arc::new(trusted_proxy_idents);
    let acceptor = Arc::new(acceptor);

    listener
        .incoming()
        .for_each(move |socket| {
            cloned!(acceptor, logger, handler, trusted_proxy_idents);

            let task = async move {
                let socket = socket.context("Error obtaining socket")?;
                let addr = socket.peer_addr().context("Error reading peer_addr()")?;
                let ssl_socket = tokio_openssl::accept(&acceptor, socket)
                    .await
                    .context("Error performing TLS handshake")?;


                let tls_socket_data = TlsSocketData::from_ssl(
                    ssl_socket.ssl(),
                    trusted_proxy_idents.as_ref(),
                    capture_session_data,
                );

                let service = handler.clone().into_service(addr, Some(tls_socket_data));

                let ssl_socket = QuietShutdownStream::new(ssl_socket);

                Http::new()
                    .serve_connection(ssl_socket, service)
                    .await
                    .context("Error serving connection")?;

                Result::<_, Error>::Ok(())
            };

            tokio::spawn(task.map_err(move |e| {
                warn!(&logger, "HTTPS Server error: {:?}", e);
            }));

            future::ready(())
        })
        .await;
}

pub async fn http<H>(logger: Logger, mut listener: TcpListener, handler: MononokeHttpHandler<H>)
where
    H: Handler + Clone + Send + Sync + 'static + RefUnwindSafe,
{
    listener
        .incoming()
        .for_each(move |socket| {
            cloned!(logger, handler);

            let task = async move {
                let socket = socket.context("Error obtaining socket")?;
                let addr = socket.peer_addr().context("Error reading peer_addr()")?;

                let service = handler.clone().into_service(addr, None);

                let socket = QuietShutdownStream::new(socket);

                Http::new()
                    .serve_connection(socket, service)
                    .await
                    .context("Error serving connection")?;

                Result::<_, Error>::Ok(())
            };

            tokio::spawn(task.map_err(move |e| {
                warn!(&logger, "HTTP Server error: {:?}", e);
            }));

            future::ready(())
        })
        .await;
}
