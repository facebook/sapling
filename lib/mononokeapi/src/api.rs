// Copyright Facebook, Inc. 2019

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use bytes::Bytes;
use failure::{ensure, Error, Fallible};
use futures::{stream::futures_unordered, Future, IntoFuture, Stream};
use hyper::Chunk;
use tokio::runtime::Runtime;
use url::Url;

use revisionstore::{DataPackVersion, Delta, Key, Metadata, MutableDataPack, MutablePack};
use url_ext::UrlExt;

use crate::client::{HyperClient, MononokeClient};

mod paths {
    pub const HEALTH_CHECK: &str = "/health_check";
    pub const GET_FILE: &str = "gethgfile/";
}

pub trait MononokeApi {
    fn health_check(&self) -> Fallible<()>;
    fn get_files(&self, keys: impl IntoIterator<Item = Key>) -> Fallible<PathBuf>;
}

impl MononokeApi for MononokeClient {
    /// Hit the API server's /health_check endpoint.
    /// Returns Ok(()) if the expected response is received, or an Error otherwise
    /// (e.g., if there was a connection problem or an unexpected repsonse).
    fn health_check(&self) -> Fallible<()> {
        let url = self.base_url.join(paths::HEALTH_CHECK)?.to_uri();

        let fut = self.client.get(url).map_err(Error::from).and_then(|res| {
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

        // Construct an iterator of Futures, each representing an individual
        // getfile request.
        let get_file_futures = keys
            .into_iter()
            .map(move |key| get_file(&client, &prefix, key));

        // Construct a Future that executes the getfiles requests concurrently,
        // returned the results in a Vec in arbitrary order.
        let work = futures_unordered(get_file_futures).collect();

        // Run the Futures.
        let mut runtime = Runtime::new()?;
        let files = runtime.block_on(work)?;

        // Write the downloaded file content to disk.
        write_datapack(self.pack_cache_path(), files)
    }
}

/// Fetch an individual file from the API server by Key.
fn get_file(
    client: &Arc<HyperClient>,
    url_prefix: &Url,
    key: Key,
) -> impl Future<Item = (Key, Bytes), Error = Error> {
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

/// Creates a new datapack in the given directory, and populates it with the file
/// contents provided by the given iterator. Each Delta written to the datapack is
/// assumed to contain the full text of the corresponding file, and as a result the
/// base revision for each file is always specified as None.
fn write_datapack(
    pack_dir: impl AsRef<Path>,
    files: impl IntoIterator<Item = (Key, Bytes)>,
) -> Fallible<PathBuf> {
    let mut datapack = MutableDataPack::new(pack_dir.as_ref(), DataPackVersion::One)?;
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
