/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use auth::AuthSection;
use configmodel::convert::FromConfigValue;
use configmodel::ConfigExt;
use http_client::Encoding;
use http_client::HttpVersion;
use http_client::MinTransferSpeed;
use lazy_static::lazy_static;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use rand::distributions::Alphanumeric;
use rand::thread_rng;
use rand::Rng;
use url::Url;

use crate::client::Client;
use crate::errors::ConfigError;
use crate::errors::EdenApiError;
use crate::EdenApi;

lazy_static! {
    pub static ref DEFAULT_CORRELATOR: String = thread_rng()
        .sample_iter(Alphanumeric)
        .take(16)
        .map(char::from)
        .collect();
}

/// External function that constructs other kinds of `EdenApi` from config.
static CUSTOM_BUILD_FUNCS: Lazy<
    RwLock<
        Vec<
            Box<
                dyn (Fn(&dyn configmodel::Config) -> Result<Option<Arc<dyn EdenApi>>, EdenApiError>)
                    + Send
                    + Sync
                    + 'static,
            >,
        >,
    >,
> = Lazy::new(Default::default);

/// Builder for creating new EdenAPI clients.
pub struct Builder<'a> {
    config: &'a dyn configmodel::Config,
    correlator: Option<String>,
}

impl<'a> Builder<'a> {
    /// Populate a `Builder` from a Mercurial configuration.
    pub fn from_config(config: &'a dyn configmodel::Config) -> Result<Self, EdenApiError> {
        let builder = Self {
            config,
            correlator: None,
        };
        Ok(builder)
    }

    /// Unique identifier that will be logged by both the client and server for
    /// every request, allowing log entries on both sides to be correlated. Also
    /// allows correlating multiple requests that were made by the same instance
    /// of the client.
    pub fn correlator(mut self, correlator: Option<impl ToString>) -> Self {
        self.correlator = correlator.map(|s| s.to_string());
        self
    }

    /// Build the client.
    pub fn build(self) -> Result<Arc<dyn EdenApi>, EdenApiError> {
        // Consider custom build functions?
        {
            let funcs = CUSTOM_BUILD_FUNCS.read();
            for func in funcs.iter() {
                if let Some(client) = func(self.config)? {
                    return Ok(client);
                }
            }
        }

        let reponame = match self.config.get("remotefilelog", "reponame") {
            Some(name) => name.to_string(),
            None => String::new(),
        };
        if reponame.is_empty() {
            return Err(EdenApiError::BadConfig(ConfigError::Invalid(
                "remotefilelog.reponame".into(),
                anyhow!("reponame is not set"),
            )));
        }
        let client = Arc::new(
            HttpClientBuilder::from_config(self.config)?
                .correlator(self.correlator)
                .build()?,
        );
        Ok(client)
    }

    /// Register a customized builder that can produce a non-HTTP `EdenApi` from config.
    pub fn register_customize_build_func<F>(func: F)
    where
        F: (Fn(&dyn configmodel::Config) -> Result<Option<Arc<dyn EdenApi>>, EdenApiError>)
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

/// Builder for creating new HTTP EdenAPI clients.
///
/// You probably want to use [`Builder`] instead.
#[derive(Debug, Default)]
pub struct HttpClientBuilder {
    repo_name: Option<String>,
    server_url: Option<Url>,
    headers: HashMap<String, String>,
    max_files: Option<usize>,
    max_trees: Option<usize>,
    max_history: Option<usize>,
    max_location_to_hash: Option<usize>,
    max_commit_mutations: Option<usize>,
    max_commit_translate_id: Option<usize>,
    timeout: Option<Duration>,
    debug: bool,
    correlator: Option<String>,
    http_version: Option<HttpVersion>,
    log_dir: Option<PathBuf>,
    encoding: Option<Encoding>,
    min_transfer_speed: Option<MinTransferSpeed>,
    max_retry_per_request: usize,
    http_config: http_client::Config,
}

impl HttpClientBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    /// Build the HTTP client.
    pub fn build(self) -> Result<Client, EdenApiError> {
        self.try_into().map(Client::with_config)
    }

    /// Populate a `HttpClientBuilder` from a Mercurial configuration.
    pub fn from_config(config: &dyn configmodel::Config) -> Result<Self, EdenApiError> {
        // XXX: Ideally, the repo name would be a required field, obtained from a `Repo` object from
        // the `clidispatch` crate. Unforunately, not all callsites presently have access to a
        // populated `Repo` object, and it isn't trivial to just initialize one (requires a path to
        // the on-disk repo) or to plumb one through (which might not be possible for usage outside
        // of a Mercurial process, such as by EdenFS). For now, let's just allow setting the
        // reponame later via `repo_name` method.
        let mut repo_name = get_config::<String>(config, "remotefilelog", "reponame")?;
        if repo_name.as_deref() == Some("") {
            repo_name = None;
        }

        let server_url = get_required_config::<String>(config, "edenapi", "url")?
            .parse::<Url>()
            .map_err(|e| ConfigError::Invalid("edenapi.url".into(), e.into()))?;

        let auth = AuthSection::from_config(config)
            .best_match_for(&server_url)
            .unwrap_or_else(|e| {
                // Ignore errors here and make it appear as if there simply
                // wasn't a matching cert. This prevents EdenAPI from crashing
                // the program on startup if the user's certificate is missing.
                tracing::warn!("Ignoring missing client certificates: {}", &e);
                None
            });

        let mut headers = get_config::<String>(config, "edenapi", "headers")?
            .map(parse_headers)
            .transpose()
            .map_err(|e| ConfigError::Invalid("edenapi.headers".into(), e.into()))?
            .unwrap_or_default();
        headers.insert(
            "User-Agent".to_string(),
            format!("EdenSCM/{}", version::VERSION),
        );

        let max_requests = get_config(config, "edenapi", "maxrequests")?;
        let max_files = get_config(config, "edenapi", "maxfiles")?;
        let max_trees = get_config(config, "edenapi", "maxtrees")?;
        let max_history = get_config(config, "edenapi", "maxhistory")?;
        let max_location_to_hash = get_config(config, "edenapi", "maxlocationtohash")?;
        let max_commit_mutations = get_config(config, "edenapi", "maxcommitmutations")?;
        let max_commit_translate_id = get_config(config, "edenapi", "maxcommittranslateid")?;
        let timeout = get_config(config, "edenapi", "timeout")?.map(Duration::from_secs);
        let debug = get_config(config, "edenapi", "debug")?.unwrap_or_default();
        let http_version =
            get_config(config, "edenapi", "http-version")?.unwrap_or_else(|| "2".to_string());
        let http_version = Some(match http_version.as_str() {
            "1.1" => HttpVersion::V11,
            "2" => HttpVersion::V2,
            x => {
                return Err(EdenApiError::BadConfig(ConfigError::Invalid(
                    "edenapi.http-version".into(),
                    anyhow!("invalid http version {}", x),
                )));
            }
        });
        let log_dir = get_config(config, "edenapi", "logdir")?;
        let encoding =
            get_config::<String>(config, "edenapi", "encoding")?.map(|s| Encoding::from(&*s));
        let low_speed_grace_period =
            get_config::<u64>(config, "edenapi", "low-speed-grace-period-seconds")?
                .unwrap_or_default();
        let min_transfer_speed =
            get_config::<u32>(config, "edenapi", "low-speed-min-bytes-per-second")?.map(
                |min_bytes_per_second| MinTransferSpeed {
                    min_bytes_per_second,
                    grace_period: Duration::from_secs(low_speed_grace_period),
                },
            );
        let max_retry_per_request =
            get_config::<usize>(config, "edenapi", "max-retry-per-request")?.unwrap_or(3);

        let mut http_config = hg_http::http_config(config, auth);
        http_config.verbose_stats |= debug;
        http_config.max_concurrent_requests = max_requests;

        Ok(HttpClientBuilder {
            repo_name,
            server_url: Some(server_url),
            headers,
            max_files,
            max_trees,
            max_history,
            max_location_to_hash,
            max_commit_mutations,
            max_commit_translate_id,
            timeout,
            debug,
            correlator: None,
            http_version,
            log_dir,
            encoding,
            min_transfer_speed,
            max_retry_per_request,
            http_config,
        })
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

    /// Maximum number of keys per file request. Larger requests will be
    /// split up into concurrently-sent batches.
    pub fn max_files(mut self, size: Option<usize>) -> Self {
        self.max_files = size;
        self
    }

    /// Maximum number of keys per tree request. Larger requests will be
    /// split up into concurrently-sent batches.
    pub fn max_trees(mut self, size: Option<usize>) -> Self {
        self.max_trees = size;
        self
    }

    /// Maximum number of keys per history request. Larger requests will be
    /// split up into concurrently-sent batches.
    pub fn max_history(mut self, size: Option<usize>) -> Self {
        self.max_history = size;
        self
    }

    /// Maximum number of locations per location to has request. Larger requests will be split up
    /// into concurrently-sent batches.
    pub fn max_location_to_hash(mut self, size: Option<usize>) -> Self {
        self.max_location_to_hash = size;
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

    /// Unique identifier that will be logged by both the client and server for
    /// every request, allowing log entries on both sides to be correlated. Also
    /// allows correlating multiple requests that were made by the same instance
    /// of the client.
    pub fn correlator(mut self, correlator: Option<impl ToString>) -> Self {
        self.correlator = correlator.map(|s| s.to_string());
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
    Ok(get_config::<T>(config, section, name)?
        .ok_or_else(|| ConfigError::Missing(format!("{}.{}", section, name)))?)
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
    pub(crate) max_files: Option<usize>,
    pub(crate) max_trees: Option<usize>,
    pub(crate) max_history: Option<usize>,
    pub(crate) max_location_to_hash: Option<usize>,
    pub(crate) max_commit_mutations: Option<usize>,
    pub(crate) max_commit_translate_id: Option<usize>,
    pub(crate) timeout: Option<Duration>,
    #[allow(dead_code)]
    pub(crate) debug: bool,
    pub(crate) correlator: Option<String>,
    pub(crate) http_version: Option<HttpVersion>,
    pub(crate) log_dir: Option<PathBuf>,
    pub(crate) encoding: Option<Encoding>,
    pub(crate) min_transfer_speed: Option<MinTransferSpeed>,
    pub(crate) max_retry_per_request: usize,
    pub(crate) http_config: http_client::Config,
}

impl TryFrom<HttpClientBuilder> for Config {
    type Error = EdenApiError;

    fn try_from(builder: HttpClientBuilder) -> Result<Self, Self::Error> {
        let HttpClientBuilder {
            repo_name,
            server_url,
            headers,
            max_files,
            max_trees,
            max_history,
            max_location_to_hash,
            max_commit_mutations,
            max_commit_translate_id,
            timeout,
            debug,
            correlator,
            http_version,
            log_dir,
            encoding,
            min_transfer_speed,
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
        let max_files = max_files.filter(|n| *n > 0);
        let max_trees = max_trees.filter(|n| *n > 0);
        let max_history = max_history.filter(|n| *n > 0);

        Ok(Config {
            repo_name,
            server_url,
            headers,
            max_files,
            max_trees,
            max_history,
            max_location_to_hash,
            max_commit_mutations,
            max_commit_translate_id,
            timeout,
            debug,
            correlator,
            http_version,
            log_dir,
            encoding,
            min_transfer_speed,
            max_retry_per_request,
            http_config,
        })
    }
}

/// Parse headers from a JSON object.
fn parse_headers(headers: impl AsRef<str>) -> Result<HashMap<String, String>, Error> {
    Ok(serde_json::from_str(headers.as_ref())
        .context(format!("Not a valid JSON object: {:?}", headers.as_ref()))?)
}
