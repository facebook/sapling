// Copyright Facebook, Inc. 2019

use std::{
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
    sync::Arc,
};

use bytes::{Buf, Bytes, IntoBuf};
use failure::{bail, ensure, format_err, Error, Fallible};
use futures::{stream, Future, IntoFuture, Stream};
use hyper::{client::HttpConnector, Body, Chunk, Client};
use hyper_rustls::HttpsConnector;
use percent_encoding::{percent_encode, DEFAULT_ENCODE_SET};
use rustls::{internal::pemfile, Certificate, ClientConfig, PrivateKey};
use serde_json::Deserializer;
use tokio::runtime::Runtime;
use url::Url;
use webpki_roots::TLS_SERVER_ROOTS;

use types::{HistoryEntry, Key, WireHistoryEntry};
use url_ext::UrlExt;

use crate::api::EdenApi;
use crate::config::{ClientCreds, Config};
use crate::packs::{write_datapack, write_historypack};

pub(crate) type HyperClient = Client<HttpsConnector<HttpConnector>, Body>;

pub struct EdenApiHyperClient {
    client: Arc<HyperClient>,
    base_url: Url,
    repo: String,
    cache_path: PathBuf,
}

impl EdenApiHyperClient {
    pub fn new(config: Config) -> Fallible<Self> {
        let base_url = match config.base_url {
            Some(url) => url,
            None => bail!("No base URL specified"),
        };

        let repo = match config.repo {
            Some(repo) => repo,
            None => bail!("No repo name specified"),
        };

        let cache_path = match config.cache_path {
            Some(path) => path,
            None => bail!("No cache path specified"),
        };
        ensure!(
            cache_path.is_dir(),
            "Configured cache path {:?} is not a directory",
            &cache_path
        );

        let hyper_client = build_hyper_client(config.creds)?;

        let client = EdenApiHyperClient {
            client: Arc::new(hyper_client),
            base_url,
            repo,
            cache_path,
        };

        // Create repo/packs directory in cache if it doesn't already exist.
        fs::create_dir_all(client.pack_cache_path())?;

        Ok(client)
    }

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

mod paths {
    pub const HEALTH_CHECK: &str = "/health_check";
    pub const HOSTNAME: &str = "/hostname";
    pub const GET_FILE: &str = "gethgfile/";
    pub const GET_HISTORY: &str = "getfilehistory/";
}

impl EdenApi for EdenApiHyperClient {
    /// Hit the API server's /health_check endpoint.
    /// Returns Ok(()) if the expected response is received, or an Error otherwise
    /// (e.g., if there was a connection problem or an unexpected repsonse).
    fn health_check(&self) -> Fallible<()> {
        let url = self.base_url.join(paths::HEALTH_CHECK)?.to_uri();

        let fut = self.client.get(url).map_err(Error::from).and_then(|res| {
            log::debug!("Received response: {:#?}", &res);
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

    /// Get the hostname of the API server.
    fn hostname(&self) -> Fallible<String> {
        let url = self.base_url.join(paths::HOSTNAME)?.to_uri();

        let fut = self.client.get(url).map_err(Error::from).and_then(|res| {
            log::debug!("Received response: {:#?}", &res);
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

        Ok(body)
    }

    /// Fetch the content of the specified file from the API server and write
    /// it to a datapack in the configured cache directory. Returns the path
    /// of the resulting packfile.
    fn get_files(&self, keys: Vec<Key>) -> Fallible<PathBuf> {
        let client = Arc::clone(&self.client);
        let prefix = self.repo_base_url()?.join(paths::GET_FILE)?;

        let get_file_futures = keys
            .into_iter()
            .map(move |key| get_file(&client, &prefix, key));

        let work = stream::futures_unordered(get_file_futures).collect();

        let mut runtime = Runtime::new()?;
        let files = runtime.block_on(work)?;

        write_datapack(self.pack_cache_path(), files)
    }

    /// Fetch the history of the specified file from the API server and write
    /// it to a historypack in the configured cache directory. Returns the path
    /// of the resulting packfile.
    fn get_history(&self, keys: Vec<Key>, max_depth: Option<u32>) -> Fallible<PathBuf> {
        let client = Arc::clone(&self.client);
        let prefix = self.repo_base_url()?.join(paths::GET_HISTORY)?;

        let get_history_futures = keys
            .into_iter()
            .map(move |key| get_history(&client, &prefix, key, max_depth).collect());

        let work = stream::futures_unordered(get_history_futures).collect();

        let mut runtime = Runtime::new()?;
        let entries = runtime.block_on(work)?.into_iter().flatten();

        write_historypack(self.pack_cache_path(), entries)
    }
}

/// Fetch an individual file from the API server by Key.
fn get_file(
    client: &Arc<HyperClient>,
    url_prefix: &Url,
    key: Key,
) -> impl Future<Item = (Key, Bytes), Error = Error> {
    log::debug!("Fetching file content for key: {}", &key);
    let filenode = key.node.to_hex();
    url_prefix
        .join(&filenode)
        .into_future()
        .from_err()
        .and_then({
            let client = Arc::clone(client);
            move |url| client.get(url.to_uri()).from_err()
        })
        .and_then(|res| {
            let status = res.status();
            res.into_body()
                .concat2()
                .from_err()
                .map(|body: Chunk| body.into_bytes())
                .and_then(move |body| {
                    // If we got an error, intepret the body as an error
                    // message and fail the Future.
                    ensure!(
                        status.is_success(),
                        "Request failed (status code: {:?}): {:?}",
                        &status,
                        String::from_utf8_lossy(&body).into_owned(),
                    );
                    Ok((key, body))
                })
        })
}

/// Fetch the history of an individual file from the API server by Key.
fn get_history(
    client: &Arc<HyperClient>,
    url_prefix: &Url,
    key: Key,
    max_depth: Option<u32>,
) -> impl Stream<Item = HistoryEntry, Error = Error> {
    log::debug!("Fetching history for key: {}", &key);
    let filenode = key.node.to_hex();
    let filename = url_encode(key.path.as_byte_slice());
    url_prefix
        .join(&format!("{}/", &filenode))
        .into_future()
        .and_then(move |url| url.join(&filename))
        .map(move |mut url| {
            if let Some(depth) = max_depth {
                url.query_pairs_mut()
                    .append_pair("depth", &depth.to_string());
            }
            url
        })
        .from_err()
        .and_then({
            let client = Arc::clone(client);
            move |url| client.get(url.to_uri()).from_err()
        })
        .and_then(|res| {
            let status = res.status();
            res.into_body()
                .concat2()
                .from_err()
                .map(|body: Chunk| body.into_bytes())
                .and_then(move |body: Bytes| {
                    // If we got an error, intepret the body as an error
                    // message and fail the Future.
                    ensure!(
                        status.is_success(),
                        "Request failed (status code: {:?}): {:?}",
                        &status,
                        String::from_utf8_lossy(&body).into_owned(),
                    );
                    Ok(body)
                })
        })
        .map(process_body)
        .flatten_stream()
        .map(move |entry| HistoryEntry::from_wire(entry, key.path.clone()))
}

fn process_body(body: Bytes) -> impl Stream<Item = WireHistoryEntry, Error = Error> {
    let entries = Deserializer::from_reader(body.into_buf().reader()).into_iter();
    stream::iter_result(entries).from_err()
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
        let certs = read_cert_chain(creds.certs)?;
        let key = read_key(creds.key)?;
        config.set_single_client_cert(certs, key);
    }

    // Tell the server to use HTTP/2 during protocol negotiation.
    config.alpn_protocols.push("h2".to_string());

    let connector = HttpsConnector::from((http, config));
    let client = Client::builder()
        .http2_only(true)
        .build::<_, Body>(connector);
    Ok(client)
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

fn url_encode(bytes: &[u8]) -> String {
    percent_encode(bytes, DEFAULT_ENCODE_SET).to_string()
}
