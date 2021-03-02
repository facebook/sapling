/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

///! The hg-http crate provides common utilities for dealing setting up and
///! managing http requests for the hg application. This crate specifies how
///! a topic should be treated. Topics may include monitoring, request setup,
///! paths, error handling, etc.
use hg_metrics::increment_counter;
use http_client::{HttpClient, Stats};

pub fn http_client(client_id: impl ToString) -> HttpClient {
    let client_id = client_id.to_string();
    let reporter = move |stats: &Stats| {
        bump_counters(&client_id, stats);
    };
    HttpClient::new().with_event_listeners(|l| {
        l.on_stats(reporter);
    })
}

fn bump_counters(client_id: &str, stats: &Stats) {
    let n = |suffix: &'static str| -> String { format!("http.{}.{}", client_id, suffix) };
    // TODO: gauges: rx_bytes and tx_bytes; histograms: request_time_ms, response_delay_ms
    increment_counter(n("total_rx_bytes"), stats.downloaded);
    increment_counter(n("total_tx_bytes"), stats.uploaded);
    increment_counter(n("num_requests"), stats.requests);
    increment_counter(n("total_request_time_ms"), stats.time.as_millis() as usize);
    increment_counter(
        n("total_response_delay_ms"),
        stats.latency.as_millis() as usize,
    )
}
