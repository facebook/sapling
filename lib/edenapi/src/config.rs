// Copyright Facebook, Inc. 2019

use std::path::{Path, PathBuf};

use failure::ensure;
use url::Url;

use crate::errors::ApiResult;

#[derive(Default)]
pub struct Config {
    pub(crate) base_url: Option<Url>,
    pub(crate) creds: Option<ClientCreds>,
    pub(crate) repo: Option<String>,
    pub(crate) data_batch_size: Option<usize>,
    pub(crate) history_batch_size: Option<usize>,
    pub(crate) validate: bool,
    pub(crate) stream_data: bool,
    pub(crate) stream_history: bool,
    pub(crate) stream_trees: bool,
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
    pub fn base_url_str(self, url: &str) -> ApiResult<Self> {
        let url = Url::parse(url)?;
        Ok(self.base_url(url))
    }

    /// Set client credentials by providing paths to a PEM encoded X.509 client certificate
    /// and a PEM encoded private key. These credentials are used for TLS mutual authentication;
    /// if not set, mutual authentication will not be used.
    pub fn client_creds(
        mut self,
        cert: impl AsRef<Path>,
        key: impl AsRef<Path>,
    ) -> ApiResult<Self> {
        self.creds = Some(ClientCreds::new(cert, key)?);
        Ok(self)
    }

    /// Set the name of the current repo.
    /// Should correspond to the remotefilelog.reponame config item.
    pub fn repo(mut self, repo: impl ToString) -> Self {
        self.repo = Some(repo.to_string());
        self
    }

    /// Number of keys that should be fetched per file data request.
    /// Setting this to `None` disables batching.
    pub fn data_batch_size(mut self, size: Option<usize>) -> Self {
        self.data_batch_size = size;
        self
    }

    /// Number of keys that should be fetched per file history request.
    /// Setting this to `None` disables batching.
    pub fn history_batch_size(mut self, size: Option<usize>) -> Self {
        self.history_batch_size = size;
        self
    }

    /// Specifies whether the client should attempt to validate the
    /// received data by recomputing and comparing the filenode hash.
    pub fn validate(mut self, validate: bool) -> Self {
        self.validate = validate;
        self
    }

    /// Specifies whether the client should request streaming responses
    /// for fetchihng data (such as file content).
    pub fn stream_data(mut self, stream_data: bool) -> Self {
        self.stream_data = stream_data;
        self
    }

    /// Specifies whether the client should request streaming responses
    /// for fetching history entries.
    pub fn stream_history(mut self, stream_history: bool) -> Self {
        self.stream_history = stream_history;
        self
    }

    /// Specifies whether the client should request streaming responses
    /// for prefetching trees.
    pub fn stream_trees(mut self, stream_trees: bool) -> Self {
        self.stream_trees = stream_trees;
        self
    }
}

/// Client credentials for TLS mutual authentication, including an X.509 client
/// certificate chain and an RSA or ECDSA private key.
#[derive(Clone, Debug)]
pub struct ClientCreds {
    pub(crate) certs: PathBuf,
    pub(crate) key: PathBuf,
}

impl ClientCreds {
    pub fn new(certs: impl AsRef<Path>, key: impl AsRef<Path>) -> ApiResult<Self> {
        let certs = certs.as_ref().to_path_buf();
        ensure!(
            certs.is_file(),
            "Client certificate does not exist: {:?}",
            &certs
        );

        let key = key.as_ref().to_path_buf();
        ensure!(
            key.is_file(),
            "Client private key does not exist: {:?}",
            &key
        );

        Ok(Self { certs, key })
    }
}
