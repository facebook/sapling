// Copyright Facebook, Inc. 2018
//! mononokeapi - A Mononoke API server client library for Mercurial

extern crate bytes;
#[macro_use]
extern crate failure;
extern crate futures;
extern crate http;
extern crate hyper;
extern crate hyper_tls;
#[macro_use]
extern crate lazy_static;
extern crate native_tls;
extern crate openssl;
extern crate tokio;
#[cfg(test)]
extern crate users;

use bytes::Bytes;
use failure::Error;
use futures::{Future, Stream};
use http::uri::{Authority, Parts, PathAndQuery, Scheme, Uri};
use hyper::{Body, Client};
use hyper::client::HttpConnector;
use hyper_tls::HttpsConnector;
use native_tls::{Identity, TlsConnector};
use openssl::{pkcs12::Pkcs12, pkey::PKey, x509::X509};
use tokio::runtime::Runtime;

use std::fs::File;
use std::io::Read;
use std::path::Path;

type Result<T> = std::result::Result<T, Error>;

mod paths {
    use super::*;

    lazy_static! {
        pub static ref HEALTH_CHECK: PathAndQuery = PathAndQuery::from_static("/health_check");
    }
}

/// An HTTP client for the Mononoke API server.
///
/// # Examples
///
/// ```no_run
/// extern crate mononokeapi;
/// use mononokeapi::MononokeClient;
///
/// let host = "mononoke-api.internal.tfbnw.net";
/// let creds = "/var/facebook/credentials/user/x509/user.pem";
/// let client = MononokeClient::new(host, creds).unwrap();
/// client.health_check().expect("health check failed");
/// ```
pub struct MononokeClient {
    client: Client<HttpsConnector<HttpConnector>, Body>,
    hostport: Authority,
}

impl MononokeClient {
    /// Initialize a new MononokeClient.
    ///
    /// The Mononoke API server requires TLS mutual authentication when being accessed
    /// via a proxy (e.g., through a VIP). As such, users must pass valid client credentials
    /// to the client constructor. (Specifically, a path to a PEM file containing the client
    /// certificate and private key.)
    pub fn new(hostport: impl Into<Bytes>, client_creds: impl AsRef<Path>) -> Result<Self> {
        let creds = read_credentials(client_creds)?;
        Ok(Self {
            client: build_client(creds)?,
            hostport: Authority::from_shared(hostport.into())?,
        })
    }

    /// Hit the API server's /health_check endpoint.
    /// Returns Ok(()) if the expected response is received, or an Error otherwise
    /// (e.g., if there was a connection problem or an unexpected repsonse).
    pub fn health_check(&self) -> Result<()> {
        let uri = self.build_uri(&paths::HEALTH_CHECK)?;
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

    /// Construct a URI using this client's configured hostport as the
    /// base URI. The scheme will always be HTTPS.
    fn build_uri(&self, path: &PathAndQuery) -> Result<Uri> {
        let mut parts = Parts::default();
        parts.scheme = Some(Scheme::HTTPS);
        parts.authority = Some(self.hostport.clone());
        parts.path_and_query = Some(path.clone());
        Ok(Uri::from_parts(parts)?)
    }
}

/// Read an X.509 certificate and private key from a PEM file and
/// convert them to a native_tls::Identity. Both the certificate
/// and key must be in the same PEM file.
fn read_credentials(path: impl AsRef<Path>) -> Result<Identity> {
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

/// Build a hyper::Client configured to support TLS mutual authentication, using
/// the provided client identity (certificate + private key) as client credentials.
fn build_client(client_id: Identity) -> Result<Client<HttpsConnector<HttpConnector>, Body>> {
    let tls = TlsConnector::builder().identity(client_id).build()?;

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

    const HOST: &str = "mononoke-api.internal.tfbnw.net";

    fn get_creds_path() -> PathBuf {
        let uid = get_current_uid();
        let user = get_user_by_uid(uid).expect(&format!("uid {} not found", uid));
        let name = user.name()
            .to_str()
            .expect(&format!("username {:?} is not valid UTF-8", user.name()));
        PathBuf::from(format!(
            "/var/facebook/credentials/{user}/x509/{user}.pem",
            user = &name
        ))
    }

    #[test]
    #[ignore] // Talks to production Mononoke; ignore by default.
    fn health_check() -> Result<()> {
        let creds = get_creds_path();
        MononokeClient::new(HOST, creds)?.health_check()
    }
}
