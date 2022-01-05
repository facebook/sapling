/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! The hg-http crate provides common utilities for dealing setting up and
//! managing http requests for the hg application. This crate specifies how
//! a topic should be treated. Topics may include monitoring, request setup,
//! paths, error handling, etc.

use std::collections::HashSet;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;

use configmodel::ConfigExt;
use hg_metrics::increment_counter;
use http_client::HttpClient;
use http_client::Request;
use http_client::Stats;
use once_cell::sync::Lazy;
use progress_model::AggregatingProgressBar;
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

pub fn http_client(client_id: impl ToString, config: http_client::Config) -> HttpClient {
    let client_id = client_id.to_string();
    let reporter = move |stats: &Stats| {
        bump_counters(&client_id, stats);
    };
    HttpClient::from_config(config).with_event_listeners(|l| {
        l.on_stats(reporter);
    })
}

pub fn http_config(config: &dyn configmodel::Config) -> http_client::Config {
    return http_client::Config {
        convert_cert: config
            .get_or("http", "convert-cert", || cfg!(windows))
            .unwrap_or(cfg!(windows)),
        ..Default::default()
    };
}

/// Global configuration settings for Mercurial's HTTP client.
#[derive(Debug)]
pub struct HgHttpConfig {
    pub verbose: bool,
    pub disable_tls_verification: bool,
    pub client_info: Option<String>,
    pub unix_socket_path: Option<String>,
    pub unix_socket_domains: HashSet<String>,
}

/// Set a global configuration that will be applied to all HTTP requests in
/// Mercurial's Rust code.
pub fn set_global_config(config: HgHttpConfig) {
    if config.disable_tls_verification {
        tracing::warn!("--insecure flag specified; server TLS certificate will not be verified");
    }


    Request::on_new_request(move |req| {
        if let Some(domain) = req.ctx().url().domain() {
            if config.unix_socket_domains.contains(domain) {
                req.set_auth_proxy_socket_path(config.unix_socket_path.clone());
            }
        }
        req.set_verify_tls_cert(!config.disable_tls_verification)
            .set_verify_tls_host(!config.disable_tls_verification)
            .set_verbose(config.verbose)
            .set_client_info(&config.client_info);
    });
}

/// Setup progress reporting to the main progress registry for the lifetime of
/// this process.
pub fn enable_progress_reporting() {
    let _state = Lazy::force(&PROGRESS_REPORTING_STATE);
}

/// State for progress reporting. Lazily initialized.
static PROGRESS_REPORTING_STATE: Lazy<Box<dyn Send + Sync>> = Lazy::new(|| {
    let trees_bar = AggregatingProgressBar::new("downloading", "bytes");
    let files_bar = AggregatingProgressBar::new("downloading", "bytes");

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

        // TODO: How to tell whether it is downloading or uploading?

        // Consolidate /trees and /files requests into single progress bars.
        let url = req.ctx_mut().url().to_string();
        let mut is_single_bar = false;
        let bar = if url.ends_with("/trees") {
            trees_bar.create_or_extend(0)
        } else if url.ends_with("/files") {
            files_bar.create_or_extend(0)
        } else {
            is_single_bar = true;
            ProgressBar::new("downloading", 0, "bytes")
        };

        bar.set_message(url);

        let req_listeners = req.ctx_mut().event_listeners();
        req_listeners.on_content_length({
            let bar = bar.clone();
            move |_req, n| {
                bar.increase_total(n as _);
            }
        });
        req_listeners.on_download_bytes({
            let bar = bar.clone();
            move |_req, n| {
                bar.increase_position(n as _);
            }
        });
        if is_single_bar {
            req_listeners.on_first_activity(move |_req| {
                let registry = Registry::main();
                registry.register_progress_bar(&bar);
            });
        }
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn test_convert_cert_config() {
        let mut hg_config = BTreeMap::<String, String>::new();

        assert_eq!(cfg!(windows), http_config(&hg_config).convert_cert);

        hg_config.insert("http.convert-cert".into(), "True".into());
        assert!(http_config(&hg_config).convert_cert);

        hg_config.insert("http.convert-cert".into(), "false".into());
        assert!(!http_config(&hg_config).convert_cert);
    }
}
