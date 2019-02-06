// Copyright Facebook, Inc. 2019

use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

use failure::{bail, ensure, Fallible};
use hyper::client::HttpConnector;
use hyper::{Body, Client};
use hyper_tls::HttpsConnector;
use native_tls::{Identity, TlsConnector};
use openssl::{pkcs12::Pkcs12, pkey::PKey, x509::X509};
use same_file::is_same_file;
use url::Url;

use crate::client::EdenApiHttpClient;

#[derive(Default)]
pub struct Builder {
    base_url: Option<Url>,
    client_id: Option<Identity>,
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

    /// Convenience function for setting client credentials when the certificate and
    /// private key are in the same PEM file.
    pub fn client_creds(self, cert_and_key: impl AsRef<Path>) -> Fallible<Self> {
        self.client_creds2(cert_and_key.as_ref(), cert_and_key.as_ref())
    }

    /// Set client credentials by providing paths to a PEM encoded X.509 client certificate
    /// and a PEM encoded private key. These credentials are used for TLS mutual authentication;
    /// if not set, mutual authentication will not be used.
    pub fn client_creds2(self, cert: impl AsRef<Path>, key: impl AsRef<Path>) -> Fallible<Self> {
        Ok(self.client_id(Some(read_identity(cert, key)?)))
    }

    /// Directly set the client credentials with a native_tls::Identity rather than
    /// parsing them from a PEM file. If None is specified, TLS mutual authentication will
    /// be disabled.
    pub fn client_id(mut self, id: Option<Identity>) -> Self {
        self.client_id = id;
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

        let hyper_client = build_hyper_client(self.client_id)?;

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

fn read_bytes(path: impl AsRef<Path>) -> Fallible<Vec<u8>> {
    let mut buf = Vec::new();
    File::open(path)?.read_to_end(&mut buf)?;
    Ok(buf)
}

/// Read an X.509 certificate and private key from PEM files and
/// convert them to a native_tls::Identity. If both paths refer
/// to the same file, it is only read once.
fn read_identity(cert_path: impl AsRef<Path>, key_path: impl AsRef<Path>) -> Fallible<Identity> {
    let cert_pem = read_bytes(&cert_path)?;
    let cert = X509::from_pem(&cert_pem)?;

    // The certificate and key might be in same PEM file, in which
    // case, don't duplicate the read.
    let key = if is_same_file(&cert_path, &key_path)? {
        PKey::private_key_from_pem(&cert_pem)?
    } else {
        let key_pem = read_bytes(&key_path)?;
        PKey::private_key_from_pem(&key_pem)?
    };

    // Build a DER-encoded PKCS#12 archive, since that's what native_tls accepts.
    // The password is intentionally set to the empty string because the archive
    // will be immediately read by the constructor for native_tls::Identity.
    let password = "";
    let pkcs12_der_bytes = Pkcs12::builder()
        .build(password, "client certificate and key", &key, &cert)?
        .to_der()?;

    Ok(Identity::from_pkcs12(&pkcs12_der_bytes, &password)?)
}

/// Build a hyper::Client that supports HTTPS. Optionally takes a client identity
/// (certificate + private key) which, if provided, will be used for TLS mutual authentication.
fn build_hyper_client(
    client_id: Option<Identity>,
) -> Fallible<Client<HttpsConnector<HttpConnector>, Body>> {
    let mut builder = TlsConnector::builder();
    if let Some(id) = client_id {
        let _ = builder.identity(id);
    }
    let tls = builder.build()?;

    let num_dns_threads = 1;
    let mut http = HttpConnector::new(num_dns_threads);
    http.enforce_http(false);

    let https = HttpsConnector::from((http, tls));
    let client = Client::builder().http2_only(true).build::<_, Body>(https);

    Ok(client)
}
