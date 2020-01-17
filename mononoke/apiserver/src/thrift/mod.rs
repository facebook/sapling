/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::sync::Arc;
use std::thread;

use cloned::cloned;
use slog::{info, Logger};

use apiserver_thrift::server::make_MononokeAPIService_server;
use fb303::server::make_FacebookService_server;
use fb303_core::server::make_BaseService_server;
use fbinit::FacebookInit;
use srserver::service_framework::{
    BuildModule, Fb303Module, ProfileModule, ServiceFramework, ThriftStatsModule,
};
use srserver::ThriftServerBuilder;
use tokio_compat::runtime::TaskExecutor;

use self::facebook::FacebookServiceImpl;
use self::mononoke::MononokeAPIServiceImpl;
use super::actor::Mononoke;
use scuba_ext::ScubaSampleBuilder;

mod facebook;
mod mononoke;

pub fn make_thrift(
    fb: FacebookInit,
    executor: TaskExecutor,
    logger: Logger,
    host: String,
    port: u16,
    mononoke: Arc<Mononoke>,
    scuba_builder: ScubaSampleBuilder,
) {
    let base = |proto| make_BaseService_server(proto, FacebookServiceImpl);
    let fb_svc = move |proto| make_FacebookService_server(proto, FacebookServiceImpl, base);
    let api_svc = {
        cloned!(logger);
        move |proto| {
            make_MononokeAPIService_server(
                proto,
                MononokeAPIServiceImpl::new(
                    fb,
                    mononoke.clone(),
                    logger.clone(),
                    scuba_builder.clone(),
                ),
                fb_svc.clone(),
            )
        }
    };

    let thrift_server = ThriftServerBuilder::new(fb)
        .with_address(&host, port.into(), false)
        .expect(&format!("cannot bind to {}:{}", host, port))
        .with_tls()
        .expect("cannot bind to tls")
        .with_factory(executor, move || api_svc)
        .build();

    let mut service_framework =
        ServiceFramework::from_server("mononoke_apiserver", thrift_server, port.into())
            .expect("Failed to create ServiceFramework");

    service_framework.add_module(ThriftStatsModule).unwrap();
    service_framework.add_module(Fb303Module).unwrap();
    service_framework.add_module(BuildModule).unwrap();
    service_framework.add_module(ProfileModule).unwrap();

    info!(logger, "Starting thrift service at {}:{}", host, port);

    thread::spawn(move || {
        service_framework
            .serve_blocking()
            .expect("Thrift server did not start");
        panic!("Thrift server should not exit");
    });
}
