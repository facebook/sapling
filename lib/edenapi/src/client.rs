// Copyright Facebook, Inc. 2019

use std::{path::PathBuf, sync::Arc};

use failure::Fallible;
use hyper::{client::HttpConnector, Body, Client};
use hyper_rustls::HttpsConnector;
use url::Url;

use crate::builder::Builder;

pub(crate) type HyperClient = Client<HttpsConnector<HttpConnector>, Body>;

/// An HTTP client for the Eden API.
///
/// # Example
/// ```rust,ignore
/// use failure::Fallible;
/// use edenapi::{EdenApi, EdenApiHttpClient};
///
/// fn main() -> Fallible<()> {
///     let cert = "/var/facebook/credentials/user/x509/user.pem";
///     let key = "/var/facebook/credentials/user/x509/user.pem";
///     let client = EdenApiHttpClient::builder()
///         .base_url_str("https://mononoke-api.internal.tfbnw.net")?
///         .repo("fbsource")
///         .cache_path("/var/cache/hgcache")
///         .client_creds(cert, key)?
///         .build()?;
///
///     client.health_check()
/// }
/// ```
pub struct EdenApiHttpClient {
    pub(crate) client: Arc<HyperClient>,
    pub(crate) base_url: Url,
    pub(crate) repo: String,
    pub(crate) cache_path: PathBuf,
}

impl EdenApiHttpClient {
    pub fn builder() -> Builder {
        Builder::new()
    }

    /// Return the base URL of the API server joined with the repo name
    /// and a trailing slash to allow additional URL components to be
    /// appended on.
    pub(crate) fn repo_base_url(&self) -> Fallible<Url> {
        Ok(self.base_url.join(&format!("{}/", &self.repo))?)
    }

    /// Get the path for the directory where packfiles should be written
    /// for the configured repo.
    pub(crate) fn pack_cache_path(&self) -> PathBuf {
        self.cache_path.join(&self.repo).join("packs")
    }
}
