// Copyright Facebook, Inc. 2019

use std::{
    cmp,
    fmt::Write,
    fs, mem,
    path::PathBuf,
    time::{Duration, Instant},
};

use curl::{
    self,
    easy::{Easy2, Handler, HttpVersion, List, WriteError},
    multi::{Easy2Handle, Multi},
};
use failure::{bail, ensure, err_msg, Fallible};
use itertools::Itertools;
use log;
use serde::{de::DeserializeOwned, Serialize};
use serde_json;
use url::Url;

use types::{FileDataRequest, FileDataResponse, FileHistoryRequest, FileHistoryResponse, Key};

use crate::api::EdenApi;
use crate::config::{ClientCreds, Config};
use crate::packs::{write_datapack, write_historypack};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

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
    batch_size: Option<usize>,
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
            batch_size: config.batch_size,
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

    /// Configure a new curl::Easy2 handle using this client's settings.
    fn easy(&self) -> Fallible<Easy2<Collector>> {
        let mut handle = Easy2::new(Collector::new());
        if let Some(ClientCreds { ref certs, ref key }) = &self.creds {
            handle.ssl_cert(certs)?;
            handle.ssl_key(key)?;
        }
        handle.http_version(HttpVersion::V2)?;
        Ok(handle)
    }

    /// Send multiple concurrent POST requests using the given requests as the
    /// JSON payload of each respective request. Assumes that the responses are
    /// also JSON encoded, and automatically deserializes and returns them.
    fn request_json_multi<'a, R, T, I>(&'a self, url: &Url, requests: I) -> Fallible<Vec<T>>
    where
        R: Serialize,
        T: DeserializeOwned,
        I: IntoIterator<Item = R>,
    {
        let requests = requests.into_iter().collect::<Vec<_>>();
        let num_requests = requests.len();

        let mut driver = MultiDriver::with_capacity(num_requests);
        for request in requests {
            let mut easy = self.easy()?;
            prepare_json_post(&mut easy, &url, &request)?;
            driver.add(easy)?;
        }

        log::debug!("Performing {} requests", num_requests);
        let start = Instant::now();
        let handles = driver.perform(true)?.into_result()?;
        let elapsed = start.elapsed();

        let mut responses = Vec::with_capacity(handles.len());
        let mut total_bytes = 0;
        for easy in handles {
            let data = &easy.get_ref().0;
            total_bytes += data.len();

            let response = serde_json::from_slice::<T>(data)?;
            responses.push(response);
        }

        print_download_stats(total_bytes, elapsed);
        Ok(responses)
    }
}

impl EdenApi for EdenApiCurlClient {
    fn health_check(&self) -> Fallible<()> {
        let mut handle = self.easy()?;
        let url = self.base_url.join(paths::HEALTH_CHECK)?;
        handle.url(url.as_str())?;
        handle.get(true)?;
        handle.perform()?;

        let code = handle.response_code()?;
        ensure!(code == 200, "Received HTTP status code {}", code);

        let response = String::from_utf8_lossy(&handle.get_ref().0);
        ensure!(
            response == "I_AM_ALIVE",
            "Unexpected response: {:?}",
            &response
        );

        Ok(())
    }

    fn get_files(&self, keys: Vec<Key>) -> Fallible<PathBuf> {
        log::debug!("Fetching {} files", keys.len());

        let url = self.repo_base_url()?.join(paths::DATA)?;
        let batch_size = self.batch_size.unwrap_or(cmp::max(keys.len(), 1));
        let num_requests = (keys.len() + batch_size - 1) / batch_size;

        log::debug!("Using batch size: {}", batch_size);
        log::debug!("Preparing {} requests", num_requests);

        let chunks = keys.into_iter().chunks(batch_size);
        let requests = (&chunks).into_iter().map(|batch| FileDataRequest {
            keys: batch.into_iter().collect(),
        });

        let responses: Vec<FileDataResponse> = self.request_json_multi(&url, requests)?;

        log::debug!(
            "Received {} responses with {} total entries",
            responses.len(),
            responses
                .iter()
                .map(|entry| entry.files.len())
                .sum::<usize>(),
        );

        let cache_path = self.pack_cache_path();
        log::debug!("Writing pack file in directory: {:?}", &cache_path);
        write_datapack(cache_path, responses.into_iter().flatten())
    }

    fn get_history(&self, keys: Vec<Key>, max_depth: Option<u32>) -> Fallible<PathBuf> {
        log::debug!("Fetching {} files", keys.len());

        let url = self.repo_base_url()?.join(paths::HISTORY)?;
        let batch_size = self.batch_size.unwrap_or(cmp::max(keys.len(), 1));
        let num_requests = (keys.len() + batch_size - 1) / batch_size;

        log::debug!("Using batch size: {}", batch_size);
        log::debug!("Preparing {} requests", num_requests);

        let chunks = keys.into_iter().chunks(batch_size);
        let requests = (&chunks).into_iter().map(|batch| FileHistoryRequest {
            keys: batch.into_iter().collect(),
            depth: max_depth,
        });

        let responses: Vec<FileHistoryResponse> = self.request_json_multi(&url, requests)?;

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

/// Simple Handler that just writes all received data to an internal buffer.
#[derive(Default)]
struct Collector(Vec<u8>);

impl Collector {
    fn new() -> Self {
        Default::default()
    }
}

impl Handler for Collector {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        self.0.extend_from_slice(data);
        Ok(data.len())
    }
}

/// The result of using a MultiDriver to manage a curl::Multi session.
/// Contains all of the Easy2 handles for the session along with
/// information about which (if any) of the transfers failed.
struct MultiDriverResult<H> {
    handles: Vec<Easy2<H>>,
    failed: Vec<(usize, curl::Error)>,
}

impl<H> MultiDriverResult<H> {
    fn into_result(self) -> Fallible<Vec<Easy2<H>>> {
        if self.failed.is_empty() {
            return Ok(self.handles);
        }

        let mut msg = "The following transfers failed:\n".to_string();
        for (i, e) in self.failed {
            write!(&mut msg, "{}: {}\n", i, e)?;
        }

        Err(err_msg(msg))
    }
}

/// Struct that manages a curl::Multi session, synchronously driving
/// all of the transfers therein to completion.
struct MultiDriver<H> {
    multi: Multi,
    handles: Vec<Easy2Handle<H>>,
}

impl<H: Handler> MultiDriver<H> {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            multi: Multi::new(),
            handles: Vec::with_capacity(capacity),
        }
    }

    /// Add an Easy2 handle to the Multi stack.
    fn add(&mut self, easy: Easy2<H>) -> Fallible<()> {
        // Assign a token to this Easy2 handle so we can correlate messages
        // for this handle with the corresponding Easy2Handle while the
        // Easy2 is owned by the Multi handle.
        let token = self.handles.len();
        let mut handle = self.multi.add2(easy)?;
        handle.set_token(token)?;
        self.handles.push(handle);
        Ok(())
    }

    /// Remove and return all of the Easy2 handles in the Multi stack.
    fn remove_all(&mut self) -> Fallible<Vec<Easy2<H>>> {
        let handles = mem::replace(&mut self.handles, Vec::with_capacity(0));
        let mut easy_vec = Vec::with_capacity(handles.len());
        for handle in handles {
            let easy = self.multi.remove2(handle)?;
            easy_vec.push(easy);
        }
        Ok(easy_vec)
    }

    /// Drive all of the Easy2 handles in the Multi stack to completion.
    ///
    /// If `fail_early` is set to true, then this method will return early if
    /// any transfers fail (leaving the remaining transfers in an unfinished
    /// state); otherwise, the driver will only return once all transfers
    /// have completed (successfully or otherwise).
    ///
    /// Returns all of the Easy2 handles in the Multi stack in the order
    /// they were added, along with the indices of any failed transfers
    /// (along with the corresponding error code).
    fn perform(&mut self, fail_early: bool) -> Fallible<MultiDriverResult<H>> {
        let num_transfers = self.handles.len();
        let mut in_progress = num_transfers;
        let mut failed = Vec::new();
        let mut i = 0;

        while in_progress > 0 {
            log::trace!(
                "Iteration {}: {}/{} transfers complete",
                i,
                num_transfers - in_progress,
                num_transfers
            );
            i += 1;

            in_progress = self.multi.perform()? as usize;

            // Check for messages; a message indicates a transfer completed (successfully or not).
            self.multi.messages(|msg| {
                let token = msg.token().unwrap();
                log::trace!("Got message for transfer {}", token);
                match msg.result() {
                    Some(Ok(())) => {
                        log::trace!("Transfer {} complete", token);
                    }
                    Some(Err(e)) => {
                        log::trace!("Transfer {} failed", token);
                        failed.push((token, e));
                    }
                    None => {
                        // Theoretically this should never happen because
                        // this closure is only called on completion.
                        log::trace!("Transfer {} incomplete", token);
                    }
                }
            });

            if fail_early && failed.len() > 0 {
                log::debug!("At least one transfer failed; aborting.");
                break;
            }

            let timeout = self.multi.get_timeout()?.unwrap_or(DEFAULT_TIMEOUT);
            log::trace!("Waiting for I/O with timeout: {:?}", &timeout);

            let num_active_transfers = self.multi.wait(&mut [], Duration::from_secs(1))?;
            if num_active_transfers == 0 {
                log::trace!("Timed out waiting for I/O; polling active transfers anyway.");
            }
        }

        let handles = self.remove_all()?;
        Ok(MultiDriverResult { handles, failed })
    }
}

/// Configure the given Easy2 handle to perform a POST request.
/// The given payload will be serialized as JSON and used as the request body.
fn prepare_json_post<H, R: Serialize>(easy: &mut Easy2<H>, url: &Url, request: &R) -> Fallible<()> {
    let payload = serde_json::to_vec(&request)?;

    easy.url(url.as_str())?;
    easy.post(true)?;
    easy.post_fields_copy(&payload)?;

    let mut headers = List::new();
    headers.append("Content-Type: application/json")?;
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
