// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use actix_web::{HttpRequest, HttpResponse};
use actix_web::error::Result;
use actix_web::middleware::{Finished, Middleware, Started};
use slog::Logger;

use super::response_time::ResponseTime;

pub struct SLogger {
    logger: Logger,
}

impl SLogger {
    pub fn new(logger: Logger) -> SLogger {
        SLogger { logger: logger }
    }
}

impl<S> ResponseTime<S> for SLogger {}

impl<S> Middleware<S> for SLogger {
    fn start(&self, req: &mut HttpRequest<S>) -> Result<Started> {
        self.start_timer(req);
        Ok(Started::Done)
    }

    fn finish(&self, req: &mut HttpRequest<S>, resp: &HttpResponse) -> Finished {
        let cost = self.time_cost(req).unwrap_or(0);

        info!(
            self.logger,
            "{} {} {} {:.3}\u{00B5}s",
            resp.status().as_u16(),
            req.method(),
            req.path(),
            cost
        );

        Finished::Done
    }
}
