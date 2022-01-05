/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Read;
use std::io::SeekFrom;
use std::mem;

use curl::easy::Handler;
use curl::easy::ReadError;
use curl::easy::SeekResult;
use curl::easy::WriteError;
use http::header;
use http::HeaderMap;
use http::StatusCode;
use http::Version;

use super::HandlerExt;
use crate::header::Header;
use crate::progress::Progress;
use crate::RequestContext;

/// Initial buffer capacity to allocate if we don't get a Content-Length header.
/// Usually, the lack of a Content-Length header indicates a streaming response,
/// in which case the body size is expected to be relatively large.
const DEFAULT_CAPACITY: usize = 1000;

/// A simple curl Handler that buffers all received data.
pub struct Buffered {
    received: Vec<u8>,
    capacity: Option<usize>,
    version: Option<Version>,
    status: Option<StatusCode>,
    headers: HeaderMap,
    bytes_sent: usize,
    is_active: bool,
    request_context: RequestContext,
}

impl Buffered {
    pub(crate) fn new(request_context: RequestContext) -> Self {
        Self {
            received: Default::default(),
            capacity: Default::default(),
            version: Default::default(),
            status: Default::default(),
            headers: Default::default(),
            bytes_sent: Default::default(),
            is_active: false,
            request_context,
        }
    }

    pub(crate) fn version(&self) -> Option<Version> {
        self.version
    }

    pub(crate) fn status(&self) -> Option<StatusCode> {
        self.status
    }

    /// Extract the received headers.
    pub(crate) fn take_headers(&mut self) -> HeaderMap {
        mem::take(&mut self.headers)
    }

    /// Extract the received data.
    pub(crate) fn take_body(&mut self) -> Vec<u8> {
        mem::take(&mut self.received)
    }

    fn trigger_first_activity(&mut self) {
        if !self.is_active {
            self.is_active = true;
            let listeners = &self.request_context.event_listeners;
            listeners.trigger_first_activity(&self.request_context);
        }
    }
}

impl Handler for Buffered {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        self.request_context
            .event_listeners
            .trigger_download_bytes(self.request_context(), data.len());
        // Set the buffer size based on the received Content-Length
        // header, or a default if we didn't get a Content-Length.
        self.received
            .reserve(self.capacity.unwrap_or(DEFAULT_CAPACITY));
        self.received.extend_from_slice(data);
        Ok(data.len())
    }

    fn read(&mut self, data: &mut [u8]) -> Result<usize, ReadError> {
        self.trigger_first_activity();

        Ok(if let Some(payload) = self.request_context.body.as_mut() {
            let sent = (&payload[self.bytes_sent..])
                .read(data)
                .expect("Failed to read from payload buffer");
            self.bytes_sent += sent;
            self.request_context
                .event_listeners
                .trigger_download_bytes(self.request_context(), sent);
            sent
        } else {
            0
        })
    }

    fn seek(&mut self, whence: SeekFrom) -> SeekResult {
        let size = match &self.request_context.body {
            Some(payload) => payload.len(),
            None => return SeekResult::CantSeek,
        };

        let (start, offset) = match whence {
            SeekFrom::Start(offset) => (0, offset as i64),
            SeekFrom::End(offset) => (size, offset),
            SeekFrom::Current(offset) => (self.bytes_sent, offset),
        };

        self.bytes_sent = if offset >= 0 {
            start.saturating_add(offset as usize).clamp(0, size)
        } else {
            start.saturating_sub(-offset as usize)
        };

        SeekResult::Ok
    }

    fn header(&mut self, data: &[u8]) -> bool {
        self.trigger_first_activity();

        match Header::parse(data) {
            Ok(Header::Header(name, value)) => {
                // XXX: This line triggers a lint error because `http::HeaderName`
                // is implemented using `bytes::Bytes` for custom headers, which has
                // interior mutability. There isn't anything we can really do here
                // since the problem is that the `http` crate declared this as `const`
                // instead of `static`. In this case, the lint error isn't actually
                // applicable since for standard headers like Content-Length, the
                // underlying representation is a simple enum, so there is actually
                // no interior mutability in use, so this line will not result in
                // any initialization code unintentionally running.
                #[allow(clippy::borrow_interior_mutable_const)]
                if name == header::CONTENT_LENGTH {
                    // Set the initial buffer capacity using the Content-Length header.
                    self.capacity = value.to_str().ok().and_then(|v| v.parse().ok());
                    if let Some(len) = self.capacity {
                        self.request_context
                            .event_listeners
                            .trigger_content_length(self.request_context(), len);
                    }
                }
                self.headers.insert(name, value);
            }
            Ok(Header::Status(version, code)) => {
                self.version = Some(version);
                self.status = Some(code);
            }
            Ok(Header::EndOfHeaders) => {}
            Err(e) => tracing::error!("{:?}", e),
        }
        true
    }

    fn progress(&mut self, dltotal: f64, dlnow: f64, ultotal: f64, ulnow: f64) -> bool {
        let listeners = &self.request_context.event_listeners;
        if listeners.should_trigger_progress() {
            let progress = Progress::from_curl(dltotal, dlnow, ultotal, ulnow);
            listeners.trigger_progress(self.request_context(), progress);
        }
        true
    }
}

impl HandlerExt for Buffered {
    fn request_context_mut(&mut self) -> &mut RequestContext {
        &mut self.request_context
    }

    fn request_context(&self) -> &RequestContext {
        &self.request_context
    }
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;
    use http::HeaderValue;

    use super::*;
    use crate::progress::ProgressReporter;

    #[test]
    fn test_read() {
        let data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0];

        let mut buf1 = [0xFF; 5];
        let mut buf2 = [0xFF; 3];
        let mut buf3 = [0xFF; 4];

        let mut handler = Buffered::new(RequestContext::dummy().body(data));

        assert_eq!(handler.read(&mut buf1[..]).unwrap(), 5);
        assert_eq!(handler.read(&mut buf2[..]).unwrap(), 3);
        assert_eq!(handler.read(&mut buf3[..]).unwrap(), 2);

        assert_eq!(&buf1[..], &[1, 2, 3, 4, 5][..]);
        assert_eq!(&buf2[..], &[6, 7, 8][..]);
        assert_eq!(&buf3[..], &[9, 0, 0xFF, 0xFF][..]);
    }

    #[test]
    fn test_write() {
        let mut handler = Buffered::new(RequestContext::dummy());

        let expected = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0];

        assert_eq!(handler.write(&expected[..4]).unwrap(), 4);
        assert_eq!(handler.write(&expected[4..]).unwrap(), 6);

        let body = handler.take_body();
        assert_eq!(&expected[..], &*body);

        assert!(handler.take_body().is_empty());
    }

    #[test]
    fn test_seek() {
        let data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0];
        let mut handler = Buffered::new(RequestContext::dummy().body(data));

        assert_matches!(handler.seek(SeekFrom::Start(3)), SeekResult::Ok);
        assert_eq!(handler.bytes_sent, 3);

        assert_matches!(handler.seek(SeekFrom::End(-3)), SeekResult::Ok);
        assert_eq!(handler.bytes_sent, 7);

        assert_matches!(handler.seek(SeekFrom::End(20)), SeekResult::Ok);
        assert_eq!(handler.bytes_sent, 10);

        assert_matches!(handler.seek(SeekFrom::Current(-4)), SeekResult::Ok);
        assert_eq!(handler.bytes_sent, 6);

        assert_matches!(handler.seek(SeekFrom::Current(2)), SeekResult::Ok);
        assert_eq!(handler.bytes_sent, 8);

        assert_matches!(handler.seek(SeekFrom::Current(-20)), SeekResult::Ok);
        assert_eq!(handler.bytes_sent, 0);

        handler.request_context_mut().body = None;
        assert_matches!(handler.seek(SeekFrom::Current(0)), SeekResult::CantSeek);
    }

    #[test]
    fn test_headers() {
        let mut handler = Buffered::new(RequestContext::dummy());

        assert!(handler.header(&b"Content-Length: 1234\r\n"[..]));
        assert!(handler.header(&[0xFF, 0xFF][..])); // Invalid UTF-8 sequence.
        assert!(handler.header(&b"X-No-Value\r\n"[..]));

        let headers = handler.take_headers();

        assert_eq!(
            headers.get("Content-Length").unwrap(),
            HeaderValue::from_static("1234")
        );
        assert_eq!(
            headers.get("X-No-Value").unwrap(),
            HeaderValue::from_static("")
        );
    }

    #[test]
    fn test_capacity() {
        let mut handler = Buffered::new(RequestContext::dummy());
        let _ = handler.write(&[1, 2, 3][..]).unwrap();
        assert_eq!(handler.received.capacity(), DEFAULT_CAPACITY);

        let mut handler = Buffered::new(RequestContext::dummy());

        let _ = handler.header(&b"Content-Length: 42\r\n"[..]);
        assert_eq!(handler.capacity, Some(42));

        let _ = handler.write(&[1, 2, 3][..]).unwrap();
        assert_eq!(handler.received.capacity(), 42);
    }

    #[test]
    fn test_progress() {
        let reporter = ProgressReporter::with_callback(|_| ());

        let mut handler = Buffered::new(RequestContext::dummy());
        handler
            .request_context_mut()
            .event_listeners()
            .on_progress({
                let updater = reporter.updater();
                move |_req, p| updater.update(p)
            });
        let _ = handler.progress(1.0, 2.0, 3.0, 4.0);

        // Note that Progress struct has different argument order.
        let expected = Progress::new(2, 1, 4, 3);
        let progress = reporter.aggregate();
        assert_eq!(progress, expected);
    }
}
