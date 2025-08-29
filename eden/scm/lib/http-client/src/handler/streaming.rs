/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Read;
use std::io::SeekFrom;

use curl::easy::Handler;
use curl::easy::ReadError;
use curl::easy::SeekResult;
use curl::easy::WriteError;

use super::HandlerExt;
use crate::RequestContext;
use crate::claimer::RequestClaim;
use crate::header::Header;
use crate::progress::Progress;
use crate::receiver::Receiver;

pub struct Streaming {
    receiver: Option<Box<dyn Receiver>>,
    bytes_sent: usize,
    request_context: RequestContext,
    is_active: bool,
    claim: RequestClaim,
}

impl Streaming {
    pub(crate) fn new(
        receiver: Box<dyn Receiver>,
        request_context: RequestContext,
        claim: RequestClaim,
    ) -> Self {
        Self {
            receiver: Some(receiver),
            bytes_sent: 0,
            request_context,
            is_active: false,
            claim,
        }
    }

    pub fn take_receiver(&mut self) -> Option<Box<dyn Receiver>> {
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

impl Handler for Streaming {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        self.request_context
            .event_listeners
            .trigger_download_bytes(self.request_context(), data.len());

        match self.receiver {
            Some(ref mut receiver) => {
                match receiver.chunk(data.into()) {
                    // Normal case - receiver handled all the bytes.
                    Ok(false) => Ok(data.len()),
                    // Receiver wants us to pause the transfer.
                    Ok(true) => {
                        tracing::trace!("receiver.chunk() wants to pause");
                        tracing::trace!(target: "curl_pause", "pausing write");
                        Err(WriteError::Pause)
                    }
                    // WriteError can only return "Pause", so instead we need to return an incorrect
                    // number of bytes written, which will trigger curl to end the request.
                    Err(err) => {
                        tracing::trace!(?err, "receiver.chunk() return error");
                        Ok(0)
                    }
                }
            }
            // No receiver - indicate everything is okay (why??)
            None => Ok(data.len()),
        }
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
                    if let Err(err) = receiver.header(header) {
                        // Don't propagate error as `false` since that will error out the entire request.
                        // One case the receiver goes away is during HTTP 1.1 -> 2 upgrade
                        // (probably related to the initial request getting dropped
                        // earlier than we expect, but not totally sure).
                        tracing::warn!(?err, "error sending header to receiver");
                    }
                }
            }
            Err(e) => {
                tracing::error!(err=?e, "error parsing header");
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

impl HandlerExt for Streaming {
    fn request_context_mut(&mut self) -> &mut RequestContext {
        &mut self.request_context
    }

    fn request_context(&self) -> &RequestContext {
        &self.request_context
    }

    fn take_receiver(&mut self) -> Option<Box<dyn Receiver>> {
        Streaming::take_receiver(self)
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn needs_unpause(&mut self) -> bool {
        self.receiver.as_mut().is_some_and(|r| r.needs_unpause())
    }

    fn is_paused(&self) -> bool {
        self.receiver.as_ref().is_some_and(|r| r.is_paused())
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

        let mut handler = Streaming::new(
            Box::new(NullReceiver),
            RequestContext::dummy().body(data),
            RequestClaim::default(),
        );

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
        let mut handler = Streaming::new(
            Box::new(receiver.clone()),
            RequestContext::dummy(),
            RequestClaim::default(),
        );

        let chunks = vec![vec![1, 2, 3], vec![5, 6], vec![7, 8, 9, 0]];

        assert_eq!(handler.write(&chunks[0]).unwrap(), 3);
        assert_eq!(handler.write(&chunks[1]).unwrap(), 2);
        assert_eq!(handler.write(&chunks[2]).unwrap(), 4);

        assert_eq!(receiver.chunks(), chunks);
    }

    #[test]
    fn test_seek() {
        let data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0];
        let mut handler = Streaming::new(
            Box::new(NullReceiver),
            RequestContext::dummy().body(data),
            RequestClaim::default(),
        );

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
        let mut handler = Streaming::new(
            Box::new(receiver.clone()),
            RequestContext::dummy(),
            RequestClaim::default(),
        );

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
        let mut handler = Streaming::new(
            Box::new(receiver.clone()),
            RequestContext::dummy(),
            RequestClaim::default(),
        );

        let reporter = ProgressReporter::default();
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
