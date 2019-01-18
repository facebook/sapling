// Copyright Facebook, Inc. 2018
//! mononokeapi - A Mononoke API server client library for Mercurial

#[macro_use]
extern crate failure;
extern crate futures;
extern crate http;
extern crate hyper;
extern crate hyper_tls;
extern crate native_tls;
extern crate openssl;
extern crate tokio;
extern crate url;
#[cfg(test)]
extern crate users;

use failure::{Error, Fallible};
use futures::{Future, Stream};
use http::uri::Uri;
use hyper::client::HttpConnector;
use hyper::{Body, Client};
use hyper_tls::HttpsConnector;
use native_tls::{Identity, TlsConnector};
use openssl::{pkcs12::Pkcs12, pkey::PKey, x509::X509};
use tokio::runtime::Runtime;
use url::Url;

use std::fs::File;
use std::io::Read;
use std::path::Path;

mod paths {
    pub const HEALTH_CHECK: &str = "/health_check";
}

/// An HTTP client for the Mononoke API server.
///
/// # Example
/// ```no_run
/// extern crate mononokeapi;
/// use mononokeapi::MononokeClient;
///
/// let url = "https://mononoke-api.internal.tfbnw.net";
/// let creds = Some("/var/facebook/credentials/user/x509/user.pem");
/// let client = MononokeClient::new(url, creds).unwrap();
/// client.health_check().expect("health check failed");
/// ```
pub struct MononokeClient {
    client: Client<HttpsConnector<HttpConnector>, Body>,
    base_url: Url,
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

    /// Hit the API server's /health_check endpoint.
    /// Returns Ok(()) if the expected response is received, or an Error otherwise
    /// (e.g., if there was a connection problem or an unexpected repsonse).
    pub fn health_check(&self) -> Fallible<()> {
        let uri = self
            .base_url
            .join(paths::HEALTH_CHECK)?
            .as_str()
            .parse::<Uri>()?;
        let fut = self.client.get(uri).map_err(Error::from).and_then(|res| {
            let status = res.status();
            res.into_body()
                .concat2()
                .from_err()
                .and_then(|body| Ok(String::from_utf8(body.into_bytes().to_vec())?))
                .map(move |body| (status, body))
        });

        let mut runtime = Runtime::new()?;
        let (status, body) = runtime.block_on(fut)?;

        ensure!(
            status.is_success(),
            "Request failed (status code: {:?}): {:?}",
            &status,
            &body
        );
        ensure!(body == "I_AM_ALIVE", "Unexpected response: {:?}", &body);

        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    use users::{get_current_uid, get_user_by_uid};

    use std::path::PathBuf;

    const HOST: &str = "https://mononoke-api.internal.tfbnw.net";

    fn init_client() -> MononokeClient {
        let creds = Some(get_creds_path());
        MononokeClient::new(HOST, creds).expect("failed to initialize client")
    }

    fn get_creds_path() -> PathBuf {
        let uid = get_current_uid();
        let user = get_user_by_uid(uid).expect(&format!("uid {} not found", uid));
        let name = user
            .name()
            .to_str()
            .expect(&format!("username {:?} is not valid UTF-8", user.name()));
        PathBuf::from(format!(
            "/var/facebook/credentials/{user}/x509/{user}.pem",
            user = &name
        ))
    }

    #[test]
    #[ignore] // Talks to production Mononoke; ignore by default.
    fn health_check() -> Fallible<()> {
        init_client().health_check()
    }
}
