/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryInto;

use curl::{easy::Easy2, multi::Multi};

use crate::{
    driver::MultiDriver,
    errors::{Abort, HttpClientError},
    handler::{Buffered, Streaming},
    progress::Progress,
    receiver::Receiver,
    request::{Request, StreamRequest},
    response::{get_status_code, Response},
    stats::Stats,
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
    pub fn send<'a, I, F>(&self, requests: I, response_cb: F) -> Result<Stats, HttpClientError>
    where
        I: IntoIterator<Item = Request<'a>>,
        F: FnMut(Result<Response, HttpClientError>) -> Result<(), Abort>,
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
    ) -> Result<Stats, HttpClientError>
    where
        I: IntoIterator<Item = Request<'a>>,
        F: FnMut(Result<Response, HttpClientError>) -> Result<(), Abort>,
        P: FnMut(Progress),
    {
        let driver = MultiDriver::new(&self.multi, progress_cb);

        for request in requests {
            let handle: Easy2<Buffered> = request.try_into()?;
            driver.add(handle)?;
        }

        driver.perform(|res| {
            let res = res
                .map_err(|(_, e)| e.into())
                .and_then(Response::from_handle);
            response_cb(res)
        })
    }

    /// Perform the given requests, but stream the responses to the
    /// `Receiver` attached to each respective request rather than
    /// buffering the content of each response.
    ///
    /// Note that this function is not asynchronous; it WILL BLOCK
    /// until all of the transfers are complete, and will return
    /// the total stats across all transfers when complete.
    pub fn stream<'a, I, R>(&self, requests: I) -> Result<Stats, HttpClientError>
    where
        I: IntoIterator<Item = StreamRequest<'a, R>>,
        R: Receiver,
    {
        self.stream_with_progress(requests, |_| ())
    }

    /// Same as `stream()`, but takes an additional closure for
    /// monitoring the collective progress of all of the transfers.
    /// The closure will be called whenever any of the underlying
    /// transfers make progress.
    pub fn stream_with_progress<'a, I, R, P>(
        &self,
        requests: I,
        progress_cb: P,
    ) -> Result<Stats, HttpClientError>
    where
        I: IntoIterator<Item = StreamRequest<'a, R>>,
        R: Receiver,
        P: FnMut(Progress),
    {
        let driver = MultiDriver::new(&self.multi, progress_cb);

        for request in requests {
            let handle: Easy2<Streaming<R>> = request.try_into()?;
            driver.add(handle)?;
        }

        driver.perform(report_status_and_drop_receiver)
    }
}

/// From [libcurl's documentation][1]:
///
/// > You must never share the same handle in multiple threads. You can pass the
/// > handles around among threads, but you must never use a single handle from
/// > more than one thread at any given time.
///
/// `curl::Multi` does not implement `Send` or `Sync` because of the above
/// constraints. In particular, it is not `Sync` because libcurl has no
/// internal synchronization, and it is not `Send` because a single Multi
/// session can only be used to drive transfers from a single thread at
/// any one time.
///
/// In this case, all usage of the `Multi` session to drive a collection of
/// transfers is confined to an individual call to `HttpClient::send`. All
/// of the transfer handles are created, driven to completion, and removed
/// from the `Multi` session within that single call. As such, the client
/// maintains the invariant that the `Multi` session contains no transfers
/// outside of a `send` call, so it can be safely sent across threads.
///
/// [1]: https://curl.haxx.se/libcurl/c/threadsafe.html
unsafe impl Send for HttpClient {}

/// Callback for `MultiDriver::perform` when working with
/// a `Streaming` handler. Reports the status code of the
/// completed request to the handler's `Receiver`.
fn report_status_and_drop_receiver<R: Receiver>(
    res: Result<Easy2<Streaming<R>>, (Easy2<Streaming<R>>, curl::Error)>,
) -> Result<(), Abort> {
    // We need to get the `Easy2` handle in both the
    // success and error cases since we ultimately
    // need to pass the result to the handler contained
    // therein.
    let (mut easy, res) = match res {
        Ok(mut easy) => {
            let status = get_status_code(&mut easy);
            (easy, status)
        }
        Err((easy, e)) => (easy, Err(e.into())),
    };

    // Extract the `Receiver` from the `Streaming` handler
    // inside the Easy2 handle. If it's already gone, just
    // log it and move on. (This shouldn't normally happen.)
    if let Some(receiver) = easy.get_mut().take_receiver() {
        receiver.done(res)
    } else {
        log::trace!("Cannot report status because receiver is missing");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashSet;

    use anyhow::Result;
    use http::StatusCode;
    use mockito::mock;
    use url::Url;

    use crate::receiver::testutil::TestReceiver;

    #[test]
    fn test_client() -> Result<()> {
        let body1 = b"body1";
        let body2 = b"body2";
        let body3 = b"body3";

        let mock1 = mock("GET", "/test1")
            .with_status(201)
            .with_body(&body1)
            .create();

        let mock2 = mock("GET", "/test2")
            .with_status(201)
            .with_body(&body2)
            .create();

        let mock3 = mock("GET", "/test3")
            .with_status(201)
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

        let client = HttpClient::new();
        let stats = client.send(vec![req1, req2, req3], |res| {
            let res = res.unwrap();
            assert_eq!(res.status, StatusCode::CREATED);
            assert!(not_received.remove(&*res.body));
            Ok(())
        })?;

        mock1.assert();
        mock2.assert();
        mock3.assert();

        assert!(not_received.is_empty());
        assert_eq!(stats.requests, 3);

        Ok(())
    }

    #[test]
    fn test_stream() -> Result<()> {
        let body1 = b"body1";
        let body2 = b"body2";
        let body3 = b"body3";

        let mock1 = mock("GET", "/test1")
            .with_status(201)
            .with_body(&body1)
            .create();

        let mock2 = mock("GET", "/test2")
            .with_status(201)
            .with_body(&body2)
            .create();

        let mock3 = mock("GET", "/test3")
            .with_status(201)
            .with_body(&body3)
            .create();

        let server_url = Url::parse(&mockito::server_url())?;

        let url1 = server_url.join("test1")?;
        let rcv1 = TestReceiver::new();
        let req1 = Request::get(&url1).into_streaming(rcv1.clone());

        let url2 = server_url.join("test2")?;
        let rcv2 = TestReceiver::new();
        let req2 = Request::get(&url2).into_streaming(rcv2.clone());

        let url3 = server_url.join("test3")?;
        let rcv3 = TestReceiver::new();
        let req3 = Request::get(&url3).into_streaming(rcv3.clone());

        let client = HttpClient::new();
        let stats = client.stream(vec![req1, req2, req3])?;

        mock1.assert();
        mock2.assert();
        mock3.assert();
        assert_eq!(stats.requests, 3);

        assert_eq!(rcv1.status().unwrap(), StatusCode::CREATED);
        let body = rcv1.chunks().into_iter().flatten().collect::<Vec<_>>();
        assert_eq!(&*body, body1);

        assert_eq!(rcv2.status().unwrap(), StatusCode::CREATED);
        let body = rcv2.chunks().into_iter().flatten().collect::<Vec<_>>();
        assert_eq!(&*body, body2);

        assert_eq!(rcv3.status().unwrap(), StatusCode::CREATED);
        let body = rcv3.chunks().into_iter().flatten().collect::<Vec<_>>();
        assert_eq!(&*body, body3);

        Ok(())
    }
}
