// Copyright Facebook, Inc. 2019

use std::path::{Path, PathBuf};

use bytes::Bytes;
use failure::{ensure, Error, Fallible};
use futures::{Future, Stream};
use tokio::runtime::Runtime;

use revisionstore::{DataPackVersion, Delta, Key, MutableDataPack, MutablePack};
use url_ext::UrlExt;

use crate::client::MononokeClient;

mod paths {
    pub const HEALTH_CHECK: &str = "/health_check";
    pub const GET_FILE: &str = "gethgfile/";
}

pub trait MononokeApi {
    fn health_check(&self) -> Fallible<()>;
    fn get_file(&self, key: Key) -> Fallible<PathBuf>;
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
    fn get_file(&self, key: Key) -> Fallible<PathBuf> {
        let url = self
            .repo_base_url()?
            .join(paths::GET_FILE)?
            .join(&key.node().to_hex())?
            .to_uri();

        let fut = self.client.get(url).map_err(Error::from).and_then(|res| {
            let status = res.status();
            res.into_body()
                .concat2()
                .from_err()
                .map(move |body| (status, body.into_bytes()))
        });

        let mut runtime = Runtime::new()?;
        let (status, data) = runtime.block_on(fut)?;

        ensure!(
            status.is_success(),
            "Request failed (status code: {:?}): {:?}",
            &status,
            &data
        );

        write_datapack(self.pack_cache_path(), vec![(key, data)])
    }
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
        let delta = Delta {
            data,
            base: None,
            key,
        };
        datapack.add(&delta, None)?;
    }
    datapack.close()
}
