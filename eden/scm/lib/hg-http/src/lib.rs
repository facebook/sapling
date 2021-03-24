/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! The hg-http crate provides common utilities for dealing setting up and
//! managing http requests for the hg application. This crate specifies how
//! a topic should be treated. Topics may include monitoring, request setup,
//! paths, error handling, etc.

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;

use once_cell::sync::Lazy;

use hg_metrics::increment_counter;
use http_client::{HttpClient, Request, Stats};
use progress_model::IoSample;
use progress_model::IoTimeSeries;
use progress_model::ProgressBar;
use progress_model::Registry;

#[derive(Default)]
struct Total {
    download_bytes: AtomicUsize,
    upload_bytes: AtomicUsize,
    request_count: AtomicUsize,
}

// Total progress from all clients.
static TOTAL: Total = Total {
    download_bytes: AtomicUsize::new(0),
    upload_bytes: AtomicUsize::new(0),
    request_count: AtomicUsize::new(0),
};

pub fn http_client(client_id: impl ToString) -> HttpClient {
    let client_id = client_id.to_string();
    let reporter = move |stats: &Stats| {
        bump_counters(&client_id, stats);
    };
    HttpClient::new().with_event_listeners(|l| {
        l.on_stats(reporter);
    })
}

/// Setup progress reporting to the main progress registry for the lifetime of
/// this process.
pub fn enable_progress_reporting() {
    let _state = Lazy::force(&PROGRESS_REPORTING_STATE);
}

/// State for progress reporting. Lazily initialized.
static PROGRESS_REPORTING_STATE: Lazy<Box<dyn Drop + Send + Sync>> = Lazy::new(|| {
    Request::on_new_request(move |req| {
        TOTAL.request_count.fetch_add(1, Relaxed);
        let req_listeners = req.ctx_mut().event_listeners();
        req_listeners.on_download_bytes({
            move |_req, n| {
                TOTAL.download_bytes.fetch_add(n, Relaxed);
            }
        });
        req_listeners.on_upload_bytes({
            move |_req, n| {
                TOTAL.upload_bytes.fetch_add(n, Relaxed);
            }
        });

        // Create a progress bar to the main progress registry.
        // TODO: How to tell whether it is downloading or uploading?
        let bar = ProgressBar::new("Downloading", 0, "bytes");
        bar.set_message(req.ctx_mut().url().to_string());

        let req_listeners = req.ctx_mut().event_listeners();
        req_listeners.on_content_length({
            let bar = bar.clone();
            move |_req, n| {
                bar.set_total(n as _);
            }
        });
        req_listeners.on_download_bytes({
            let bar = bar.clone();
            move |_req, n| {
                bar.increase_position(n as _);
            }
        });

        let registry = Registry::main();
        registry.register_progress_bar(&bar);
    });

    // HTTP I/O time series.
    let take_sample = {
        || {
            IoSample::from_io_bytes_count(
                TOTAL.download_bytes.load(Relaxed) as _,
                TOTAL.upload_bytes.load(Relaxed) as _,
                TOTAL.request_count.load(Relaxed) as _,
            )
        }
    };

    let net_time_series = IoTimeSeries::new("HTTP", "requests");
    let task = net_time_series.async_sampling(take_sample, IoTimeSeries::default_sample_interval());
    async_runtime::spawn(task);

    let registry = Registry::main();
    registry.register_io_time_series(&net_time_series);

    Box::new(net_time_series)
});

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
