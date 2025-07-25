/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Error;
use anyhow::anyhow;
use cats::CatsSection;
use configmodel::ConfigExt;
use configmodel::convert::FromConfigValue;
use http_client::Encoding;
use http_client::HttpVersion;
use http_client::MinTransferSpeed;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use url::Url;

use crate::SaplingRemoteApi;
use crate::client::Client;
use crate::errors::ConfigError;
use crate::errors::SaplingRemoteApiError;

/// External function that constructs other kinds of `SaplingRemoteApi` from config.
static CUSTOM_BUILD_FUNCS: Lazy<
    RwLock<
        Vec<
            Box<
                dyn (Fn(
                        &dyn configmodel::Config,
                    )
                        -> Result<Option<Arc<dyn SaplingRemoteApi>>, SaplingRemoteApiError>)
                    + Send
                    + Sync
                    + 'static,
            >,
        >,
    >,
> = Lazy::new(Default::default);

/// Builder for creating new SaplingRemoteAPI clients.
pub struct Builder<'a> {
    config: &'a dyn configmodel::Config,
    repo_name: Option<String>,
    server_url: Option<Url>,
}

impl<'a> Builder<'a> {
    /// Populate a `Builder` from a Mercurial configuration.
    pub fn from_config(config: &'a dyn configmodel::Config) -> Result<Self, SaplingRemoteApiError> {
        let builder = Self {
            config,
            repo_name: None,
            server_url: None,
        };
        Ok(builder)
    }

    /// Configure repo name for client. This is only used by the Http Client.
    pub fn repo_name(mut self, repo_name: Option<impl ToString>) -> Self {
        self.repo_name = repo_name.map(|s| s.to_string());
        self
    }

    /// Configure server URL for client. This is only used by the Http Client.
    pub fn server_url(mut self, url: Option<Url>) -> Self {
        self.server_url = url;
        self
    }

    /// Build the client.
    pub fn build(self) -> Result<Arc<dyn SaplingRemoteApi>, SaplingRemoteApiError> {
        {
            // Hook in other SaplingRemoteAPI implementations such as eagerepo (used for tests).
            let funcs = CUSTOM_BUILD_FUNCS.read();
            for func in funcs.iter() {
                if let Some(client) = func(self.config)? {
                    return Ok(client);
                }
            }
        }
        let mut builder = if let Some(server_url) = self.server_url {
            HttpClientBuilder::from_config_with_url(self.config, server_url)?
        } else {
            HttpClientBuilder::from_config(self.config)?
        };

        if let Some(repo_name) = &self.repo_name {
            builder = builder.repo_name(repo_name);
        }

        Ok(Arc::new(builder.build()?))
    }

    /// Register a customized builder that can produce a non-HTTP `SaplingRemoteApi` from config.
    pub fn register_customize_build_func<F>(func: F)
    where
        F: (Fn(
                &dyn configmodel::Config,
            ) -> Result<Option<Arc<dyn SaplingRemoteApi>>, SaplingRemoteApiError>)
            + Send
            + Sync
            + 'static,
        F: Copy,
    {
        tracing::debug!(
            "registered {} to edenapi Builder",
            std::any::type_name::<F>()
        );
        CUSTOM_BUILD_FUNCS.write().push(Box::new(func))
    }
}

/// Builder for creating new HTTP SaplingRemoteAPI clients.
///
/// You probably want to use [`Builder`] instead.
#[derive(Debug, Default)]
pub struct HttpClientBuilder {
    repo_name: Option<String>,
    server_url: Option<Url>,
    headers: HashMap<String, String>,
    try_route_consistently: bool,
    augmented_trees: bool,
    max_commit_data_per_batch: Option<usize>,
    max_files_per_batch: Option<usize>,
    max_trees_per_batch: Option<usize>,
    max_history_per_batch: Option<usize>,
    max_path_history_per_batch: Option<usize>,
    max_location_to_hash_per_batch: Option<usize>,
    max_commit_mutations_per_batch: Option<usize>,
    max_commit_translate_id_per_batch: Option<usize>,
    min_batch_size: Option<usize>,
    timeout: Option<Duration>,
    connect_timeout: Option<Duration>,
    handler_timeouts: HashMap<String, Duration>,
    debug: bool,
    http_version: Option<HttpVersion>,
    log_dir: Option<PathBuf>,
    encoding: Option<Encoding>,
    min_transfer_speed: Option<MinTransferSpeed>,
    handler_min_transfer_speeds: HashMap<String, MinTransferSpeed>,
    max_retry_per_request: usize,
    http_config: http_client::Config,
}

impl HttpClientBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    /// Build the HTTP client.
    pub fn build(self) -> Result<Client, SaplingRemoteApiError> {
        self.try_into().map(Client::with_config)
    }

    pub fn from_config_with_url(
        config: &dyn configmodel::Config,
        server_url: Url,
    ) -> Result<Self, SaplingRemoteApiError> {
        // XXX: Ideally, the repo name would be a required field, obtained from a `Repo` object from
        // the `clidispatch` crate. Unfortunately, not all callsites presently have access to a
        // populated `Repo` object, and it isn't trivial to just initialize one (requires a path to
        // the on-disk repo) or to plumb one through (which might not be possible for usage outside
        // of a Mercurial process, such as by EdenFS). For now, let's just allow setting the
        // reponame later via `repo_name` method.
        let mut repo_name = get_config::<String>(config, "remotefilelog", "reponame")?;
        if repo_name.as_deref() == Some("") {
            repo_name = None;
        }

        let mut headers = get_config::<String>(config, "edenapi", "headers")?
            .map(parse_headers)
            .transpose()
            .map_err(|e| ConfigError::Invalid("edenapi.headers".into(), e))?
            .unwrap_or_default();

        let source = if std::env::current_exe()
            .ok()
            .and_then(|path| {
                path.file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s.contains("edenfs"))
            })
            .unwrap_or_default()
        {
            "EdenFs"
        } else {
            "Sapling"
        };

        headers.insert(
            "User-Agent".to_string(),
            format!("{}/{}", source, version::VERSION),
        );

        let cats = CatsSection::from_config(&config, "cats").get_cats();
        if let Ok(Some(cats)) = cats {
            headers.insert("x-forwarded-cats".to_string(), cats.clone());
        }

        // edenapi.maxrequests is old name supported for transition to new name - can delete in future
        let max_requests = get_config(config, "edenapi", "max-concurrent-requests")?
            .or(get_config(config, "edenapi", "maxrequests")?);

        let max_requests_per_batch =
            get_config(config, "edenapi", "max-concurrent-requests-per-batch")?;

        let try_route_consistently =
            get_config(config, "edenapi", "try-route-consistently")?.unwrap_or_default();

        let augmented_trees = get_config(config, "edenapi", "augmented-trees")?.unwrap_or_default();

        let min_batch_size = get_config(config, "edenapi", "min-batch-size")?;
        let max_commit_data_per_batch = get_config(config, "edenapi", "maxcommitdata")?;
        let max_files_per_batch = get_config(config, "edenapi", "maxfiles")?;
        let max_trees_per_batch = get_config(config, "edenapi", "maxtrees")?;
        let max_history_per_batch = get_config(config, "edenapi", "maxhistory")?;
        let max_path_history_per_batch = get_config(config, "edenapi", "maxpathhistory")?;
        let max_location_to_hash_per_batch = get_config(config, "edenapi", "maxlocationtohash")?;
        let max_commit_mutations_per_batch = get_config(config, "edenapi", "maxcommitmutations")?;
        let max_commit_translate_id_per_batch =
            get_config(config, "edenapi", "maxcommittranslateid")?;

        let timeout: Option<Duration> = get_config(config, "edenapi", "timeout")?;
        let connect_timeout: Option<Duration> = get_config(config, "edenapi", "connect-timeout")?;

        let handler_timeouts: HashMap<String, Duration> = config
            .keys_prefixed("edenapi", "timeout.")
            .into_iter()
            .filter_map(
                |key| match get_config::<Duration>(config, "edenapi", &key) {
                    Err(err) => Some(Err(err)),
                    Ok(Some(value)) => Some(Ok((key.strip_prefix("timeout.")?.to_string(), value))),
                    Ok(None) => None,
                },
            )
            .collect::<Result<_, _>>()?;

        let debug = get_config(config, "edenapi", "debug")?.unwrap_or_default();
        let http_version =
            get_config(config, "edenapi", "http-version")?.unwrap_or_else(|| "2".to_string());
        let http_version = Some(match http_version.as_str() {
            "1.1" => HttpVersion::V11,
            "2" => HttpVersion::V2,
            x => {
                return Err(SaplingRemoteApiError::BadConfig(ConfigError::Invalid(
                    "edenapi.http-version".into(),
                    anyhow!("invalid http version {}", x),
                )));
            }
        });
        let log_dir = get_config(config, "edenapi", "logdir")?;
        let encoding =
            get_config::<String>(config, "edenapi", "encoding")?.map(|s| Encoding::from(&*s));

        let low_speed_window: Duration = match get_config(config, "edenapi", "low-speed-window")? {
            Some(window) => window,
            None => {
                get_config(config, "edenapi", "low-speed-grace-period-seconds")?.unwrap_or_default()
            }
        };
        let min_transfer_speed =
            get_config::<u32>(config, "edenapi", "low-speed-min-bytes-per-second")?.map(
                |min_bytes_per_second| MinTransferSpeed {
                    min_bytes_per_second,
                    window: low_speed_window,
                },
            );

        let handler_min_transfer_speeds: HashMap<String, MinTransferSpeed> = config
            .keys_prefixed("edenapi", "low-speed-min-bytes-per-second.")
            .into_iter()
            .filter_map(|key| {
                let handler = key.strip_prefix("low-speed-min-bytes-per-second.")?;
                match get_config(config, "edenapi", &key) {
                    Err(err) => Some(Err(err)),
                    Ok(Some(value)) => Some(Ok((
                        handler.to_string(),
                        MinTransferSpeed {
                            min_bytes_per_second: value,
                            window: match get_config(
                                config,
                                "edenapi",
                                &format!("low-speed-window.{handler}"),
                            ) {
                                Err(err) => return Some(Err(err)),
                                Ok(Some(window)) => window,
                                Ok(None) => Duration::default(),
                            },
                        },
                    ))),
                    Ok(None) => None,
                }
            })
            .collect::<Result<_, _>>()?;

        let max_retry_per_request =
            get_config::<usize>(config, "edenapi", "max-retry-per-request")?.unwrap_or(3);

        let max_concurrent_streams =
            get_config::<usize>(config, "edenapi", "max-concurrent-streams")?;

        let mut http_config = hg_http::http_config(config, &server_url)?;
        http_config.verbose_stats |= debug;
        http_config.max_concurrent_requests = max_requests;
        http_config.max_concurrent_requests_per_batch = max_requests_per_batch;
        http_config.max_concurrent_streams = max_concurrent_streams;

        let builder = HttpClientBuilder {
            repo_name,
            server_url: Some(server_url),
            headers,
            try_route_consistently,
            augmented_trees,
            max_commit_data_per_batch,
            max_files_per_batch,
            max_trees_per_batch,
            max_history_per_batch,
            max_path_history_per_batch,
            max_location_to_hash_per_batch,
            max_commit_mutations_per_batch,
            max_commit_translate_id_per_batch,
            min_batch_size,
            timeout,
            connect_timeout,
            handler_timeouts,
            debug,
            http_version,
            log_dir,
            encoding,
            min_transfer_speed,
            handler_min_transfer_speeds,
            max_retry_per_request,
            http_config,
        };

        tracing::debug!(?builder);
        Ok(builder)
    }

    /// Populate a `HttpClientBuilder` from a Mercurial configuration.
    pub fn from_config(config: &dyn configmodel::Config) -> Result<Self, SaplingRemoteApiError> {
        let server_url = get_required_config::<String>(config, "edenapi", "url")?
            .parse::<Url>()
            .map_err(|e| ConfigError::Invalid("edenapi.url".into(), e.into()))?;
        Self::from_config_with_url(config, server_url)
    }

    /// Set the repo name.
    pub fn repo_name(mut self, repo_name: &str) -> Self {
        self.repo_name = Some(repo_name.into());
        self
    }

    /// Set the server URL.
    pub fn server_url(mut self, url: Url) -> Self {
        self.server_url = Some(url);
        self
    }

    /// Extra HTTP headers that should be sent with each request.
    pub fn headers<T, K, V>(mut self, headers: T) -> Self
    where
        T: IntoIterator<Item = (K, V)>,
        K: ToString,
        V: ToString,
    {
        let headers = headers
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()));
        self.headers.extend(headers);
        self
    }

    /// Add an extra HTTP header that should be sent with each request.
    pub fn header(mut self, name: impl ToString, value: impl ToString) -> Self {
        self.headers.insert(name.to_string(), value.to_string());
        self
    }

    /// Maximum number of concurrent HTTP requests allowed.
    pub fn max_requests(mut self, size: Option<usize>) -> Self {
        self.http_config.max_concurrent_requests = size;
        self
    }

    /// Maximum number of keys per commit data request. Larger requests will be
    /// split up into concurrently-sent batches.
    pub fn max_commit_data_per_batch(mut self, size: Option<usize>) -> Self {
        self.max_commit_data_per_batch = size;
        self
    }

    /// Maximum number of keys per file request. Larger requests will be
    /// split up into concurrently-sent batches.
    pub fn max_files_per_batch(mut self, size: Option<usize>) -> Self {
        self.max_files_per_batch = size;
        self
    }

    /// Maximum number of keys per tree request. Larger requests will be
    /// split up into concurrently-sent batches.
    pub fn max_trees_per_batch(mut self, size: Option<usize>) -> Self {
        self.max_trees_per_batch = size;
        self
    }

    /// Maximum number of keys per history request. Larger requests will be
    /// split up into concurrently-sent batches.
    pub fn max_history_per_batch(mut self, size: Option<usize>) -> Self {
        self.max_history_per_batch = size;
        self
    }

    /// Maximum number of paths per path_history request. Larger requests will be
    /// split up into concurrently-sent batches.
    pub fn max_path_history_per_batch(mut self, size: Option<usize>) -> Self {
        self.max_path_history_per_batch = size;
        self
    }

    /// Maximum number of locations per location to has request. Larger requests will be split up
    /// into concurrently-sent batches.
    pub fn max_location_to_hash_per_batch(mut self, size: Option<usize>) -> Self {
        self.max_location_to_hash_per_batch = size;
        self
    }

    /// Maximum number of retries per request.
    pub fn max_retry_per_request(mut self, max: usize) -> Self {
        self.max_retry_per_request = max;
        self
    }

    /// Timeout for HTTP requests sent by the client.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set the HTTP version that the client should use.
    pub fn http_version(mut self, version: HttpVersion) -> Self {
        self.http_version = Some(version);
        self
    }

    /// If specified, the client will write a JSON version of every request
    /// it sends to the specified directory. This is primarily useful for
    /// debugging.
    pub fn log_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.log_dir = Some(dir.as_ref().into());
        self
    }

    /// If enabled, convert the user's client certificate from PEM to PKCS#12
    /// prior to use. This is required on platforms that do not natively support
    /// PEM certificates, such as Windows.
    pub fn convert_cert(mut self, enable: bool) -> Self {
        self.http_config.convert_cert = enable;
        self
    }

    /// Specify settings for the underlying HTTP client for non-Sapling
    /// edenapi clients.
    pub fn http_config(mut self, http_config: http_client::Config) -> Self {
        self.http_config = http_config;
        self
    }
}

fn get_config<T: FromConfigValue>(
    config: &dyn configmodel::Config,
    section: &str,
    name: &str,
) -> Result<Option<T>, ConfigError> {
    config
        .get_opt::<T>(section, name)
        .map_err(|e| ConfigError::Invalid(format!("{}.{}", section, name), e.into()))
}

fn get_required_config<T: FromConfigValue>(
    config: &dyn configmodel::Config,
    section: &str,
    name: &str,
) -> Result<T, ConfigError> {
    get_config::<T>(config, section, name)?
        .ok_or_else(|| ConfigError::Missing(format!("{}.{}", section, name)))
}

/// Configuration for a `Client`. Essentially has the same fields as a
/// `HttpClientBuilder`, but required fields are not optional and values have
/// been appropriately parsed and validated.
#[derive(Debug)]
pub(crate) struct Config {
    #[allow(dead_code)]
    pub(crate) repo_name: String,
    pub(crate) server_url: Url,
    pub(crate) headers: HashMap<String, String>,
    pub(crate) try_route_consistently: bool,
    pub(crate) augmented_trees: bool,
    pub(crate) max_commit_data_per_batch: Option<usize>,
    pub(crate) max_files_per_batch: Option<usize>,
    pub(crate) max_trees_per_batch: Option<usize>,
    pub(crate) max_history_per_batch: Option<usize>,
    pub(crate) max_path_history_per_batch: Option<usize>,
    pub(crate) max_location_to_hash_per_batch: Option<usize>,
    pub(crate) max_commit_mutations_per_batch: Option<usize>,
    pub(crate) max_commit_translate_id_per_batch: Option<usize>,
    pub(crate) min_batch_size: Option<usize>,
    pub(crate) connect_timeout: Option<Duration>,
    pub(crate) timeout: Option<Duration>,
    pub(crate) handler_timeouts: HashMap<String, Duration>,
    #[allow(dead_code)]
    pub(crate) debug: bool,
    pub(crate) http_version: Option<HttpVersion>,
    pub(crate) log_dir: Option<PathBuf>,
    pub(crate) encoding: Option<Encoding>,
    pub(crate) min_transfer_speed: Option<MinTransferSpeed>,
    pub(crate) handler_min_transfer_speeds: HashMap<String, MinTransferSpeed>,
    pub(crate) max_retry_per_request: usize,
    pub(crate) http_config: http_client::Config,
}

impl TryFrom<HttpClientBuilder> for Config {
    type Error = SaplingRemoteApiError;

    fn try_from(builder: HttpClientBuilder) -> Result<Self, Self::Error> {
        let HttpClientBuilder {
            repo_name,
            server_url,
            headers,
            try_route_consistently,
            augmented_trees,
            max_commit_data_per_batch,
            max_files_per_batch,
            max_trees_per_batch,
            max_history_per_batch,
            max_path_history_per_batch,
            max_location_to_hash_per_batch,
            max_commit_mutations_per_batch,
            max_commit_translate_id_per_batch,
            min_batch_size,
            connect_timeout,
            timeout,
            handler_timeouts,
            debug,
            http_version,
            log_dir,
            encoding,
            min_transfer_speed,
            handler_min_transfer_speeds,
            max_retry_per_request,
            http_config,
        } = builder;

        // Check for missing required fields.
        let repo_name = repo_name.ok_or(ConfigError::Missing("remotefilelog.reponame".into()))?;
        let mut server_url = server_url.ok_or(ConfigError::Missing("edenapi.url".into()))?;

        // Ensure the base URL's path ends with a slash so that `Url::join`
        // won't strip the final path component.
        if !server_url.path().ends_with('/') {
            let path = format!("{}/", server_url.path());
            server_url.set_path(&path);
        }

        // Setting these to 0 is the same as None.
        let max_commit_data_per_batch = max_commit_data_per_batch.filter(|n| *n > 0);
        let max_files_per_batch = max_files_per_batch.filter(|n| *n > 0);
        let max_trees_per_batch = max_trees_per_batch.filter(|n| *n > 0);
        let max_history_per_batch = max_history_per_batch.filter(|n| *n > 0);
        let max_path_history_per_batch = max_path_history_per_batch.filter(|n| *n > 0);

        Ok(Config {
            repo_name,
            server_url,
            headers,
            try_route_consistently,
            augmented_trees,
            max_commit_data_per_batch,
            max_files_per_batch,
            max_trees_per_batch,
            max_history_per_batch,
            max_path_history_per_batch,
            max_location_to_hash_per_batch,
            max_commit_mutations_per_batch,
            max_commit_translate_id_per_batch,
            min_batch_size,
            connect_timeout,
            timeout,
            handler_timeouts,
            debug,
            http_version,
            log_dir,
            encoding,
            min_transfer_speed,
            handler_min_transfer_speeds,
            max_retry_per_request,
            http_config,
        })
    }
}

/// Parse headers from a JSON object.
fn parse_headers(headers: impl AsRef<str>) -> Result<HashMap<String, String>, Error> {
    serde_json::from_str(headers.as_ref())
        .context(format!("Not a valid JSON object: {:?}", headers.as_ref()))
}
