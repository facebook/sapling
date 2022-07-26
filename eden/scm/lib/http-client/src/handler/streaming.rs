/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Read;
use std::io::SeekFrom;

use curl::easy::Handler;
use curl::easy::ReadError;
use curl::easy::SeekResult;
use curl::easy::WriteError;

use super::HandlerExt;
use crate::header::Header;
use crate::progress::Progress;
use crate::receiver::Receiver;
use crate::RequestContext;

pub struct Streaming<R> {
    receiver: Option<R>,
    bytes_sent: usize,
    request_context: RequestContext,
    is_active: bool,
}

impl<R> Streaming<R> {
    pub(crate) fn new(receiver: R, request_context: RequestContext) -> Self {
        Self {
            receiver: Some(receiver),
            bytes_sent: 0,
            request_context,
            is_active: false,
        }
    }

    pub fn take_receiver(&mut self) -> Option<R> {
        self.receiver.take()
    }

    fn trigger_first_activity(&mut self) {
        if !self.is_active {
            self.is_active = true;
            let listeners = &self.request_context.event_listeners;
            listeners.trigger_first_activity(&self.request_context);
        }
    }
}

impl<R: Receiver> Handler for Streaming<R> {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        self.request_context
            .event_listeners
            .trigger_download_bytes(self.request_context(), data.len());
        if let Some(ref mut receiver) = self.receiver {
            if receiver.chunk(data.into()).is_err() {
                // WriteError can only return "Pause", so instead we need to return an incorrect
                // number of bytes written, which will trigger curl to end the request.
                return Ok(0);
            }
        }
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
            Ok(header) => {
                if let Some(ref mut receiver) = self.receiver {
                    if receiver.header(header).is_err() {
                        return false;
                    }
                }
            }
            Err(e) => {
                tracing::error!("{:?}", e);
            }
        }
        true
    }

    fn progress(&mut self, dltotal: f64, dlnow: f64, ultotal: f64, ulnow: f64) -> bool {
        let progress = Progress::from_curl(dltotal, dlnow, ultotal, ulnow);

        if let Some(ref mut receiver) = self.receiver {
            receiver.progress(progress);
        }

        let listeners = &self.request_context.event_listeners;
        if listeners.should_trigger_progress() {
            listeners.trigger_progress(self.request_context(), progress)
        }

        true
    }
}

impl<R: Receiver> HandlerExt for Streaming<R> {
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
    use http::header;
    use http::header::HeaderName;
    use http::header::HeaderValue;

    use super::*;
    use crate::progress::ProgressReporter;
    use crate::receiver::testutil::NullReceiver;
    use crate::receiver::testutil::TestReceiver;

    #[test]
    fn test_read() {
        let data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0];

        let mut buf1 = [0xFF; 5];
        let mut buf2 = [0xFF; 3];
        let mut buf3 = [0xFF; 4];

        let mut handler = Streaming::new(NullReceiver, RequestContext::dummy().body(data));

        assert_eq!(handler.read(&mut buf1[..]).unwrap(), 5);
        assert_eq!(handler.read(&mut buf2[..]).unwrap(), 3);
        assert_eq!(handler.read(&mut buf3[..]).unwrap(), 2);

        assert_eq!(&buf1[..], &[1, 2, 3, 4, 5][..]);
        assert_eq!(&buf2[..], &[6, 7, 8][..]);
        assert_eq!(&buf3[..], &[9, 0, 0xFF, 0xFF][..]);
    }

    #[test]
    fn test_write() {
        let receiver = TestReceiver::new();
        let mut handler = Streaming::new(receiver.clone(), RequestContext::dummy());

        let chunks = vec![vec![1, 2, 3], vec![5, 6], vec![7, 8, 9, 0]];

        assert_eq!(handler.write(&chunks[0]).unwrap(), 3);
        assert_eq!(handler.write(&chunks[1]).unwrap(), 2);
        assert_eq!(handler.write(&chunks[2]).unwrap(), 4);

        assert_eq!(receiver.chunks(), chunks);
    }

    #[test]
    fn test_seek() {
        let data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0];
        let mut handler = Streaming::new(NullReceiver, RequestContext::dummy().body(data));

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
        let receiver = TestReceiver::new();
        let mut handler = Streaming::new(receiver.clone(), RequestContext::dummy());

        assert!(handler.header(&b"Content-Length: 1234\r\n"[..]));
        assert!(handler.header(&[1, 2, 58, 3, 4][..])); // Valid UTF-8 but not alphanumeric.
        assert!(handler.header(&[0xFF, 0xFF][..])); // Invalid UTF-8 sequence.
        assert!(handler.header(&b"X-No-Value\r\n"[..]));

        let expected = vec![
            (header::CONTENT_LENGTH, HeaderValue::from_static("1234")),
            (
                HeaderName::from_static("x-no-value"),
                HeaderValue::from_static(""),
            ),
        ];

        assert_eq!(receiver.headers(), expected);
    }

    #[test]
    fn test_progress() {
        let receiver = TestReceiver::new();
        let mut handler = Streaming::new(receiver.clone(), RequestContext::dummy());

        let reporter = ProgressReporter::with_callback(|_| ());
        handler
            .request_context_mut()
            .event_listeners()
            .on_progress({
                let updater = reporter.updater();
                move |_req, p| updater.update(p)
            });
        let _ = handler.progress(0.0, 0.0, 0.0, 0.0);
        let _ = handler.progress(1.0, 2.0, 3.0, 4.0);

        // Note that Progress struct has different argument order.
        let expected = Progress::new(2, 1, 4, 3);

        // Check that the TestReceiver got the value.
        assert_eq!(receiver.progress().unwrap(), expected);

        // Check that ProgressReporter also got the value.
        assert_eq!(reporter.aggregate(), expected);
    }
}
