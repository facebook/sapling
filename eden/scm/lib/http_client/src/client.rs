/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use curl::multi::Multi;

use crate::{
    driver::MultiDriver, errors::HttpClientError, progress::Progress, request::Request,
    response::Response,
};

/// A simple callback-oriented HTTP client.
///
/// Essentially a more ergonomic API for working with
/// libcurl's multi interface. See URL for details:
///
/// https://curl.haxx.se/libcurl/c/libcurl-multi.html
///
/// NOTE: If you do not need to perform multiple concurrent
/// requests, you may want to use  `Request::send` instead.
pub struct HttpClient {
    multi: Multi,
}

impl HttpClient {
    pub fn new() -> Self {
        Self {
            multi: Multi::new(),
        }
    }

    /// Perform multiple HTTP requests concurrently.
    ///
    /// This function will block until all transfers have completed.
    /// Whenever a transfer completes, the user-provided closure
    /// will be called with the result.
    ///
    /// The closure returns a boolean. If false, this function will
    /// return early and all other pending transfers will be aborted.
    pub fn send<'a, I, F>(&self, requests: I, response_cb: F) -> Result<(), HttpClientError>
    where
        I: IntoIterator<Item = Request<'a>>,
        F: FnMut(Result<Response, HttpClientError>) -> bool,
    {
        self.send_with_progress(requests, response_cb, |_| ())
    }

    /// Same as `send()`, but takes an additional closure for
    /// monitoring the collective progress of all of the transfers.
    /// The closure will be called whenever any of the underlying
    /// transfers make progress.
    pub fn send_with_progress<'a, I, F, P>(
        &self,
        requests: I,
        mut response_cb: F,
        progress_cb: P,
    ) -> Result<(), HttpClientError>
    where
        I: IntoIterator<Item = Request<'a>>,
        F: FnMut(Result<Response, HttpClientError>) -> bool,
        P: FnMut(Progress),
    {
        let driver = MultiDriver::new(&self.multi, progress_cb);

        for request in requests {
            let handle = request.into_handle()?;
            driver.add(handle)?;
        }

        driver.perform(|res| {
            let res = res.map_err(Into::into).and_then(Response::from_handle);
            response_cb(res)
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashSet;

    use anyhow::Result;
    use mockito::mock;
    use url::Url;

    #[test]
    fn test_client() -> Result<()> {
        let body1 = b"body1";
        let body2 = b"body2";
        let body3 = b"body3";

        let mock1 = mock("GET", "/test1")
            .with_status(200)
            .with_body(&body1)
            .create();

        let mock2 = mock("GET", "/test2")
            .with_status(200)
            .with_body(&body2)
            .create();

        let mock3 = mock("GET", "/test3")
            .with_status(200)
            .with_body(&body3)
            .create();

        let server_url = Url::parse(&mockito::server_url())?;

        let url1 = server_url.join("test1")?;
        let req1 = Request::get(&url1);

        let url2 = server_url.join("test2")?;
        let req2 = Request::get(&url2);

        let url3 = server_url.join("test3")?;
        let req3 = Request::get(&url3);

        let mut not_received = HashSet::new();
        not_received.insert(body1.to_vec());
        not_received.insert(body2.to_vec());
        not_received.insert(body3.to_vec());

        let handle_response = |res: Result<Response, HttpClientError>| -> bool {
            assert!(not_received.remove(&*res.unwrap().body));
            true
        };

        let client = HttpClient::new();
        client.send(vec![req1, req2, req3], handle_response)?;

        mock1.assert();
        mock2.assert();
        mock3.assert();

        assert!(not_received.is_empty());

        Ok(())
    }
}
