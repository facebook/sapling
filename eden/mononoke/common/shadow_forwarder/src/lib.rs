/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context as _;
use anyhow::Result;
use bytes::Bytes;
use cached_config::ConfigHandle;
use cached_config::ConfigStore;
use gotham::helpers::http::Body;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use gotham_ext::middleware::Middleware;
use http::HeaderMap;
use http::Method;
use http::Response;
use http::Uri;
use http_body_util::BodyExt as _;
use regex::Regex;
use shadow_traffic_config::ShadowTrafficConfig;
use stats::prelude::*;
use tracing::debug;
use tracing::warn;

define_stats! {
    prefix = "mononoke.shadow";
    skipped_disabled: timeseries(Rate, Sum),
    skipped_sample: timeseries(Rate, Sum),
    skipped_path: timeseries(Rate, Sum),
    skipped_backpressure: timeseries(Rate, Sum),
    skipped_is_shadow: timeseries(Rate, Sum),
    received: timeseries(Rate, Sum),
    eligible: timeseries(Rate, Sum),
    forwarded: timeseries(Rate, Sum),
    forward_success: timeseries(Rate, Sum),
    forward_failure: timeseries(Rate, Sum),
    forward_duration_ms: histogram(1, 0, 10_000, Average, Sum, Count; P 50; P 95; P 99),
    shadow_first_success: timeseries(Rate, Sum),
    shadow_first_fallback: timeseries(Rate, Sum),
    shadow_first_timeout: timeseries(Rate, Sum),
}

pub const SHADOW_HEADER: &str = "x-mononoke-shadow";

/// Headers to forward from the original request to the shadow server
/// for identity, authorization, and content handling.
const FORWARDED_HEADERS: &[&str] = &[
    "x-fb-validated-client-encoded-identity",
    "tfb-orig-client-ip",
    "tfb-orig-client-port",
    "x-client-info",
    "content-type",
    "accept",
];

/// Stores the request body bytes captured during inbound, so outbound
/// can include them in the forwarded request. The handler consumes the
/// original body from State, so we must capture it before that happens.
#[derive(StateData)]
struct ShadowRequestBody(Bytes);

enum ForwardDecision {
    Forward {
        url: String,
        method: Method,
        headers: reqwest::header::HeaderMap,
    },
    Skip,
}

/// Stores the original request URI captured in inbound before the router
/// modifies it. The router may strip path prefixes (e.g., `/edenapi/` or
/// `/repos/git/ro/`), so outbound would see a truncated path. This ensures
/// we forward the full original path to the shadow server.
#[derive(Clone, StateData)]
struct ShadowOriginalUri(String);

pub struct ShadowForwarderMiddleware {
    /// None when config is missing — middleware is disabled but server still starts.
    config_handle: Option<ConfigHandle<ShadowTrafficConfig>>,
    /// Tracks in-flight shadow requests. Compared against the live
    /// config value of semaphore_permits on each request, so changes
    /// to the config take effect immediately without restart.
    inflight: Arc<AtomicUsize>,
    http_client: reqwest::Client,
    cached_include: std::sync::RwLock<CachedRegex>,
    cached_exclude: std::sync::RwLock<CachedRegex>,
    /// When true, this instance is a shadow tier and should never forward.
    /// Set via --shadow-tier CLI flag.
    is_shadow_tier: bool,
}

// request::Client contains dyn trait objects that don't impl RefUnwindSafe,
// but the client is safe to use across unwind boundaries — it's just an
// HTTP connection pool. std::sync::RwLock is RefUnwindSafe.
impl std::panic::RefUnwindSafe for ShadowForwarderMiddleware {}

struct CachedRegex {
    pattern: String,
    compiled: Option<Regex>,
}

impl CachedRegex {
    fn new() -> Self {
        Self {
            pattern: String::new(),
            compiled: None,
        }
    }

    fn get_or_compile(&mut self, pattern: &str) -> Option<&Regex> {
        if self.pattern != pattern {
            self.pattern = pattern.to_string();
            self.compiled = if pattern.is_empty() {
                None
            } else {
                match Regex::new(pattern) {
                    Ok(r) => Some(r),
                    Err(e) => {
                        warn!(
                            pattern = %pattern,
                            error = %e,
                            "Shadow forwarder: invalid regex pattern in config",
                        );
                        None
                    }
                }
            };
        }
        self.compiled.as_ref()
    }
}

impl ShadowForwarderMiddleware {
    pub fn new(
        config_store: &ConfigStore,
        config_path: &str,
        is_shadow_tier: bool,
        tls_ca_path: Option<&Path>,
    ) -> Result<Self> {
        let config_handle =
            match config_store.get_config_handle::<ShadowTrafficConfig>(config_path.to_string()) {
                Ok(handle) => Some(handle),
                Err(e) => {
                    warn!(
                        config_path = %config_path,
                        error = %e,
                        "Shadow forwarder: config not found, middleware disabled",
                    );
                    None
                }
            };

        let mut client_builder =
            reqwest::Client::builder().timeout(std::time::Duration::from_secs(30));

        if let Some(ca_path) = tls_ca_path {
            let ca_cert = std::fs::read(ca_path)
                .with_context(|| format!("Failed to read TLS CA from {}", ca_path.display()))?;
            let cert = reqwest::Certificate::from_pem(&ca_cert)
                .context("Failed to parse TLS CA certificate")?;
            client_builder = client_builder.add_root_certificate(cert);
        }

        let http_client = client_builder.build()?;

        Ok(Self {
            config_handle,
            inflight: Arc::new(AtomicUsize::new(0)),
            http_client,
            cached_include: std::sync::RwLock::new(CachedRegex::new()),
            cached_exclude: std::sync::RwLock::new(CachedRegex::new()),
            is_shadow_tier,
        })
    }

    fn path_matches_include(&self, path: &str, pattern: &str) -> bool {
        if pattern.is_empty() {
            return true;
        }
        let mut cache = self.cached_include.write().expect("poisoned lock");
        cache
            .get_or_compile(pattern)
            .is_some_and(|re| re.is_match(path))
    }
    fn path_matches_exclude(&self, path: &str, pattern: &str) -> bool {
        if pattern.is_empty() {
            return false;
        }
        let mut cache = self.cached_exclude.write().expect("poisoned lock");
        cache
            .get_or_compile(pattern)
            .is_some_and(|re| re.is_match(path))
    }

    /// Extract identity headers from the request state for forwarding.
    fn extract_forwarding_headers(state: &State) -> reqwest::header::HeaderMap {
        let mut forwarding = reqwest::header::HeaderMap::new();
        if let Some(headers) = HeaderMap::try_borrow_from(state) {
            for &name in FORWARDED_HEADERS {
                if let Some(value) = headers.get(name) {
                    if let (Ok(name), Ok(val)) = (
                        reqwest::header::HeaderName::from_bytes(name.as_bytes()),
                        reqwest::header::HeaderValue::from_bytes(value.as_bytes()),
                    ) {
                        forwarding.insert(name, val);
                    }
                }
            }
        }
        forwarding
    }

    /// Check whether the request should be forwarded based on config, sampling,
    /// and path filters. Used by both fire-and-forget (outbound) and
    /// shadow-first (inbound) modes.
    fn should_forward(
        &self,
        state: &State,
        config: &ShadowTrafficConfig,
        skip_sampling: bool,
    ) -> ForwardDecision {
        if !config.enabled {
            STATS::skipped_disabled.add_value(1);
            return ForwardDecision::Skip;
        }

        let sample_percent = if config.sample_ratio > 0 {
            config.sample_ratio.min(100)
        } else {
            STATS::skipped_disabled.add_value(1);
            return ForwardDecision::Skip;
        };

        // Shadow-first skips sampling — it applies to all matching requests
        if !skip_sampling && rand::random::<u64>() % 100 >= (sample_percent as u64) {
            STATS::skipped_sample.add_value(1);
            return ForwardDecision::Skip;
        }

        let path = match state.try_borrow::<ShadowOriginalUri>() {
            Some(original) => original.0.clone(),
            None => match Uri::try_borrow_from(state) {
                Some(uri) => uri
                    .path_and_query()
                    .map(|pq| pq.as_str().to_string())
                    .unwrap_or_else(|| uri.path().to_string()),
                None => return ForwardDecision::Skip,
            },
        };

        if !self.path_matches_include(&path, &config.path_include) {
            STATS::skipped_path.add_value(1);
            return ForwardDecision::Skip;
        }

        if self.path_matches_exclude(&path, &config.path_exclude) {
            STATS::skipped_path.add_value(1);
            return ForwardDecision::Skip;
        }

        if config.target_url.is_empty() {
            return ForwardDecision::Skip;
        }

        STATS::eligible.add_value(1);

        let headers = Self::extract_forwarding_headers(state);
        let method = Method::try_borrow_from(state)
            .cloned()
            .unwrap_or(Method::GET);

        ForwardDecision::Forward {
            url: format!("{}{}", config.target_url, path),
            method,
            headers,
        }
    }
}

#[async_trait::async_trait]
impl Middleware for ShadowForwarderMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        // Capture the original URI before the router modifies it.
        // The router strips path prefixes (e.g., /edenapi/ or /repos/git/ro/),
        // so outbound would see a truncated path without this.
        if let Some(uri) = Uri::try_borrow_from(state) {
            let original_path = uri
                .path_and_query()
                .map(|pq| pq.as_str().to_string())
                .unwrap_or_else(|| uri.path().to_string());
            state.put(ShadowOriginalUri(original_path));
        }

        // Track shadow requests received on this tier
        if let Some(headers) = HeaderMap::try_borrow_from(state) {
            if headers.contains_key(SHADOW_HEADER) {
                STATS::received.add_value(1);
            }
        }

        // Capture the request body for later forwarding in outbound.
        // The handler will consume the body from State, so we take it now,
        // read the bytes, store a copy, and put a new body back.
        if !self.is_shadow_tier {
            let enabled = self
                .config_handle
                .as_ref()
                .map_or(false, |h| h.get().enabled);
            if enabled {
                if let Some(body) = state.try_take::<Body>() {
                    let body_bytes = body
                        .collect()
                        .await
                        .map(|collected| collected.to_bytes())
                        .unwrap_or_default();
                    state.put(ShadowRequestBody(body_bytes.clone()));
                    // Put a new body back so the handler can still consume it
                    use gotham::handler::IntoBody as _;
                    state.put(body_bytes.into_body());
                }
            }
        }

        // Shadow tiers never forward
        if self.is_shadow_tier {
            return None;
        }

        // Loop prevention
        if let Some(headers) = HeaderMap::try_borrow_from(state) {
            if headers.contains_key(SHADOW_HEADER) {
                return None;
            }
        }

        let config_handle = match &self.config_handle {
            Some(h) => h,
            None => return None,
        };
        let config: Arc<ShadowTrafficConfig> = config_handle.get();

        // Shadow-first mode: forward synchronously with timeout, use shadow
        // response on success, fall back to local processing on failure.
        if !config.shadow_first {
            return None;
        }

        let (url, method, fwd_headers) = match self.should_forward(state, &config, true) {
            ForwardDecision::Forward {
                url,
                method,
                headers,
            } => (url, method, headers),
            ForwardDecision::Skip => return None,
        };

        let max_inflight = config.semaphore_permits.max(1) as usize;
        let current = self.inflight.fetch_add(1, Ordering::Relaxed);
        if current >= max_inflight {
            self.inflight.fetch_sub(1, Ordering::Relaxed);
            STATS::skipped_backpressure.add_value(1);
            return None;
        }

        let timeout = Duration::from_millis(config.shadow_first_timeout_ms.max(1) as u64);
        let client = self.http_client.clone();
        let body_bytes = state
            .try_take::<ShadowRequestBody>()
            .map(|b| b.0)
            .unwrap_or_default();

        STATS::forwarded.add_value(1);
        let start = Instant::now();

        let result = tokio::time::timeout(
            timeout,
            client
                .request(method, &url)
                .header(SHADOW_HEADER, "1")
                .headers(fwd_headers)
                .body(body_bytes)
                .send(),
        )
        .await;

        let duration_ms = start.elapsed().as_millis() as i64;
        STATS::forward_duration_ms.add_value(duration_ms);
        self.inflight.fetch_sub(1, Ordering::Relaxed);

        match result {
            Ok(Ok(resp)) if resp.status().is_success() => {
                STATS::shadow_first_success.add_value(1);
                STATS::forward_success.add_value(1);

                let status = resp.status();
                // Copy response headers from shadow
                let mut builder = Response::builder().status(status);
                for (name, value) in resp.headers() {
                    if let Ok(val) = http::HeaderValue::from_bytes(value.as_bytes()) {
                        builder = builder.header(name.as_str(), val);
                    }
                }
                let resp_bytes = resp.bytes().await.unwrap_or_default();
                use gotham::handler::IntoBody as _;
                let response = builder.body(resp_bytes.into_body()).ok()?;
                Some(response)
            }
            Ok(Ok(resp)) => {
                STATS::shadow_first_fallback.add_value(1);
                STATS::forward_failure.add_value(1);
                debug!(
                    url = %url,
                    status = %resp.status(),
                    "Shadow-first non-success, falling back to local",
                );
                None
            }
            Ok(Err(e)) => {
                STATS::shadow_first_fallback.add_value(1);
                STATS::forward_failure.add_value(1);
                warn!(
                    url = %url,
                    error = %e,
                    "Shadow-first failed, falling back to local",
                );
                None
            }
            Err(_) => {
                STATS::shadow_first_timeout.add_value(1);
                STATS::shadow_first_fallback.add_value(1);
                warn!(
                    url = %url,
                    timeout_ms = %timeout.as_millis(),
                    "Shadow-first timed out, falling back to local",
                );
                None
            }
        }
    }

    async fn outbound(&self, state: &mut State, _response: &mut Response<Body>) {
        // Shadow tiers never forward
        if self.is_shadow_tier {
            return;
        }

        // Loop prevention: never forward requests that already have the shadow header
        if let Some(headers) = HeaderMap::try_borrow_from(state) {
            if headers.contains_key(SHADOW_HEADER) {
                STATS::skipped_is_shadow.add_value(1);
                return;
            }
        }

        let config_handle = match &self.config_handle {
            Some(h) => h,
            None => return,
        };
        let config: Arc<ShadowTrafficConfig> = config_handle.get();

        // If shadow-first is enabled, inbound already handled forwarding
        if config.shadow_first {
            return;
        }

        let (url, method, fwd_headers) = match self.should_forward(state, &config, false) {
            ForwardDecision::Forward {
                url,
                method,
                headers,
            } => (url, method, headers),
            ForwardDecision::Skip => return,
        };

        // Dynamic backpressure: compare inflight count against live config value
        let max_inflight = config.semaphore_permits.max(1) as usize;
        let current = self.inflight.fetch_add(1, Ordering::Relaxed);
        if current >= max_inflight {
            self.inflight.fetch_sub(1, Ordering::Relaxed);
            STATS::skipped_backpressure.add_value(1);
            return;
        }

        let client = self.http_client.clone();
        let body_bytes = state
            .try_take::<ShadowRequestBody>()
            .map(|b| b.0)
            .unwrap_or_default();
        let inflight = self.inflight.clone();

        STATS::forwarded.add_value(1);
        mononoke_macros::mononoke::spawn_task(async move {
            let start = Instant::now();
            let result = client
                .request(method, &url)
                .header(SHADOW_HEADER, "1")
                .headers(fwd_headers)
                .body(body_bytes)
                .send()
                .await;

            let duration_ms = start.elapsed().as_millis() as i64;
            STATS::forward_duration_ms.add_value(duration_ms);

            match result {
                Ok(resp) if resp.status().is_success() => {
                    STATS::forward_success.add_value(1);
                }
                Ok(resp) => {
                    STATS::forward_failure.add_value(1);
                    debug!(
                        url = %url,
                        status = %resp.status(),
                        "Shadow forward non-success",
                    );
                }
                Err(e) => {
                    STATS::forward_failure.add_value(1);
                    warn!(
                        url = %url,
                        error = %e,
                        "Shadow forward failed",
                    );
                }
            }

            inflight.fetch_sub(1, Ordering::Relaxed);
        });
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use cached_config::ModificationTime;
    use cached_config::TestSource;
    use gotham::handler::IntoBody as _;
    use http::Request;
    use mononoke_macros::mononoke;

    use super::*;

    fn make_config(
        enabled: bool,
        sample_ratio: i64,
        path_include: &str,
        path_exclude: &str,
    ) -> String {
        make_config_full(
            enabled,
            sample_ratio,
            path_include,
            path_exclude,
            false,
            5000,
        )
    }

    fn make_config_full(
        enabled: bool,
        sample_ratio: i64,
        path_include: &str,
        path_exclude: &str,
        shadow_first: bool,
        shadow_first_timeout_ms: i64,
    ) -> String {
        format!(
            r#"{{
                "enabled": {enabled},
                "sample_ratio": {sample_ratio},
                "path_include": "{path_include}",
                "path_exclude": "{path_exclude}",
                "target_url": "https://shadow.example.com",
                "semaphore_permits": 100,
                "shadow_first_timeout_ms": {shadow_first_timeout_ms},
                "shadow_first": {shadow_first}
            }}"#,
        )
    }

    fn make_middleware_from_config(
        config_json: &str,
        is_shadow_tier: bool,
    ) -> ShadowForwarderMiddleware {
        let test_source = TestSource::new();
        test_source.insert_config(
            "test/shadow_traffic",
            config_json,
            ModificationTime::UnixTimestamp(0),
        );
        let config_store = ConfigStore::new(Arc::new(test_source), None, None);
        ShadowForwarderMiddleware::new(&config_store, "test/shadow_traffic", is_shadow_tier, None)
            .unwrap()
    }

    fn make_test_middleware(
        enabled: bool,
        sample_ratio: i64,
        path_include: &str,
        path_exclude: &str,
    ) -> ShadowForwarderMiddleware {
        make_middleware_from_config(
            &make_config(enabled, sample_ratio, path_include, path_exclude),
            false,
        )
    }

    fn make_test_middleware_shadow_tier(
        enabled: bool,
        sample_ratio: i64,
        path_include: &str,
        path_exclude: &str,
        is_shadow_tier: bool,
    ) -> ShadowForwarderMiddleware {
        make_middleware_from_config(
            &make_config(enabled, sample_ratio, path_include, path_exclude),
            is_shadow_tier,
        )
    }

    fn make_test_middleware_shadow_first(
        enabled: bool,
        sample_ratio: i64,
        timeout_ms: i64,
    ) -> ShadowForwarderMiddleware {
        make_middleware_from_config(
            &make_config_full(enabled, sample_ratio, "", "", true, timeout_ms),
            false,
        )
    }

    fn make_state(path: &str) -> State {
        let req = Request::builder().uri(path).body(Body::default()).unwrap();
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        State::from_request(req, addr)
    }

    fn make_state_with_body(path: &str, method: &str, body: &[u8]) -> State {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .body(body.to_vec().into_body())
            .unwrap();
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        State::from_request(req, addr)
    }

    fn make_shadow_state(path: &str) -> State {
        let req = Request::builder()
            .uri(path)
            .header(SHADOW_HEADER, "1")
            .body(Body::default())
            .unwrap();
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        State::from_request(req, addr)
    }

    fn make_response() -> Response<Body> {
        Response::builder()
            .status(200)
            .body(Body::default())
            .unwrap()
    }

    // --- Config tests ---

    #[mononoke::test]
    fn test_disabled_config() {
        let mw = make_test_middleware(false, 0, "", "");
        let config = mw.config_handle.as_ref().unwrap().get();
        assert!(!config.enabled);
        assert_eq!(config.sample_ratio, 0);
    }

    #[mononoke::test]
    fn test_enabled_config() {
        let mw = make_test_middleware(true, 100, "", "");
        let config = mw.config_handle.as_ref().unwrap().get();
        assert!(config.enabled);
        assert_eq!(config.sample_ratio, 100);
    }

    // --- Path filter tests ---

    #[mononoke::test]
    fn test_path_include_filter() {
        let mw = make_test_middleware(true, 10, "trees|files", "");
        assert!(mw.path_matches_include("/edenapi/trees", "trees|files"));
        assert!(mw.path_matches_include("/edenapi/files", "trees|files"));
        assert!(!mw.path_matches_include("/edenapi/commit", "trees|files"));
    }

    #[mononoke::test]
    fn test_path_include_empty_matches_all() {
        let mw = make_test_middleware(true, 10, "", "");
        assert!(mw.path_matches_include("/anything", ""));
    }

    #[mononoke::test]
    fn test_path_exclude_filter() {
        let mw = make_test_middleware(true, 10, "", "/upload/|/land/|/set_bookmark");
        assert!(mw.path_matches_exclude("/edenapi/upload/token", "/upload/|/land/|/set_bookmark"));
        assert!(mw.path_matches_exclude("/edenapi/land/v2", "/upload/|/land/|/set_bookmark"));
        assert!(!mw.path_matches_exclude("/edenapi/trees", "/upload/|/land/|/set_bookmark"));
    }

    #[mononoke::test]
    fn test_path_exclude_empty_matches_nothing() {
        let mw = make_test_middleware(true, 10, "", "");
        assert!(!mw.path_matches_exclude("/anything", ""));
    }

    #[mononoke::test]
    fn test_regex_caching() {
        let mw = make_test_middleware(true, 10, "", "");
        assert!(mw.path_matches_include("/foo", "foo|bar"));
        assert!(mw.path_matches_include("/bar", "foo|bar"));
        assert!(!mw.path_matches_include("/foo", "baz"));
    }

    #[mononoke::test]
    fn test_invalid_regex_returns_false() {
        let mw = make_test_middleware(true, 10, "", "");
        assert!(!mw.path_matches_include("/foo", "[invalid"));
        assert!(!mw.path_matches_exclude("/foo", "[invalid"));
    }

    #[mononoke::test]
    fn test_inflight_counter_starts_at_zero() {
        let mw = make_test_middleware(true, 10, "", "");
        assert_eq!(mw.inflight.load(Ordering::Relaxed), 0);
    }

    #[mononoke::test]
    fn test_negative_sample_ratio_treated_as_disabled() {
        let mw = make_test_middleware(true, -5, "", "");
        let config = mw.config_handle.as_ref().unwrap().get();
        assert!(config.sample_ratio < 0);
        // The outbound check `config.sample_ratio > 0` will skip forwarding
    }

    #[mononoke::test]
    fn test_default_path_exclude_blocks_mutations() {
        let mw = make_test_middleware(true, 100, "", "/upload/|/land/|/set_bookmark");
        assert!(mw.path_matches_exclude("/edenapi/upload/token", "/upload/|/land/|/set_bookmark"));
        assert!(mw.path_matches_exclude("/edenapi/land/v2", "/upload/|/land/|/set_bookmark"));
        assert!(mw.path_matches_exclude("/edenapi/set_bookmark", "/upload/|/land/|/set_bookmark"));
        assert!(!mw.path_matches_exclude("/edenapi/trees", "/upload/|/land/|/set_bookmark"));
        assert!(!mw.path_matches_exclude("/edenapi/files", "/upload/|/land/|/set_bookmark"));
    }

    // --- should_forward tests ---

    #[mononoke::test]
    fn test_should_forward_disabled() {
        let mw = make_test_middleware(false, 0, "", "");
        let state = make_state("/edenapi/trees");
        let config = mw.config_handle.as_ref().unwrap().get();
        assert!(matches!(
            mw.should_forward(&state, &config, false),
            ForwardDecision::Skip
        ));
    }

    #[mononoke::test]
    fn test_should_forward_zero_sample() {
        let mw = make_test_middleware(true, 0, "", "");
        let state = make_state("/edenapi/trees");
        let config = mw.config_handle.as_ref().unwrap().get();
        assert!(matches!(
            mw.should_forward(&state, &config, false),
            ForwardDecision::Skip
        ));
    }

    #[mononoke::test]
    fn test_should_forward_100_percent() {
        let mw = make_test_middleware(true, 100, "", "");
        let state = make_state("/edenapi/trees");
        let config = mw.config_handle.as_ref().unwrap().get();
        assert!(matches!(
            mw.should_forward(&state, &config, false),
            ForwardDecision::Forward { .. }
        ));
    }

    #[mononoke::test]
    fn test_should_forward_skip_sampling() {
        let mw = make_test_middleware(true, 1, "", "");
        let config = mw.config_handle.as_ref().unwrap().get();
        // With skip_sampling=true, even 1% should always forward
        for _ in 0..100 {
            let state = make_state("/edenapi/trees");
            assert!(matches!(
                mw.should_forward(&state, &config, true),
                ForwardDecision::Forward { .. }
            ));
        }
    }

    #[mononoke::test]
    fn test_should_forward_excluded_path() {
        let mw = make_test_middleware(true, 100, "", "/upload/");
        let state = make_state("/edenapi/upload/token");
        let config = mw.config_handle.as_ref().unwrap().get();
        assert!(matches!(
            mw.should_forward(&state, &config, false),
            ForwardDecision::Skip
        ));
    }

    #[mononoke::test]
    fn test_should_forward_include_mismatch() {
        let mw = make_test_middleware(true, 100, "trees", "");
        let state = make_state("/edenapi/commit");
        let config = mw.config_handle.as_ref().unwrap().get();
        assert!(matches!(
            mw.should_forward(&state, &config, false),
            ForwardDecision::Skip
        ));
    }

    // --- Outbound middleware logic tests ---

    #[mononoke::test]
    async fn test_outbound_disabled_skips() {
        let mw = make_test_middleware(false, 0, "", "");
        let mut state = make_state("/edenapi/trees");
        let mut response = make_response();
        mw.outbound(&mut state, &mut response).await;
    }

    #[mononoke::test]
    async fn test_outbound_zero_sample_ratio_skips() {
        let mw = make_test_middleware(true, 0, "", "");
        let mut state = make_state("/edenapi/trees");
        let mut response = make_response();
        mw.outbound(&mut state, &mut response).await;
    }

    #[mononoke::test]
    async fn test_outbound_excluded_path_skips() {
        let mw = make_test_middleware(true, 10, "", "/upload/|/land/");
        let mut response = make_response();
        for _ in 0..100 {
            let mut state = make_state("/edenapi/upload/token");
            mw.outbound(&mut state, &mut response).await;
        }
    }

    #[mononoke::test]
    async fn test_outbound_include_filter_blocks_non_matching() {
        let mw = make_test_middleware(true, 10, "trees", "");
        let mut response = make_response();
        for _ in 0..100 {
            let mut state = make_state("/edenapi/commit");
            mw.outbound(&mut state, &mut response).await;
        }
    }

    #[mononoke::test]
    async fn test_backpressure_with_low_permits() {
        let config_json = r#"{
            "enabled": true,
            "sample_ratio": 100,
            "path_include": "",
            "path_exclude": "",
            "target_url": "https://shadow.example.com",
            "semaphore_permits": 1,
            "shadow_first_timeout_ms": 5000,
            "shadow_first": false
        }"#;
        let mw = make_middleware_from_config(config_json, false);
        // Simulate one inflight request
        mw.inflight.fetch_add(1, Ordering::Relaxed);
        // With semaphore_permits=1 and 1 already inflight, next should be skipped
        assert_eq!(mw.inflight.load(Ordering::Relaxed), 1);

        // Config says max 1, so any new request should hit backpressure
        let config = mw.config_handle.as_ref().unwrap().get();
        let max = config.semaphore_permits.max(1) as usize;
        assert_eq!(max, 1);
        assert!(mw.inflight.load(Ordering::Relaxed) >= max);
    }

    #[mononoke::test]
    fn test_sample_percent_clamped_to_100() {
        let mw = make_test_middleware(true, 200, "", "");
        let config = mw.config_handle.as_ref().unwrap().get();
        assert_eq!(config.sample_ratio, 200);
        assert_eq!(config.sample_ratio.min(100), 100);
    }

    #[mononoke::test]
    fn test_shadow_header_constant() {
        assert_eq!(SHADOW_HEADER, "x-mononoke-shadow");
    }

    // --- Body capture tests ---

    #[mononoke::test]
    async fn test_inbound_captures_body_when_enabled() {
        let mw = make_test_middleware(true, 100, "", "");
        let mut state = make_state_with_body("/edenapi/trees", "POST", b"request body data");
        let result = mw.inbound(&mut state).await;
        assert!(result.is_none());
        // Body should be captured
        let captured = state.try_take::<ShadowRequestBody>();
        assert!(captured.is_some());
        assert_eq!(&captured.unwrap().0[..], b"request body data");
    }

    #[mononoke::test]
    async fn test_inbound_does_not_capture_body_when_disabled() {
        let mw = make_test_middleware(false, 0, "", "");
        let mut state = make_state_with_body("/edenapi/trees", "POST", b"request body data");
        let result = mw.inbound(&mut state).await;
        assert!(result.is_none());
        // Body should NOT be captured when disabled
        let captured = state.try_take::<ShadowRequestBody>();
        assert!(captured.is_none());
    }

    #[mononoke::test]
    async fn test_inbound_does_not_capture_body_on_shadow_tier() {
        let mw = make_test_middleware_shadow_tier(true, 100, "", "", true);
        let mut state = make_state_with_body("/edenapi/trees", "POST", b"request body data");
        let result: Option<Response<Body>> = mw.inbound(&mut state).await;
        assert!(result.is_none());
        // Shadow tier should NOT capture body
        let captured = state.try_take::<ShadowRequestBody>();
        assert!(captured.is_none());
    }

    #[mononoke::test]
    async fn test_inbound_replaces_body_for_handler() {
        let mw = make_test_middleware(true, 100, "", "");
        let mut state = make_state_with_body("/edenapi/trees", "POST", b"request body data");
        let result = mw.inbound(&mut state).await;
        assert!(result.is_none());
        // The handler should still be able to consume the body
        let body = Body::try_take_from(&mut state);
        assert!(body.is_some());
    }

    // --- Loop prevention tests ---

    #[mononoke::test]
    async fn test_outbound_skips_shadow_requests() {
        let mw = make_test_middleware(true, 10, "", "");
        let mut state = make_shadow_state("/edenapi/trees");
        let mut response = make_response();
        mw.outbound(&mut state, &mut response).await;
    }

    #[mononoke::test]
    async fn test_outbound_shadow_request_never_forwards() {
        let mw = make_test_middleware(true, 10, "", "");
        let mut response = make_response();
        for _ in 0..100 {
            let mut state = make_shadow_state("/edenapi/trees");
            mw.outbound(&mut state, &mut response).await;
        }
    }

    #[mononoke::test]
    async fn test_inbound_tracks_shadow_requests() {
        let mw = make_test_middleware(true, 10, "", "");
        let mut state = make_shadow_state("/edenapi/trees");
        let result = mw.inbound(&mut state).await;
        assert!(result.is_none());
    }

    #[mononoke::test]
    async fn test_inbound_normal_request_no_tracking() {
        let mw = make_test_middleware(true, 10, "", "");
        let mut state = make_state("/edenapi/trees");
        let result = mw.inbound(&mut state).await;
        assert!(result.is_none());
    }

    // --- Shadow tier flag tests ---

    #[mononoke::test]
    async fn test_shadow_tier_never_forwards_outbound() {
        let mw = make_test_middleware_shadow_tier(true, 100, "", "", true);
        let mut response = make_response();
        for _ in 0..100 {
            let mut state = make_state("/edenapi/trees");
            mw.outbound(&mut state, &mut response).await;
        }
    }

    #[mononoke::test]
    async fn test_shadow_tier_never_forwards_inbound() {
        let mw =
            make_middleware_from_config(&make_config_full(true, 100, "", "", true, 5000), true);
        let mut state = make_state("/edenapi/trees");
        let result = mw.inbound(&mut state).await;
        assert!(result.is_none());
    }

    #[mononoke::test]
    fn test_shadow_tier_flag_stored() {
        let mw = make_test_middleware_shadow_tier(true, 10, "", "", true);
        assert!(mw.is_shadow_tier);

        let mw = make_test_middleware_shadow_tier(true, 10, "", "", false);
        assert!(!mw.is_shadow_tier);
    }

    #[mononoke::test]
    async fn test_shadow_tier_still_tracks_received() {
        let mw = make_test_middleware_shadow_tier(true, 10, "", "", true);
        let mut state = make_shadow_state("/edenapi/trees");
        let result = mw.inbound(&mut state).await;
        assert!(result.is_none());
    }

    // --- HTTP method forwarding tests ---

    #[mononoke::test]
    async fn test_outbound_preserves_get_method() {
        let mw = make_test_middleware(true, 100, "", "");
        let mut state = make_state("/edenapi/repos");
        let method = Method::try_borrow_from(&state).cloned();
        assert_eq!(method, Some(Method::GET));
        let mut response = make_response();
        mw.outbound(&mut state, &mut response).await;
    }

    #[mononoke::test]
    async fn test_outbound_preserves_post_method() {
        let mw = make_test_middleware(true, 100, "", "");
        let mut state = make_state_with_body("/edenapi/trees", "POST", b"body");
        let method = Method::try_borrow_from(&state).cloned();
        assert_eq!(method, Some(Method::POST));
        let mut response = make_response();
        mw.inbound(&mut state).await;
        mw.outbound(&mut state, &mut response).await;
    }

    // --- Identity header forwarding tests ---

    #[mononoke::test]
    fn test_extract_forwarding_headers_present() {
        let req = Request::builder()
            .uri("/test")
            .header("x-fb-validated-client-encoded-identity", "test-identity")
            .header("tfb-orig-client-ip", "1.2.3.4")
            .header("tfb-orig-client-port", "12345")
            .header("x-client-info", "{}")
            .body(Body::default())
            .unwrap();
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let state = State::from_request(req, addr);

        let headers = ShadowForwarderMiddleware::extract_forwarding_headers(&state);
        assert_eq!(
            headers
                .get("x-fb-validated-client-encoded-identity")
                .unwrap(),
            "test-identity"
        );
        assert_eq!(headers.get("tfb-orig-client-ip").unwrap(), "1.2.3.4");
        assert_eq!(headers.get("tfb-orig-client-port").unwrap(), "12345");
        assert_eq!(headers.get("x-client-info").unwrap(), "{}");
    }

    #[mononoke::test]
    fn test_extract_forwarding_headers_missing() {
        let state = make_state("/test");
        let headers = ShadowForwarderMiddleware::extract_forwarding_headers(&state);
        assert!(headers.is_empty());
    }

    #[mononoke::test]
    fn test_extract_forwarding_headers_partial() {
        let req = Request::builder()
            .uri("/test")
            .header("x-fb-validated-client-encoded-identity", "test-identity")
            .body(Body::default())
            .unwrap();
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let state = State::from_request(req, addr);

        let headers = ShadowForwarderMiddleware::extract_forwarding_headers(&state);
        assert_eq!(headers.len(), 1);
        assert_eq!(
            headers
                .get("x-fb-validated-client-encoded-identity")
                .unwrap(),
            "test-identity"
        );
    }

    // --- Body edge case tests ---

    #[mononoke::test]
    async fn test_inbound_captures_empty_body() {
        let mw = make_test_middleware(true, 100, "", "");
        let mut state = make_state("/edenapi/repos");
        let result = mw.inbound(&mut state).await;
        assert!(result.is_none());
        let captured = state.try_take::<ShadowRequestBody>();
        assert!(captured.is_some());
        assert!(captured.unwrap().0.is_empty());
    }

    // --- Missing config tests ---

    #[mononoke::test]
    fn test_new_with_missing_config_succeeds() {
        let test_source = TestSource::new();
        // No config inserted — path doesn't exist
        let config_store = ConfigStore::new(Arc::new(test_source), None, None);
        let mw = ShadowForwarderMiddleware::new(&config_store, "nonexistent/path", false, None);
        assert!(mw.is_ok());
        assert!(mw.unwrap().config_handle.is_none());
    }

    #[mononoke::test]
    async fn test_missing_config_inbound_is_noop() {
        let test_source = TestSource::new();
        let config_store = ConfigStore::new(Arc::new(test_source), None, None);
        let mw =
            ShadowForwarderMiddleware::new(&config_store, "nonexistent/path", false, None).unwrap();
        let mut state = make_state("/edenapi/trees");
        let result = mw.inbound(&mut state).await;
        assert!(result.is_none());
        // Body should NOT be captured (no config = disabled)
        let captured = state.try_take::<ShadowRequestBody>();
        assert!(captured.is_none());
    }

    #[mononoke::test]
    async fn test_missing_config_outbound_is_noop() {
        let test_source = TestSource::new();
        let config_store = ConfigStore::new(Arc::new(test_source), None, None);
        let mw =
            ShadowForwarderMiddleware::new(&config_store, "nonexistent/path", false, None).unwrap();
        let mut state = make_state("/edenapi/trees");
        let mut response = make_response();
        // Should not panic or forward anything
        mw.outbound(&mut state, &mut response).await;
    }

    // --- Shadow-first mode tests ---

    #[mononoke::test]
    async fn test_shadow_first_disabled_no_inbound_intercept() {
        let mw = make_test_middleware(true, 100, "", "");
        let mut state = make_state("/edenapi/trees");
        let result = mw.inbound(&mut state).await;
        assert!(result.is_none());
    }

    #[mononoke::test]
    async fn test_shadow_first_skips_when_disabled() {
        let mw = make_test_middleware_shadow_first(false, 0, 5000);
        let mut state = make_state("/edenapi/trees");
        let result = mw.inbound(&mut state).await;
        assert!(result.is_none());
    }

    #[mononoke::test]
    async fn test_shadow_first_skips_shadow_requests() {
        let mw = make_test_middleware_shadow_first(true, 100, 5000);
        let mut state = make_shadow_state("/edenapi/trees");
        let result = mw.inbound(&mut state).await;
        assert!(result.is_none());
    }

    #[mononoke::test]
    async fn test_shadow_first_outbound_skips_when_shadow_first_enabled() {
        let mw = make_test_middleware_shadow_first(true, 100, 5000);
        let mut state = make_state("/edenapi/trees");
        let mut response = make_response();
        // outbound should skip because shadow_first=true means inbound handled it
        mw.outbound(&mut state, &mut response).await;
    }

    #[mononoke::test]
    async fn test_shadow_first_timeout_returns_none() {
        // Target URL is unreachable, so this will fail/timeout and return None (fallback)
        let mw = make_test_middleware_shadow_first(true, 100, 100);
        let mut state = make_state("/edenapi/trees");
        let result = mw.inbound(&mut state).await;
        // Should fall back to local (None) because shadow.example.com is unreachable
        assert!(result.is_none());
    }

    #[mononoke::test]
    async fn test_shadow_first_fallback_preserves_body_for_handler() {
        // Shadow-first with unreachable target and a POST body.
        // On fallback, the handler must still be able to consume the body.
        let mw = make_test_middleware_shadow_first(true, 100, 100);
        let mut state = make_state_with_body("/edenapi/trees", "POST", b"post body data");
        let result = mw.inbound(&mut state).await;
        // Should fall back to local (None)
        assert!(result.is_none());
        // The handler should still be able to consume the body from State
        let body = Body::try_take_from(&mut state);
        assert!(body.is_some());
    }

    #[mononoke::test]
    async fn test_shadow_first_success_forwards_headers_and_body() {
        // Start a mock HTTP server that returns a response with custom headers and body
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Spawn a simple HTTP responder
        mononoke_macros::mononoke::spawn_task(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            use tokio::io::AsyncReadExt;
            use tokio::io::AsyncWriteExt;
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let response = "HTTP/1.1 200 OK\r\n\
                Content-Type: application/json\r\n\
                X-Custom-Shadow: test-value\r\n\
                Content-Length: 15\r\n\
                \r\n\
                {\"shadow\":true}";
            let _ = stream.write_all(response.as_bytes()).await;
        });

        // Create middleware pointing to the mock server
        let config_json = format!(
            r#"{{
                "enabled": true,
                "sample_ratio": 100,
                "path_include": "",
                "path_exclude": "",
                "target_url": "http://127.0.0.1:{port}",
                "semaphore_permits": 100,
                "shadow_first_timeout_ms": 5000,
                "shadow_first": true
            }}"#
        );
        let mw = make_middleware_from_config(&config_json, false);

        let mut state = make_state("/test");
        let result = mw.inbound(&mut state).await;

        // Shadow-first should return the shadow's response
        assert!(
            result.is_some(),
            "shadow-first should return Some(response)"
        );
        let response = result.unwrap();

        // Verify status
        assert_eq!(response.status(), 200);

        // Verify response headers were forwarded
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/json"
        );
        assert_eq!(
            response.headers().get("x-custom-shadow").unwrap(),
            "test-value"
        );

        // Verify response body was forwarded
        use http_body_util::BodyExt as _;
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .map(|c| c.to_bytes())
            .unwrap_or_default();
        assert_eq!(&body_bytes[..], b"{\"shadow\":true}");
    }
}
