/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Error};
use url::Url;

use anyhow::anyhow;
use auth::AuthSection;
use configparser::config::ConfigSet;
use http_client::HttpVersion;

use crate::client::Client;
use crate::errors::{ConfigError, EdenApiError};

/// Builder for creating new EdenAPI clients.
#[derive(Debug, Default)]
pub struct Builder {
    server_url: Option<Url>,
    cert: Option<PathBuf>,
    key: Option<PathBuf>,
    ca_bundle: Option<PathBuf>,
    headers: HashMap<String, String>,
    max_files: Option<usize>,
    max_trees: Option<usize>,
    max_history: Option<usize>,
    max_location_to_hash: Option<usize>,
    timeout: Option<Duration>,
    debug: bool,
    correlator: Option<String>,
    http_version: Option<HttpVersion>,
    validate_certs: bool,
    log_dir: Option<PathBuf>,
}

impl Builder {
    pub fn new() -> Self {
        Default::default()
    }

    /// Build the client.
    pub fn build(self) -> Result<Client, EdenApiError> {
        self.try_into().map(Client::with_config)
    }

    /// Populate a `Builder` from a Mercurial configuration.
    pub fn from_config(config: &ConfigSet) -> Result<Self, EdenApiError> {
        let server_url = config
            .get_opt::<String>("edenapi", "url")
            .map_err(|e| ConfigError::Malformed("edenapi.url".into(), e))?
            .ok_or(ConfigError::MissingUrl)?
            .parse::<Url>()
            .map_err(ConfigError::InvalidUrl)?;

        let validate_certs = config
            .get_opt::<bool>("edenapi", "validate-certs")
            .map_err(|e| ConfigError::Malformed("edenapi.validate-certs".into(), e))?
            .unwrap_or_default();

        let (cert, key, ca_bundle) = AuthSection::from_config(&config)
            .validate(validate_certs)
            .best_match_for(&server_url)?
            .map(|auth| (auth.cert, auth.key, auth.cacerts))
            .unwrap_or_default();

        let headers = config
            .get_opt::<String>("edenapi", "headers")
            .map_err(|e| ConfigError::Malformed("edenapi.headers".into(), e))?
            .map(parse_headers)
            .transpose()
            .map_err(|e| ConfigError::Malformed("edenapi.headers".into(), e))?
            .unwrap_or_default();

        let max_files = config
            .get_opt("edenapi", "maxfiles")
            .map_err(|e| ConfigError::Malformed("edenapi.maxfiles".into(), e))?;

        let max_trees = config
            .get_opt("edenapi", "maxtrees")
            .map_err(|e| ConfigError::Malformed("edenapi.maxtrees".into(), e))?;

        let max_history = config
            .get_opt("edenapi", "maxhistory")
            .map_err(|e| ConfigError::Malformed("edenapi.maxhistory".into(), e))?;

        let max_location_to_hash = config
            .get_opt("edenapi", "maxlocationtohash")
            .map_err(|e| ConfigError::Malformed("edenapi.maxlocationtohash".into(), e))?;

        let timeout = config
            .get_opt("edenapi", "timeout")
            .map_err(|e| ConfigError::Malformed("edenapi.timeout".into(), e))?
            .map(Duration::from_secs);

        let debug = config
            .get_opt("edenapi", "debug")
            .map_err(|e| ConfigError::Malformed("edenapi.timeout".into(), e))?
            .unwrap_or_default();

        let http_version = config
            .get_opt("edenapi", "http-version")
            .map_err(|e| ConfigError::Malformed("edenapi.http-version".into(), e))?
            .unwrap_or_else(|| "2".to_string());
        let http_version = Some(match http_version.as_str() {
            "1.1" => HttpVersion::V11,
            "2" => HttpVersion::V2,
            x => {
                return Err(EdenApiError::BadConfig(ConfigError::Malformed(
                    "edenapi.http-version".into(),
                    anyhow!("invalid http version {}", x),
                )));
            }
        });

        let log_dir = config
            .get_opt::<PathBuf>("edenapi", "logdir")
            .map_err(|e| ConfigError::Malformed("edenapi.logdir".into(), e))?;
        Ok(Self {
            server_url: Some(server_url),
            cert,
            key,
            ca_bundle,
            headers,
            max_files,
            max_trees,
            max_history,
            max_location_to_hash,
            timeout,
            debug,
            correlator: None,
            http_version,
            validate_certs,
            log_dir,
        })
    }

    /// Set the server URL.
    pub fn server_url(mut self, url: Url) -> Self {
        self.server_url = Some(url);
        self
    }

    /// Specify a client certificate for authenticating with the server.
    /// The caller should provide a path to PEM-encoded X.509 certificate file.
    /// The corresponding private key may either be provided in the same file
    /// as the certificate, or separately using the `key` method.
    pub fn cert(mut self, cert: impl AsRef<Path>) -> Self {
        self.cert = Some(cert.as_ref().into());
        self
    }

    /// Specify the client's private key
    pub fn key(mut self, key: impl AsRef<Path>) -> Self {
        self.key = Some(key.as_ref().into());
        self
    }

    /// Specify a CA certificate bundle to be used to validate the server's
    /// TLS certificate in place of the default system certificate bundle.
    /// Primarily used in tests.
    pub fn ca_bundle(mut self, ca: impl AsRef<Path>) -> Self {
        self.ca_bundle = Some(ca.as_ref().into());
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

    /// Specify whether the client should validate the user's client certificate
    /// before each request.
    pub fn validate_certs(mut self, validate_certs: bool) -> Self {
        self.validate_certs = validate_certs;
        self
    }

    /// If specified, the client will write a JSON version of every request
    /// it sends to the specified directory. This is primarily useful for
    /// debugging. The JSON requests can be sent with the `edenapi_cli`, or
    /// converted to CBOR with the `make_req` tool and sent with `curl`.
    pub fn log_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.log_dir = Some(dir.as_ref().into());
        self
    }
}

/// Configuration for a `Client`. Essentially has the same fields as a
/// `Builder`, but required fields are not optional and values have been
/// appropriately parsed and validated.
#[derive(Debug)]
pub(crate) struct Config {
    pub(crate) server_url: Url,
    pub(crate) cert: Option<PathBuf>,
    pub(crate) key: Option<PathBuf>,
    pub(crate) ca_bundle: Option<PathBuf>,
    pub(crate) headers: HashMap<String, String>,
    pub(crate) max_files: Option<usize>,
    pub(crate) max_trees: Option<usize>,
    pub(crate) max_history: Option<usize>,
    pub(crate) max_location_to_hash: Option<usize>,
    pub(crate) timeout: Option<Duration>,
    pub(crate) debug: bool,
    pub(crate) correlator: Option<String>,
    pub(crate) http_version: Option<HttpVersion>,
    pub(crate) validate_certs: bool,
    pub(crate) log_dir: Option<PathBuf>,
}

impl TryFrom<Builder> for Config {
    type Error = EdenApiError;

    fn try_from(builder: Builder) -> Result<Self, Self::Error> {
        let Builder {
            server_url,
            cert,
            key,
            ca_bundle,
            headers,
            max_files,
            max_trees,
            max_history,
            max_location_to_hash,
            timeout,
            debug,
            correlator,
            http_version,
            validate_certs,
            log_dir,
        } = builder;

        // Check for missing required fields.
        let mut server_url = server_url.ok_or(ConfigError::MissingUrl)?;

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
            server_url,
            cert,
            key,
            ca_bundle,
            headers,
            max_files,
            max_trees,
            max_history,
            max_location_to_hash,
            timeout,
            debug,
            correlator,
            http_version,
            validate_certs,
            log_dir,
        })
    }
}

/// Parse headers from a JSON object.
fn parse_headers(headers: impl AsRef<str>) -> Result<HashMap<String, String>, Error> {
    Ok(serde_json::from_str(headers.as_ref())
        .context(format!("Not a valid JSON object: {:?}", headers.as_ref()))?)
}
