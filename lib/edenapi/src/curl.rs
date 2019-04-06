// Copyright Facebook, Inc. 2019

use std::{fs, path::PathBuf};

use curl::easy::{Easy2, Handler, HttpVersion, List, WriteError};
use failure::{bail, ensure, Fallible};
use serde_json;
use url::Url;

use types::{FileDataRequest, FileDataResponse, FileHistoryRequest, FileHistoryResponse, Key};

use crate::api::EdenApi;
use crate::config::{ClientCreds, Config};
use crate::packs::{write_datapack, write_historypack};

mod paths {
    pub const HEALTH_CHECK: &str = "/health_check";
    pub const DATA: &str = "eden/data";
    pub const HISTORY: &str = "eden/history";
}

pub struct EdenApiCurlClient {
    base_url: Url,
    repo: String,
    cache_path: PathBuf,
    creds: Option<ClientCreds>,
}

impl EdenApiCurlClient {
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

        let client = Self {
            base_url,
            repo,
            cache_path,
            creds: config.creds,
        };

        // Create repo/packs directory in cache if it doesn't already exist.
        fs::create_dir_all(client.pack_cache_path())?;

        Ok(client)
    }

    fn repo_base_url(&self) -> Fallible<Url> {
        Ok(self.base_url.join(&format!("{}/", &self.repo))?)
    }

    fn pack_cache_path(&self) -> PathBuf {
        self.cache_path.join(&self.repo).join("packs")
    }

    fn handle<H: Handler>(&self, handler: H) -> Fallible<Easy2<H>> {
        let mut handle = Easy2::new(handler);
        if let Some(ClientCreds { ref certs, ref key }) = &self.creds {
            handle.ssl_cert(certs)?;
            handle.ssl_key(key)?;
        }
        handle.http_version(HttpVersion::V2)?;
        Ok(handle)
    }
}

impl EdenApi for EdenApiCurlClient {
    fn health_check(&self) -> Fallible<()> {
        let mut handle = self.handle(Collector::new())?;
        let url = self.base_url.join(paths::HEALTH_CHECK)?;
        handle.url(url.as_str())?;
        handle.get(true)?;
        handle.perform()?;

        let code = handle.response_code()?;
        ensure!(code == 200, "Received HTTP status code {}", code);

        let response = String::from_utf8_lossy(handle.get_ref().data());
        ensure!(
            response == "I_AM_ALIVE",
            "Unexpected response: {:?}",
            &response
        );

        Ok(())
    }

    fn get_files(&self, keys: Vec<Key>) -> Fallible<PathBuf> {
        let url = self.repo_base_url()?.join(paths::DATA)?;

        let request = FileDataRequest {
            keys: keys.into_iter().collect(),
        };
        let payload = serde_json::to_vec(&request)?;

        let mut handle = self.handle(Collector::new())?;
        handle.url(url.as_str())?;
        handle.post(true)?;
        handle.post_fields_copy(&payload)?;

        let mut headers = List::new();
        headers.append("Content-Type: application/json")?;
        handle.http_headers(headers)?;

        handle.perform()?;

        let response: FileDataResponse = serde_json::from_slice(handle.get_ref().data())?;
        write_datapack(self.pack_cache_path(), response)
    }

    fn get_history(&self, keys: Vec<Key>, max_depth: Option<u32>) -> Fallible<PathBuf> {
        let url = self.repo_base_url()?.join(paths::HISTORY)?;

        let request = FileHistoryRequest {
            keys: keys.into_iter().collect(),
            depth: max_depth,
        };
        let payload = serde_json::to_vec(&request)?;

        let mut handle = self.handle(Collector::new())?;
        handle.url(url.as_str())?;
        handle.post(true)?;
        handle.post_fields_copy(&payload)?;

        let mut headers = List::new();
        headers.append("Content-Type: application/json")?;
        handle.http_headers(headers)?;

        handle.perform()?;

        let response: FileHistoryResponse = serde_json::from_slice(handle.get_ref().data())?;
        write_historypack(self.pack_cache_path(), response)
    }
}

#[derive(Default)]
struct Collector(Vec<u8>);

impl Collector {
    fn new() -> Self {
        Default::default()
    }

    fn data(&self) -> &[u8] {
        &self.0
    }
}

impl Handler for Collector {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        self.0.extend_from_slice(data);
        Ok(data.len())
    }
}
