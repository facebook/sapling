// Copyright Facebook, Inc. 2019

use std::{path::PathBuf, sync::Arc};

use failure::Fallible;
use hyper::client::HttpConnector;
use hyper::{Body, Client};
use hyper_tls::HttpsConnector;
use url::Url;

pub(crate) type HyperClient = Client<HttpsConnector<HttpConnector>, Body>;

/// An HTTP client for the Eden API.
///
/// # Example
/// ```no_run
/// use failure::Fallible;
/// use edenapi::{ClientBuilder, EdenApi, EdenApiHttpClient};
///
/// fn main() -> Fallible<()> {
///     let client = ClientBuilder::new()
///         .base_url_str("https://mononoke-api.internal.tfbnw.net")?
///         .client_creds("/var/facebook/credentials/user/x509/user.pem")?
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
