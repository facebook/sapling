/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Read;

use curl::easy::{Handler, ReadError, WriteError};

use crate::header::Header;
use crate::progress::{Progress, ProgressUpdater};
use crate::receiver::Receiver;

use super::Configure;

pub struct Streaming<R> {
    receiver: Option<R>,
    payload: Option<Vec<u8>>,
    bytes_sent: usize,
    updater: Option<ProgressUpdater>,
}

impl<R> Streaming<R> {
    pub fn with_receiver(receiver: R) -> Self {
        Self {
            receiver: Some(receiver),
            payload: None,
            bytes_sent: 0,
            updater: None,
        }
    }

    pub fn take_receiver(&mut self) -> Option<R> {
        self.receiver.take()
    }
}

impl<R: Receiver> Handler for Streaming<R> {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        if let Some(ref mut receiver) = self.receiver {
            receiver.chunk(data.into());
        }
        Ok(data.len())
    }

    fn read(&mut self, data: &mut [u8]) -> Result<usize, ReadError> {
        Ok(if let Some(payload) = self.payload.as_mut() {
            let sent = (&payload[self.bytes_sent..])
                .read(data)
                .expect("Failed to read from payload buffer");
            self.bytes_sent += sent;
            sent
        } else {
            0
        })
    }

    fn header(&mut self, data: &[u8]) -> bool {
        match Header::parse(data) {
            Ok(header) => {
                if let Some(ref mut receiver) = self.receiver {
                    receiver.header(header);
                }
            }
            Err(e) => {
                log::trace!("{}", e);
            }
        }
        true
    }

    fn progress(&mut self, dltotal: f64, dlnow: f64, ultotal: f64, ulnow: f64) -> bool {
        let progress = Progress::from_curl(dltotal, dlnow, ultotal, ulnow);

        if let Some(ref mut receiver) = self.receiver {
            receiver.progress(progress);
        }

        if let Some(ref updater) = self.updater {
            updater.update(progress);
        }

        true
    }
}

impl<R: Receiver> Configure for Streaming<R> {
    fn with_payload(self, payload: Option<Vec<u8>>) -> Self {
        Self { payload, ..self }
    }

    fn monitor_progress(&mut self, updater: ProgressUpdater) {
        self.updater = Some(updater);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use http::header::{self, HeaderName, HeaderValue};

    use crate::progress::ProgressReporter;
    use crate::receiver::testutil::{NullReceiver, TestReceiver};

    #[test]
    fn test_read() {
        let data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0];

        let mut buf1 = [0xFF; 5];
        let mut buf2 = [0xFF; 3];
        let mut buf3 = [0xFF; 4];

        let mut handler = Streaming::with_receiver(NullReceiver).with_payload(Some(data.into()));

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
        let mut handler = Streaming::with_receiver(receiver.clone());

        let chunks = vec![vec![1, 2, 3], vec![5, 6], vec![7, 8, 9, 0]];

        assert_eq!(handler.write(&chunks[0]).unwrap(), 3);
        assert_eq!(handler.write(&chunks[1]).unwrap(), 2);
        assert_eq!(handler.write(&chunks[2]).unwrap(), 4);

        assert_eq!(receiver.chunks(), chunks);
    }

    #[test]
    fn test_headers() {
        let receiver = TestReceiver::new();
        let mut handler = Streaming::with_receiver(receiver.clone());

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
        let mut handler = Streaming::with_receiver(receiver.clone());

        let reporter = ProgressReporter::with_callback(|_| ());

        handler.monitor_progress(reporter.updater());
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
