// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::time::Instant;

use actix_web::middleware::Middleware;
use actix_web::HttpRequest;
use time_ext::DurationExt;

#[derive(Clone)]
enum TimeMeasurement {
    StartTime(Instant),
    ResponseTime(u64),
}

pub trait ResponseTime<S>: Middleware<S> {
    fn start_timer(&self, req: &HttpRequest<S>) {
        req.extensions_mut()
            .insert(TimeMeasurement::StartTime(Instant::now()));
    }

    fn time_cost(&self, req: &HttpRequest<S>) -> Option<u64> {
        let time = req.extensions().get::<TimeMeasurement>().map(|x| x.clone());

        if let Some(time) = time {
            match time {
                TimeMeasurement::ResponseTime(t) => Some(t),
                TimeMeasurement::StartTime(t) => {
                    let cost = t.elapsed().as_micros_unchecked();
                    req.extensions_mut()
                        .insert(TimeMeasurement::ResponseTime(cost));

                    Some(cost)
                }
            }
        } else {
            None
        }
    }
}
