/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::{TryFrom, TryInto};
use std::pin::Pin;

use curl::easy::Easy2;
use futures::prelude::*;

use crate::{
    driver::MultiDriver,
    errors::{Abort, HttpClientError},
    handler::{Buffered, Streaming},
    pool::Pool,
    progress::Progress,
    receiver::{ChannelReceiver, Receiver},
    request::{Request, StreamRequest},
    response::{AsyncResponse, Response},
    stats::Stats,
};

pub type ResponseStream =
    Pin<Box<dyn Stream<Item = Result<AsyncResponse, HttpClientError>> + Send + 'static>>;
pub type StatsFuture =
    Pin<Box<dyn Future<Output = Result<Stats, HttpClientError>> + Send + 'static>>;

/// An async-compatible HTTP client powered by libcurl.
///
/// Essentially a more ergonomic API for working with
/// libcurl's multi interface. See URL for details:
///
/// https://curl.haxx.se/libcurl/c/libcurl-multi.html
///
/// NOTE: If you do not need to perform multiple concurrent
/// requests, you may want to use  `Request::send` instead.
#[derive(Clone)]
pub struct HttpClient {
    pool: Pool,
}

impl HttpClient {
    pub fn new() -> Self {
        Self { pool: Pool::new() }
    }

    /// Perform multiple HTTP requests concurrently.
    ///
    /// This function will block until all transfers have completed.
    /// Whenever a transfer completes, the user-provided closure
    /// will be called with the result.
    ///
    /// The closure returns a boolean. If false, this function will
    /// return early and all other pending transfers will be aborted.
    pub fn send<I, F>(&self, requests: I, response_cb: F) -> Result<Stats, HttpClientError>
    where
        I: IntoIterator<Item = Request>,
        F: FnMut(Result<Response, HttpClientError>) -> Result<(), Abort>,
    {
        self.send_with_progress(requests, response_cb, |_| ())
    }

    /// Same as `send()`, but takes an additional closure for
    /// monitoring the collective progress of all of the transfers.
    /// The closure will be called whenever any of the underlying
    /// transfers make progress.
    pub fn send_with_progress<I, F, P>(
        &self,
        requests: I,
        mut response_cb: F,
        progress_cb: P,
    ) -> Result<Stats, HttpClientError>
    where
        I: IntoIterator<Item = Request>,
        F: FnMut(Result<Response, HttpClientError>) -> Result<(), Abort>,
        P: FnMut(Progress),
    {
        let multi = self.pool.multi();
        let driver = MultiDriver::new(multi.get(), progress_cb);

        for request in requests {
            let handle: Easy2<Buffered> = request.try_into()?;
            driver.add(handle)?;
        }

        driver.perform(|res| {
            let res = res
                .map_err(|(_, e)| e.into())
                .and_then(|mut easy| Response::try_from(easy.get_mut()));
            response_cb(res)
        })
    }

    /// Async version of `send` which runs all of the given request concurrently
    /// in another thread. Returns a stream of responses (returned in the order
    /// in which they arrive) as well as a future that will return aggregated
    /// transfer statistics once all of the requests have completed.
    ///
    /// Note that the response stream will yield a `Response` whenever all of
    /// the headers for that responses have been received. The response body is
    /// available as a `Stream` inside of each returned `Response` struct
    pub fn send_async<I: IntoIterator<Item = Request>>(
        &self,
        requests: I,
    ) -> Result<(ResponseStream, StatsFuture), HttpClientError> {
        self.send_async_with_progress(requests, |_| ())
    }

    /// Same as `send_async()`, but takes an additional closure for
    /// monitoring the collective progress of all of the transfers.
    /// The closure will be called whenever any of the underlying
    /// transfers make progress.
    pub fn send_async_with_progress<I, P>(
        &self,
        requests: I,
        progress_cb: P,
    ) -> Result<(ResponseStream, StatsFuture), HttpClientError>
    where
        I: IntoIterator<Item = Request>,
        P: FnMut(Progress) + Send + 'static,
    {
        let client = self.clone();

        let mut stream_requests = Vec::new();
        let response_stream = stream::FuturesUnordered::new();
        for req in requests {
            let (receiver, streams) = ChannelReceiver::new();

            let req = req.into_streaming(receiver);
            stream_requests.push(req);

            let res = AsyncResponse::new(streams);
            response_stream.push(res);
        }

        let task = tokio::task::spawn_blocking(move || {
            client.stream_with_progress(stream_requests, progress_cb)
        });

        let stats = task
            .err_into::<HttpClientError>()
            .map(|res| Ok(res??))
            .boxed();

        Ok((response_stream.boxed(), stats))
    }

    /// Perform the given requests, but stream the responses to the
    /// `Receiver` attached to each respective request rather than
    /// buffering the content of each response.
    ///
    /// Note that this function is not asynchronous; it WILL BLOCK
    /// until all of the transfers are complete, and will return
    /// the total stats across all transfers when complete.
    pub fn stream<I, R>(&self, requests: I) -> Result<Stats, HttpClientError>
    where
        I: IntoIterator<Item = StreamRequest<R>>,
        R: Receiver,
    {
        self.stream_with_progress(requests, |_| ())
    }

    /// Same as `stream()`, but takes an additional closure for
    /// monitoring the collective progress of all of the transfers.
    /// The closure will be called whenever any of the underlying
    /// transfers make progress.
    pub fn stream_with_progress<I, R, P>(
        &self,
        requests: I,
        progress_cb: P,
    ) -> Result<Stats, HttpClientError>
    where
        I: IntoIterator<Item = StreamRequest<R>>,
        R: Receiver,
        P: FnMut(Progress),
    {
        let multi = self.pool.multi();
        let driver = MultiDriver::new(multi.get(), progress_cb);

        for request in requests {
            let handle: Easy2<Streaming<R>> = request.try_into()?;
            driver.add(handle)?;
        }

        driver.perform(report_result_and_drop_receiver)
    }
}

/// Callback for `MultiDriver::perform` when working with
/// a `Streaming` handler. Reports the result of the
/// completed request to the handler's `Receiver`.
fn report_result_and_drop_receiver<R: Receiver>(
    res: Result<Easy2<Streaming<R>>, (Easy2<Streaming<R>>, curl::Error)>,
) -> Result<(), Abort> {
    // We need to get the `Easy2` handle in both the
    // success and error cases since we ultimately
    // need to pass the result to the handler contained
    // therein.
    let (mut easy, res) = match res {
        Ok(easy) => (easy, Ok(())),
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
        let req1 = Request::get(url1);

        let url2 = server_url.join("test2")?;
        let req2 = Request::get(url2);

        let url3 = server_url.join("test3")?;
        let req3 = Request::get(url3);

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
        let req1 = Request::get(url1).into_streaming(rcv1.clone());

        let url2 = server_url.join("test2")?;
        let rcv2 = TestReceiver::new();
        let req2 = Request::get(url2).into_streaming(rcv2.clone());

        let url3 = server_url.join("test3")?;
        let rcv3 = TestReceiver::new();
        let req3 = Request::get(url3).into_streaming(rcv3.clone());

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

    #[tokio::test]
    async fn test_async() -> Result<()> {
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
        let req1 = Request::get(url1);

        let url2 = server_url.join("test2")?;
        let req2 = Request::get(url2);

        let url3 = server_url.join("test3")?;
        let req3 = Request::get(url3);

        let client = HttpClient::new();
        let (stream, stats) = client.send_async(vec![req1, req2, req3])?;

        let responses = stream.try_collect::<Vec<_>>().await?;

        mock1.assert();
        mock2.assert();
        mock3.assert();

        let mut not_received = HashSet::new();
        not_received.insert(body1.to_vec());
        not_received.insert(body2.to_vec());
        not_received.insert(body3.to_vec());

        for res in responses {
            assert_eq!(res.status, StatusCode::CREATED);
            let body = res.body.try_concat().await?;
            assert!(not_received.remove(&*body));
        }

        assert!(not_received.is_empty());

        let stats = stats.await?;
        assert_eq!(stats.requests, 3);

        Ok(())
    }
}
