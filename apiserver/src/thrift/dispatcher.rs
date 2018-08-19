// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use actix::{Addr, Arbiter};
use actix::msgs::Execute;
use futures::Future;

use srserver::{ThriftExecutor, ThriftServer};

#[derive(Clone)]
pub struct ThriftDispatcher(pub Addr<Arbiter>);

impl ThriftDispatcher {
    pub fn start<F: FnOnce(Self) -> ThriftServer + Send + 'static>(self, thrift: F) {
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
