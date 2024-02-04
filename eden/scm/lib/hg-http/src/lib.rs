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
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;

use auth::AuthSection;
use clientinfo::ClientInfo;
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
use url::Url;

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

pub fn current_progress() -> (usize, usize, usize) {
    (
        TOTAL.download_bytes.load(Relaxed),
        TOTAL.upload_bytes.load(Relaxed),
        TOTAL.request_count.load(Relaxed),
    )
}

pub fn http_client(client_id: impl ToString, config: http_client::Config) -> HttpClient {
    let client_id = client_id.to_string();
    let reporter = move |stats: &Stats| {
        bump_counters(&client_id, stats);
    };
    HttpClient::from_config(config).with_event_listeners(|l| {
        l.on_stats(reporter);
    })
}

/// Generate http_client::Config taking into account hg specific config/auth
/// settings. url_for_auth will should be the URL to connect to, and will be
/// used to look up TLS credentials.
pub fn http_config(
    config: &dyn configmodel::Config,
    url_for_auth: &Url,
) -> Result<http_client::Config, auth::MissingCerts> {
    let mut hc = http_client::Config {
        client_info: ClientInfo::new().and_then(|i| i.to_json()).ok(),
        disable_tls_verification: INSECURE_MODE.load(Relaxed),
        unix_socket_path: config
            .get_nonempty_opt("auth_proxy", "unix_socket_path")
            .expect("Can't get auth_proxy.unix_socket_path config"),
        unix_socket_domains: HashSet::from_iter(
            config
                .get_or("auth_proxy", "unix_socket_domains", Vec::new)
                .unwrap_or_else(|_| vec![]),
        ),
        verbose: config.get_or_default("http", "verbose").unwrap_or(false),
        ..Default::default()
    };

    if let Some(convert) = config.get_opt("http", "convert-cert").unwrap_or_default() {
        hc.convert_cert = convert;
    }

    let using_auth_proxy = hc.unix_socket_path.is_some()
        && url_for_auth
            .domain()
            .map_or(false, |d| hc.unix_socket_domains.contains(d));

    if !using_auth_proxy {
        // If we aren't using auth proxy, we need to configure client certs.
        // Defer attempt to load certs until we know we need them.
        let auth = AuthSection::from_config(config).best_match_for(url_for_auth)?;
        (hc.cert_path, hc.key_path, hc.ca_path) = auth
            .map(|auth| (auth.cert, auth.key, auth.cacerts))
            .unwrap_or_default();
    }

    Ok(hc)
}

static INSECURE_MODE: AtomicBool = AtomicBool::new(false);

pub fn enable_insecure_mode() {
    INSECURE_MODE.store(true, Relaxed);
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
    let lfs_bar = AggregatingProgressBar::new("downloading", "bytes");

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
        let mut url = req.ctx_mut().url().to_string();
        let mut is_single_bar = false;
        let bar = if url.ends_with("/trees") {
            trees_bar.create_or_extend(0)
        } else if url.ends_with("/files") || url.ends_with("/files2") {
            files_bar.create_or_extend(0)
        } else if let Some((prefix, _)) = url.split_once("/download/") {
            // Strip out the fetch key after /download/.
            url = format!("{}/download/... (LFS)", prefix);
            lfs_bar.create_or_extend(0)
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
        let mut hg_config = BTreeMap::<&str, &str>::new();

        let url: Url = "https://example.com".parse().unwrap();

        hg_config.insert("http.convert-cert", "True");
        assert!(http_config(&hg_config, &url).unwrap().convert_cert);

        hg_config.insert("http.convert-cert", "false");
        assert!(!http_config(&hg_config, &url).unwrap().convert_cert);
    }
}
