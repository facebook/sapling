// Copyright Facebook, Inc. 2019

use std::{
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
    sync::Arc,
};

use failure::{bail, ensure, format_err, Fallible};
use hyper::{client::HttpConnector, Body, Client};
use hyper_rustls::HttpsConnector;
use rustls::{internal::pemfile, Certificate, ClientConfig, PrivateKey};
use url::Url;
use webpki_roots::TLS_SERVER_ROOTS;

use crate::client::{EdenApiHttpClient, HyperClient};

#[derive(Default)]
pub struct Builder {
    base_url: Option<Url>,
    creds: Option<ClientCreds>,
    repo: Option<String>,
    cache_path: Option<PathBuf>,
}

impl Builder {
    pub(crate) fn new() -> Self {
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
    pub fn client_creds(mut self, cert: impl AsRef<Path>, key: impl AsRef<Path>) -> Fallible<Self> {
        self.creds = Some(ClientCreds::new(cert, key)?);
        Ok(self)
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

    pub fn build(self) -> Fallible<EdenApiHttpClient> {
        let base_url = match self.base_url {
            Some(url) => url,
            None => bail!("No base URL specified"),
        };

        let repo = match self.repo {
            Some(repo) => repo,
            None => bail!("No repo name specified"),
        };

        let cache_path = match self.cache_path {
            Some(path) => path,
            None => bail!("No cache path specified"),
        };
        ensure!(
            cache_path.is_dir(),
            "Configured cache path {:?} is not a directory",
            &cache_path
        );

        let hyper_client = build_hyper_client(self.creds)?;

        let client = EdenApiHttpClient {
            client: Arc::new(hyper_client),
            base_url,
            repo,
            cache_path,
        };

        // Create repo/packs directory in cache if it doesn't already exist.
        fs::create_dir_all(client.pack_cache_path())?;

        Ok(client)
    }
}

/// Client credentials for TLS mutual authentication, including an X.509 client
/// certificate chain and an RSA or ECDSA private key.
struct ClientCreds {
    cert_chain: Vec<Certificate>,
    key: PrivateKey,
}

impl ClientCreds {
    /// Read client credentials from the specified PEM files.
    fn new(cert_pem: impl AsRef<Path>, key_pem: impl AsRef<Path>) -> Fallible<Self> {
        Ok(Self {
            cert_chain: read_cert_chain(cert_pem)?,
            key: read_key(key_pem)?,
        })
    }
}

/// Read and parse a PEM-encoded X.509 client certificate chain.
fn read_cert_chain(path: impl AsRef<Path>) -> Fallible<Vec<Certificate>> {
    let mut reader = BufReader::new(File::open(path.as_ref())?);
    pemfile::certs(&mut reader).map_err(|()| {
        format_err!(
            "failed to read certificates from PEM file: {:?}",
            path.as_ref()
        )
    })
}

/// Read and parse a PEM-encoded RSA or ECDSA private key.
fn read_key(path: impl AsRef<Path>) -> Fallible<PrivateKey> {
    let mut reader = BufReader::new(File::open(path.as_ref())?);
    pemfile::pkcs8_private_keys(&mut reader)
        .and_then(|mut keys| keys.pop().ok_or(()))
        .map_err(|()| {
            format_err!(
                "failed to read private key from PEM file: {:?}",
                path.as_ref()
            )
        })
}

/// Set up a Hyper client that configured to support HTTP/2 over TLS (with ALPN).
/// Optionally takes client credentials to be used for TLS mutual authentication.
fn build_hyper_client(creds: Option<ClientCreds>) -> Fallible<HyperClient> {
    let num_dns_threads = 1;
    let mut http = HttpConnector::new(num_dns_threads);

    // Allow URLs to begin with "https://" since we intend to use TLS.
    http.enforce_http(false);

    let mut config = ClientConfig::new();
    config
        .root_store
        .add_server_trust_anchors(&TLS_SERVER_ROOTS);
    if let Some(creds) = creds {
        config.set_single_client_cert(creds.cert_chain, creds.key);
    }

    // Tell the server to use HTTP/2 during protocol negotiation.
    config.alpn_protocols.push("h2".to_string());

    let connector = HttpsConnector::from((http, config));
    let client = Client::builder()
        .http2_only(true)
        .build::<_, Body>(connector);
    Ok(client)
}
