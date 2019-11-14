/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::{Path, PathBuf};

use failure::ResultExt;
use url::Url;

use auth::AuthConfig;
use configparser::{config::ConfigSet, hg::ConfigSetHgExt};

use crate::errors::{ApiErrorKind, ApiResult};

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

    pub fn from_hg_config(config: &ConfigSet) -> ApiResult<Self> {
        let base_url = config
            .get_opt("edenapi", "url")
            .context(ApiErrorKind::BadConfig("edenapi.url".into()))?
            .map(|s: String| s.parse())
            .transpose()?;
        let creds = base_url
            .as_ref()
            .and_then(|url| AuthConfig::new(&config).auth_for_url(url))
            .and_then(|auth| match (auth.cert, auth.key) {
                (Some(cert), Some(key)) => Some(ClientCreds::new(cert, key)),
                _ => None,
            })
            .transpose()?;

        let repo = config
            .get_opt("remotefilelog", "reponame")
            .context(ApiErrorKind::BadConfig("remotefilelog.reponame".into()))?;
        let data_batch_size = config
            .get_opt("edenapi", "databatchsize")
            .context(ApiErrorKind::BadConfig("edenapi.databatchsize".into()))?;
        let history_batch_size = config
            .get_opt("edenapi", "historybatchsize")
            .context(ApiErrorKind::BadConfig("edenapi.historybatchsize".into()))?;
        let validate = config
            .get_or_default("edenapi", "validate")
            .context(ApiErrorKind::BadConfig("edenapi.validate".into()))?;
        let stream_data = config
            .get_or_default("edenapi", "streamdata")
            .context(ApiErrorKind::BadConfig("edenapi.streamdata".into()))?;
        let stream_history = config
            .get_or_default("edenapi", "streamhistory")
            .context(ApiErrorKind::BadConfig("edenapi.streamhistory".into()))?;
        let stream_trees = config
            .get_or_default("edenapi", "streamtrees")
            .context(ApiErrorKind::BadConfig("edenapi.streamtrees".into()))?;

        Ok(Self {
            base_url,
            creds,
            repo,
            data_batch_size,
            history_batch_size,
            validate,
            stream_data,
            stream_history,
            stream_trees,
        })
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
    pub(crate) cert: PathBuf,
    pub(crate) key: PathBuf,
}

impl ClientCreds {
    pub fn new(cert: impl AsRef<Path>, key: impl AsRef<Path>) -> ApiResult<Self> {
        let cert = cert.as_ref().to_path_buf();
        if !cert.is_file() {
            return Err(ApiErrorKind::BadCertificate(cert).into());
        }

        let key = key.as_ref().to_path_buf();
        if !key.is_file() {
            return Err(ApiErrorKind::BadCertificate(key).into());
        }

        Ok(Self { cert, key })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs::File;

    use failure::Fallible;
    use tempdir::TempDir;

    use configparser::config::Options;

    #[test]
    fn test_from_hg_config() -> Fallible<()> {
        // Need to ensure that configured cert and key
        // paths actually exist on disk since otherwise
        // the auth crate will ignore the auth settings.
        let tmp = TempDir::new("test_from_hg_config")?;
        let cert = tmp.path().to_path_buf().join("cert.pem");
        let key = tmp.path().to_path_buf().join("key.pem");
        let _ = File::create(&cert)?;
        let _ = File::create(&key)?;

        let mut hg_config = ConfigSet::new();
        let _errors = hg_config.parse(
            format!(
                "[remotefilelog]\n\
                 reponame = repo\n\
                 [edenapi]\n\
                 url = https://example.com/repo\n\
                 databatchsize = 1234\n\
                 historybatchsize = 5678\n\
                 validate = true\n\
                 streamdata = true\n\
                 [auth]\n\
                 edenapi.prefix = example.com\n\
                 edenapi.cert = {}\n\
                 edenapi.key = {}\n\
                 ",
                cert.to_string_lossy(),
                key.to_string_lossy()
            ),
            &Options::default(),
        );
        let config = Config::from_hg_config(&hg_config)?;

        assert_eq!(config.repo, Some("repo".into()));
        assert_eq!(config.base_url, Some("https://example.com/repo".parse()?));
        assert_eq!(config.creds.as_ref().expect("cert missing").cert, cert);
        assert_eq!(config.creds.as_ref().expect("key missing").key, key);
        assert_eq!(config.data_batch_size, Some(1234));
        assert_eq!(config.history_batch_size, Some(5678));
        assert_eq!(config.validate, true);
        assert_eq!(config.stream_data, true);
        assert_eq!(config.stream_history, false);
        assert_eq!(config.stream_trees, false);

        Ok(())
    }
}
