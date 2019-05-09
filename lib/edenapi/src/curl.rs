// Copyright Facebook, Inc. 2019

use std::{
    cmp, fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use curl::{
    self,
    easy::{Easy2, Handler, HttpVersion, List, WriteError},
};
use failure::{bail, ensure, Fallible};
use itertools::Itertools;
use log;
use serde::{de::DeserializeOwned, Serialize};
use serde_cbor;
use url::Url;

use driver::MultiDriver;
use types::{
    api::{FileDataRequest, FileDataResponse, FileHistoryRequest, FileHistoryResponse},
    Key,
};

use crate::api::EdenApi;
use crate::config::{ClientCreds, Config};
use crate::packs::{write_datapack, write_historypack};
use crate::progress::{ProgressFn, ProgressHandle, ProgressManager, ProgressStats};

mod driver;

mod paths {
    pub const HEALTH_CHECK: &str = "/health_check";
    pub const HOSTNAME: &str = "/hostname";
    pub const DATA: &str = "eden/data";
    pub const HISTORY: &str = "eden/history";
}

pub struct EdenApiCurlClient {
    base_url: Url,
    repo: String,
    cache_path: PathBuf,
    creds: Option<ClientCreds>,
    data_batch_size: Option<usize>,
    history_batch_size: Option<usize>,
    validate_files: bool,
}

// Public API.
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
            data_batch_size: config.data_batch_size,
            history_batch_size: config.history_batch_size,
            validate_files: config.validate_files,
        };

        // Create repo/packs directory in cache if it doesn't already exist.
        fs::create_dir_all(client.pack_cache_path())?;

        Ok(client)
    }
}

impl EdenApi for EdenApiCurlClient {
    fn health_check(&self) -> Fallible<()> {
        let handler = Collector::new();
        let mut handle = self.easy(handler)?;
        let url = self.base_url.join(paths::HEALTH_CHECK)?;
        handle.url(url.as_str())?;
        handle.get(true)?;
        handle.perform()?;

        let code = handle.response_code()?;
        ensure!(code == 200, "Received HTTP status code {}", code);

        let response = String::from_utf8_lossy(&handle.get_ref().data());
        ensure!(
            response == "I_AM_ALIVE",
            "Unexpected response: {:?}",
            &response
        );

        Ok(())
    }

    fn hostname(&self) -> Fallible<String> {
        let handler = Collector::new();
        let mut handle = self.easy(handler)?;
        let url = self.base_url.join(paths::HOSTNAME)?;
        handle.url(url.as_str())?;
        handle.get(true)?;
        handle.perform()?;

        let code = handle.response_code()?;
        ensure!(code == 200, "Received HTTP status code {}", code);

        let response = String::from_utf8(handle.get_ref().data().to_vec())?;
        Ok(response)
    }

    fn get_files(&self, keys: Vec<Key>, progress: Option<ProgressFn>) -> Fallible<PathBuf> {
        log::debug!("Fetching {} files", keys.len());

        let url = self.repo_base_url()?.join(paths::DATA)?;
        let batch_size = self.data_batch_size.unwrap_or(cmp::max(keys.len(), 1));
        let num_requests = (keys.len() + batch_size - 1) / batch_size;

        log::debug!("Using batch size: {}", batch_size);
        log::debug!("Preparing {} requests", num_requests);

        let chunks = keys.into_iter().chunks(batch_size);
        let requests = (&chunks).into_iter().map(|batch| FileDataRequest {
            keys: batch.into_iter().collect(),
        });

        let responses: Vec<FileDataResponse> = self.multi_request(&url, requests, progress)?;

        log::debug!(
            "Received {} responses with {} total entries",
            responses.len(),
            responses
                .iter()
                .map(|response| response.entries.len())
                .sum::<usize>(),
        );

        let mut files = Vec::new();
        for entry in responses.into_iter().flatten() {
            if self.validate_files {
                log::trace!("Validating file: {}", &entry.key);
                entry.validate()?;
            }
            files.push((entry.key, entry.data));
        }

        let cache_path = self.pack_cache_path();
        log::debug!("Writing pack file in directory: {:?}", &cache_path);
        write_datapack(cache_path, files)
    }

    fn get_history(
        &self,
        keys: Vec<Key>,
        max_depth: Option<u32>,
        progress: Option<ProgressFn>,
    ) -> Fallible<PathBuf> {
        log::debug!("Fetching {} files", keys.len());

        let url = self.repo_base_url()?.join(paths::HISTORY)?;
        let batch_size = self.history_batch_size.unwrap_or(cmp::max(keys.len(), 1));
        let num_requests = (keys.len() + batch_size - 1) / batch_size;

        log::debug!("Using batch size: {}", batch_size);
        log::debug!("Preparing {} requests", num_requests);

        let chunks = keys.into_iter().chunks(batch_size);
        let requests = (&chunks).into_iter().map(|batch| FileHistoryRequest {
            keys: batch.into_iter().collect(),
            depth: max_depth,
        });

        let responses: Vec<FileHistoryResponse> = self.multi_request(&url, requests, progress)?;

        log::debug!(
            "Received {} responses with {} total entries",
            responses.len(),
            responses
                .iter()
                .map(|entry| entry.entries.len())
                .sum::<usize>(),
        );

        let cache_path = self.pack_cache_path();
        log::debug!("Writing pack file in directory: {:?}", &cache_path);
        write_historypack(cache_path, responses.into_iter().flatten())
    }
}

// Private methods.
impl EdenApiCurlClient {
    fn repo_base_url(&self) -> Fallible<Url> {
        Ok(self.base_url.join(&format!("{}/", &self.repo))?)
    }

    fn pack_cache_path(&self) -> PathBuf {
        self.cache_path.join(&self.repo).join("packs")
    }

    /// Configure a new curl::Easy2 handle using this client's settings.
    fn easy<H: Handler>(&self, handler: H) -> Fallible<Easy2<H>> {
        let mut handle = Easy2::new(handler);
        if let Some(ClientCreds { ref certs, ref key }) = &self.creds {
            handle.ssl_cert(certs)?;
            handle.ssl_key(key)?;
        }
        handle.http_version(HttpVersion::V2)?;
        handle.progress(true)?;
        Ok(handle)
    }

    /// Send multiple concurrent POST requests using the given requests as the
    /// JSON payload of each respective request. Assumes that the responses are
    /// CBOR encoded, and automatically deserializes and returns them.
    fn multi_request<I, T, R>(
        &self,
        url: &Url,
        requests: I,
        progress_cb: Option<ProgressFn>,
    ) -> Fallible<Vec<T>>
    where
        R: Serialize,
        T: DeserializeOwned,
        I: IntoIterator<Item = R>,
    {
        let requests = requests.into_iter().collect::<Vec<_>>();
        let num_requests = requests.len();

        let mut progress = ProgressManager::with_capacity(num_requests);
        let mut driver = MultiDriver::with_capacity(num_requests);

        for request in requests {
            let handle = progress.register();
            let handler = Collector::with_progress(handle);
            let mut easy = self.easy(handler)?;
            prepare_cbor_post(&mut easy, &url, &request)?;
            driver.add(easy)?;
        }

        progress.set_callback(progress_cb);
        driver.set_progress_manager(progress);

        log::debug!("Performing {} requests", num_requests);
        let start = Instant::now();
        let handles = driver.perform(true)?.into_result()?;
        let elapsed = start.elapsed();

        let mut responses = Vec::with_capacity(handles.len());
        let mut total_bytes = 0;
        for easy in handles {
            let data = &easy.get_ref().data();
            total_bytes += data.len();

            let response = serde_cbor::from_slice::<T>(data)?;
            responses.push(response);
        }

        print_download_stats(total_bytes, elapsed);
        Ok(responses)
    }
}

/// Simple Handler that just writes all received data to an internal buffer.
struct Collector {
    data: Vec<u8>,
    progress: Option<ProgressHandle>,
}

impl Collector {
    fn new() -> Self {
        Self {
            data: Vec::new(),
            progress: None,
        }
    }

    fn with_progress(progress: ProgressHandle) -> Self {
        Self {
            data: Vec::new(),
            progress: Some(progress),
        }
    }

    fn data(&self) -> &[u8] {
        &self.data
    }
}

impl Handler for Collector {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        self.data.extend_from_slice(data);
        Ok(data.len())
    }

    fn progress(&mut self, dltotal: f64, dlnow: f64, ultotal: f64, ulnow: f64) -> bool {
        if let Some(ref progress) = self.progress {
            let dltotal = dltotal as u64;
            let dlnow = dlnow as u64;
            let ultotal = ultotal as u64;
            let ulnow = ulnow as u64;
            let stats = ProgressStats::new(dlnow, ulnow, dltotal, ultotal);
            progress.update(stats);
        }
        true
    }
}

/// Configure the given Easy2 handle to perform a POST request.
/// The given payload will be serialized as CBOR and used as the request body.
fn prepare_cbor_post<H, R: Serialize>(easy: &mut Easy2<H>, url: &Url, request: &R) -> Fallible<()> {
    let payload = serde_cbor::to_vec(&request)?;

    easy.url(url.as_str())?;
    easy.post(true)?;
    easy.post_fields_copy(&payload)?;

    let mut headers = List::new();
    headers.append("Content-Type: application/cbor")?;
    easy.http_headers(headers)?;

    Ok(())
}

fn print_download_stats(total_bytes: usize, elapsed: Duration) {
    let seconds = elapsed.as_secs() as f64 + elapsed.subsec_nanos() as f64 / 1_000_000_000.0;
    let rate = total_bytes as f64 * 8.0 / 1_000_000.0 / seconds;
    log::info!(
        "Downloaded {} bytes in {:?} ({:.6} Mb/s)",
        total_bytes,
        elapsed,
        rate
    );
}
