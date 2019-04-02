// Copyright Facebook, Inc. 2019

use std::path::{Path, PathBuf};

use failure::Fallible;
use url::Url;

#[derive(Default)]
pub struct Config {
    pub(crate) base_url: Option<Url>,
    pub(crate) creds: Option<ClientCreds>,
    pub(crate) repo: Option<String>,
    pub(crate) cache_path: Option<PathBuf>,
}

impl Config {
    pub fn new() -> Self {
        Default::default()
    }

    /// Base URL of the Mononoke API server host.
    pub fn base_url(mut self, url: Url) -> Self {
        self.base_url = Some(url);
        self
    }

    /// Parse an arbitrary string as the base URL.
    /// Fails if the string is not a valid URL.
    pub fn base_url_str(self, url: &str) -> Fallible<Self> {
        let url = Url::parse(url)?;
        Ok(self.base_url(url))
    }

    /// Set client credentials by providing paths to a PEM encoded X.509 client certificate
    /// and a PEM encoded private key. These credentials are used for TLS mutual authentication;
    /// if not set, mutual authentication will not be used.
    pub fn client_creds(mut self, cert: impl AsRef<Path>, key: impl AsRef<Path>) -> Self {
        self.creds = Some(ClientCreds::new(cert, key));
        self
    }

    /// Set the name of the current repo.
    /// Should correspond to the remotefilelog.reponame config item.
    pub fn repo(mut self, repo: impl ToString) -> Self {
        self.repo = Some(repo.to_string());
        self
    }

    /// Set the path of the cache directory where packfiles are stored.
    /// Should correspond to the remotefilelog.cachepath config item.
    pub fn cache_path(mut self, path: impl AsRef<Path>) -> Self {
        self.cache_path = Some(path.as_ref().to_path_buf());
        self
    }
}

/// Client credentials for TLS mutual authentication, including an X.509 client
/// certificate chain and an RSA or ECDSA private key.
pub struct ClientCreds {
    pub(crate) certs: PathBuf,
    pub(crate) key: PathBuf,
}

impl ClientCreds {
    pub fn new(certs: impl AsRef<Path>, key: impl AsRef<Path>) -> Self {
        Self {
            certs: certs.as_ref().to_path_buf(),
            key: key.as_ref().to_path_buf(),
        }
    }
}
