// Copyright Facebook, Inc. 2019

use failure::{bail, Fallible};
use hyper::client::HttpConnector;
use hyper::{Body, Client};
use hyper_tls::HttpsConnector;
use native_tls::{Identity, TlsConnector};
use openssl::{pkcs12::Pkcs12, pkey::PKey, x509::X509};
use url::Url;

use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub struct MononokeClientBuilder {
    base_url: Option<Url>,
    client_creds: Option<PathBuf>,
}

impl MononokeClientBuilder {
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

    /// Set client credentials (X.509 client certificate + private key) for TLS
    /// mutual authentication. If not set, TLS mutual authentication will not be
    /// used.
    pub fn client_creds(mut self, path: impl AsRef<Path>) -> Self {
        self.client_creds = Some(path.as_ref().to_path_buf());
        self
    }

    /// Optionally set client credentials (X.509 client certificate + private key)
    /// for TLS mutual authentication. Takes an Option, allowing for the option to
    /// be unset by passing None.
    pub fn client_creds_opt<P: AsRef<Path>>(mut self, path: Option<P>) -> Self {
        self.client_creds = path.map(|p| p.as_ref().to_path_buf());
        self
    }

    pub fn build(self) -> Fallible<MononokeClient> {
        let base_url = match self.base_url {
            Some(url) => url,
            None => bail!("No base URL specified"),
        };

        let creds = match self.client_creds {
            Some(path) => Some(read_credentials(path)?),
            None => None,
        };
        let client = build_hyper_client(creds)?;

        Ok(MononokeClient { client, base_url })
    }
}

/// An HTTP client for the Mononoke API server.
///
/// # Example
/// ```no_run
/// use failure::Fallible;
/// use mononokeapi::{MononokeApi, MononokeClient, MononokeClientBuilder};
///
/// fn main() -> Fallible<()> {
///     let client = MononokeClientBuilder::new()
///         .base_url_str("https://mononoke-api.internal.tfbnw.net")?
///         .client_creds("/var/facebook/credentials/user/x509/user.pem")
///         .build()?;
///
///     client.health_check()
/// }
/// ```
pub struct MononokeClient {
    pub(crate) client: Client<HttpsConnector<HttpConnector>, Body>,
    pub(crate) base_url: Url,
}

/// Read an X.509 certificate and private key from a PEM file and
/// convert them to a native_tls::Identity. Both the certificate
/// and key must be in the same PEM file.
fn read_credentials(path: impl AsRef<Path>) -> Fallible<Identity> {
    let mut pem_bytes = Vec::new();
    File::open(path)?.read_to_end(&mut pem_bytes)?;

    // Read the X.509 certificate and private key from the PEM file.
    let cert = X509::from_pem(&pem_bytes)?;
    let key = PKey::private_key_from_pem(&pem_bytes)?;

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
    Ok(Client::builder().build::<_, Body>(https))
}
