// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use actix_web::{HttpRequest, HttpResponse};
use actix_web::error::Result;
use actix_web::middleware::{Finished, Middleware, Started};
use scuba_ext::ScubaSampleBuilder;

use super::response_time::ResponseTime;

pub struct ScubaMiddleware {
    scuba: ScubaSampleBuilder,
}

impl ScubaMiddleware {
    pub fn new(table_name: Option<String>) -> ScubaMiddleware {
        let mut scuba = if let Some(table_name) = table_name {
            ScubaSampleBuilder::new(table_name)
        } else {
            ScubaSampleBuilder::with_discard()
        };

        scuba.add_common_server_data();

        ScubaMiddleware { scuba }
    }
}

impl<S> ResponseTime<S> for ScubaMiddleware {}

impl<S> Middleware<S> for ScubaMiddleware {
    fn start(&self, req: &HttpRequest<S>) -> Result<Started> {
        let mut scuba = self.scuba.clone();

        {
            let info = req.connection_info();
            scuba.add("hostname", info.host());
            if let Some(remote) = info.remote() {
                scuba.add("client", remote);
            }
        }

        scuba
            .add("type", "http")
            .add("method", req.method().to_string())
            .add("path", req.path());
        req.extensions_mut().insert(scuba);

        self.start_timer(req);

        Ok(Started::Done)
    }

    fn finish(&self, req: &HttpRequest<S>, resp: &HttpResponse) -> Finished {
        let response_time = self.time_cost(req);

        if let Some(scuba) = req.extensions_mut().get_mut::<ScubaSampleBuilder>() {
            scuba.add("status_code", resp.status().as_u16());
            scuba.add("response_size", resp.response_size());

            if let Some(time) = response_time {
                scuba.add("response_time", time);
            }

            scuba.log();
        }

        Finished::Done
    }
}
