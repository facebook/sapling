// Copyright Facebook, Inc. 2019

use failure::Fallible;
use hyper::client::HttpConnector;
use hyper::{Body, Client};
use hyper_tls::HttpsConnector;
use native_tls::{Identity, TlsConnector};
use openssl::{pkcs12::Pkcs12, pkey::PKey, x509::X509};
use url::Url;

use std::{fs::File, io::Read, path::Path};

/// An HTTP client for the Mononoke API server.
///
/// # Example
/// ```no_run
/// use mononokeapi::{MononokeApi, MononokeClient};
///
/// let url = "https://mononoke-api.internal.tfbnw.net";
/// let creds = Some("/var/facebook/credentials/user/x509/user.pem");
/// let client = MononokeClient::new(url, creds).unwrap();
/// client.health_check().expect("health check failed");
/// ```
pub struct MononokeClient {
    pub(crate) client: Client<HttpsConnector<HttpConnector>, Body>,
    pub(crate) base_url: Url,
}

impl MononokeClient {
    /// Initialize a new MononokeClient.
    ///
    /// The Mononoke API server requires TLS mutual authentication when being accessed
    /// via a proxy (e.g., through a VIP). As such, users must pass valid client credentials
    /// to the client constructor. (Specifically, a path to a PEM file containing the client
    /// certificate and private key.)
    pub fn new<P: AsRef<Path>>(base_url: &str, client_creds: Option<P>) -> Fallible<Self> {
        let base_url = Url::parse(base_url)?;
        let creds = match client_creds {
            Some(path) => Some(read_credentials(path)?),
            None => None,
        };
        let client = build_client(creds)?;
        Ok(Self { client, base_url })
    }
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
fn build_client(
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
