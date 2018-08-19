// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

mod dispatcher;
mod fb303;
mod mononoke;

use actix::Arbiter;
use failure::Error;
use slog::Logger;

use apiserver_thrift::server::make_MononokeAPIService_server;
use fb303::server::make_FacebookService_server;
use srserver::ThriftServerBuilder;

use self::dispatcher::ThriftDispatcher;
use self::fb303::FacebookServiceImpl;
use self::mononoke::MononokeAPIServiceImpl;

pub fn make_thrift(logger: Logger, host: String, port: i32) -> Result<(), Error> {
    let dispatcher = ThriftDispatcher(Arbiter::new("thrift-worker"));

    dispatcher.start(move |dispatcher| {
        info!(logger, "Starting thrift service at {}:{}", host, port);
        ThriftServerBuilder::new()
            .with_address(&host, port, false)
            .expect(&format!("cannot bind to {}:{}", host, port))
            .with_tls()
            .expect("cannot bind to tls")
            .with_factory(dispatcher, move || {
                move |proto| {
                    make_MononokeAPIService_server(proto, MononokeAPIServiceImpl {}, |proto| {
                        make_FacebookService_server(proto, FacebookServiceImpl {})
                    })
                }
            })
            .build()
    });

    Ok(())
}
