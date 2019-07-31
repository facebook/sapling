// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use actix::msgs::Execute;
use actix::{Addr, Arbiter};
use futures::Future;
use srserver::service_framework::ServiceFramework;

use srserver::ThriftExecutor;

#[derive(Clone)]
pub struct ThriftDispatcher(pub Addr<Arbiter>);

impl ThriftDispatcher {
    pub fn start<F>(self, thrift: F)
    where
        F: FnOnce(Self) -> ServiceFramework + Send + 'static,
    {
        let arbiter = Arbiter::new("thrift-server");

        arbiter.do_send::<Execute>(Execute::new(move || {
            let mut thrift = thrift(self);

            thrift
                .serve()
                .map_err(|e| eprintln!("Failed to start serve(): {}", e))
        }));
    }
}

impl ThriftExecutor for ThriftDispatcher {
    fn spawn<F>(&self, future: F)
    where
        F: Future<Item = (), Error = ()> + Send + 'static,
    {
        self.0.do_send(Execute::new(move || -> Result<(), ()> {
            Ok(Arbiter::spawn(future))
        }));
    }
}
