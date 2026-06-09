/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! An async-compatible HTTP client built on top of libcurl.

// There are many layers about how to read data.
// - `curl::easy::Handler`: This is the lowest layer. It uses callbacks
//   to send and recv data, read HTTP header, update progress. However,
//   the callbacks do not handle request completion (Ok or Err). `curl`
//   requires completion handled separately by `Multi::messages`.
// - `crate::Receiver`: This is at a higher level. It uses callbacks to
//   recv data, recv HTTP header, update progress, and handle completion.
//   However, it does not handle sending data. It has 1 implementation:
//   - `ChannelReceiver`: Write data to *async* channels.
// - `crate::HandlerExt` (private): Extends `curl::easy::Handler` with
//   richer information. It handles "sending data", and has 2 versions:
//   - `crate::handler::Buffered`: Buffers data into `Vec<u8>`.
//      Does *not* use `Receiver` abstraction.
//   - `crate::handler::Streaming`: Delegates to `crate::Receiver` for
//     receiving data for async functions.
//
// Other types:
// - `HttpClient`: Configured HTTP client. Uses at least one libcurl
//   `Multi` to handle multiple requests in a single loop/thread. Async
//   requests use a small pool of long-lived dispatcher threads by
//   default, with a config escape hatch back to `spawn_blocking`.
// - `Request` / `StreamRequest`: Similar but duplicated implementation
//   to send requests.
// - `CborStream`: Turn a stream of bytes into a stream of CBOR decoded
//   items in the async world.
// - `MultiDriver`: Extends `Multi` with some higher level logic/types,
//    like `HttpClientError`, handle event listeners, progress, etc.
// - `Pool`: Workaround lifetime / !Send limitation of `Multi`.

#![allow(dead_code)]

mod claimer;
mod client;
mod dispatcher;
mod driver;
mod errors;
mod event_listeners;
mod handler;
mod header;
mod pool;
mod progress;
mod receiver;
mod request;
mod response;
mod stats;
mod stream;

pub use client::Config;
pub use client::HttpClient;
pub use client::ResponseFuture;
pub use client::StatsFuture;
pub use curl;
use curl::easy::Easy2;
pub use curl::easy::HttpVersion;
pub use errors::Abort;
pub use errors::HttpClientError;
pub use errors::TlsError;
pub use errors::TlsErrorKind;
use handler::HandlerExt;
pub use header::Header;
pub use progress::Progress;
pub use receiver::Receiver;
pub use request::Encoding;
pub use request::Method;
pub use request::MinTransferSpeed;
pub use request::Request;
pub use request::RequestContext;
pub use request::RequestInfo;
pub use request::StreamRequest;
pub use response::AsyncBody;
pub use response::AsyncResponse;
pub use response::Response;
pub use stats::Stats;
pub use stream::CborStream;

pub(crate) fn init_openssl() {
    // Force openssl to initialize to to work around openssl bug
    // https://github.com/openssl/openssl/issues/6214. Initializing openssl explicitly
    // causes openssl to use OPENSSL_INIT_NO_ATEXIT which avoids shutdown race conditions
    // (but not shutting down openssl). If we don't explicitly innitialize, curl
    // initializes openssl without OPENSSL_INIT_NO_ATEXIT and we get the race conditions.
    openssl::init();
}

/// The only Easy2 type used by this crate.
pub(crate) type Easy2H = Easy2<Box<dyn HandlerExt>>;
