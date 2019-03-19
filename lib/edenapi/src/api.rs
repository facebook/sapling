// Copyright Facebook, Inc. 2019

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use bytes::{Buf, Bytes, IntoBuf};
use failure::{ensure, Error, Fallible};
use futures::{stream, Future, IntoFuture, Stream};
use hyper::Chunk;
use percent_encoding::{percent_encode, DEFAULT_ENCODE_SET};
use serde_json::Deserializer;
use tokio::runtime::Runtime;
use url::Url;

use revisionstore::{
    DataPackVersion, Delta, HistoryPackVersion, Metadata, MutableDataPack, MutableHistoryPack,
    MutablePack,
};
use types::{HistoryEntry, Key};
use url_ext::UrlExt;

use crate::client::{EdenApiHttpClient, HyperClient};

mod paths {
    pub const HEALTH_CHECK: &str = "/health_check";
    pub const GET_FILE: &str = "gethgfile/";
    pub const GET_HISTORY: &str = "getfilehistory/";
}

pub trait EdenApi {
    fn health_check(&self) -> Fallible<()>;
    fn get_files(&self, keys: impl IntoIterator<Item = Key>) -> Fallible<PathBuf>;
    fn get_history(
        &self,
        keys: impl IntoIterator<Item = Key>,
        max_depth: Option<u32>,
    ) -> Fallible<PathBuf>;
}

impl EdenApi for EdenApiHttpClient {
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

    /// Fetch the content of the specified file from the API server and write
    /// it to a datapack in the configured cache directory. Returns the path
    /// of the resulting packfile.
    fn get_files(&self, keys: impl IntoIterator<Item = Key>) -> Fallible<PathBuf> {
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
    fn get_history(
        &self,
        keys: impl IntoIterator<Item = Key>,
        max_depth: Option<u32>,
    ) -> Fallible<PathBuf> {
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
    let filenode = key.node().to_hex();
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
    let filenode = key.node().to_hex();
    let filename = url_encode(&key.name());
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
        .map(move |body: Bytes| {
            let entries = Deserializer::from_reader(body.into_buf().reader()).into_iter();
            stream::iter_result(entries).from_err()
        })
        .flatten_stream()
        .map(move |entry| HistoryEntry::from_wire(entry, key.name().to_vec()))
}

/// Create a new datapack in the given directory, and populate it with the file
/// contents provided by the given iterator. Each Delta written to the datapack is
/// assumed to contain the full text of the corresponding file, and as a result the
/// base revision for each file is always specified as None.
fn write_datapack(
    pack_dir: impl AsRef<Path>,
    files: impl IntoIterator<Item = (Key, Bytes)>,
) -> Fallible<PathBuf> {
    let mut datapack = MutableDataPack::new(pack_dir, DataPackVersion::One)?;
    for (key, data) in files {
        let metadata = Metadata {
            size: Some(data.len() as u64),
            flags: None,
        };
        let delta = Delta {
            data,
            base: None,
            key,
        };
        datapack.add(&delta, Some(metadata))?;
    }
    datapack.close()
}

/// Create a new historypack in the given directory, and populate it
/// with the given history entries.
fn write_historypack(
    pack_dir: impl AsRef<Path>,
    entries: impl IntoIterator<Item = HistoryEntry>,
) -> Fallible<PathBuf> {
    let mut historypack = MutableHistoryPack::new(pack_dir, HistoryPackVersion::One)?;
    for entry in entries {
        historypack.add_entry(&entry)?;
    }
    historypack.close()
}

fn url_encode(bytes: &[u8]) -> String {
    percent_encode(bytes, DEFAULT_ENCODE_SET).to_string()
}
