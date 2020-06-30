/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    io::Read,
    str::{self, Utf8Error},
};

use curl::easy::{Handler, ReadError, WriteError};
use once_cell::unsync::OnceCell;

use crate::progress::{MonitorProgress, Progress, ProgressUpdater};

/// Initial buffer capacity to allocate if we don't get a Content-Length header.
/// Usually, the lack of a Content-Length header indicates a streaming response,
/// in which case the body size is expected to be relatively large.
const DEFAULT_CAPACITY: usize = 1000;

/// A simple curl Handler that buffers all received data.
#[derive(Default)]
pub struct Buffered {
    received: OnceCell<Vec<u8>>,
    capacity: Option<usize>,
    headers: Vec<(String, String)>,
    payload: Option<Vec<u8>>,
    bytes_sent: usize,
    updater: Option<ProgressUpdater>,
}

impl Buffered {
    pub(crate) fn new() -> Self {
        Default::default()
    }

    /// Specify a payload that will be uploaded as the *request* body.
    pub(crate) fn with_payload(payload: Option<Vec<u8>>) -> Self {
        Self {
            payload,
            ..Self::new()
        }
    }

    /// Access the received headers.
    pub(crate) fn headers(&mut self) -> &[(String, String)] {
        &self.headers
    }

    /// Extract the received data.
    pub(crate) fn take_data(&mut self) -> Vec<u8> {
        self.received.take().unwrap_or_default()
    }
}

impl Handler for Buffered {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        let _ = self
            .received
            .get_or_init(|| Vec::with_capacity(self.capacity.unwrap_or(DEFAULT_CAPACITY)));

        // XXX: There is no method that both initializes the cell and returns
        // a mutable reference, so we ignore the initial reference and call
        // `get_mut()`, which can't panic because we've already initialized.
        self.received.get_mut().unwrap().extend_from_slice(data);
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
        let (name, value) = match split_header(data) {
            Ok((name, value)) => {
                log::trace!("Received header: {}: {}", name, value);
                (name, value)
            }
            Err(e) => {
                // Drop invalid headers.
                let i = e.valid_up_to();
                log::trace!(
                    "Dropping non-UTF-8 header: Valid prefix: {:?}; Invalid bytes: {:x?}",
                    str::from_utf8(&data[..i]).unwrap(),
                    &data[i..],
                );
                return true;
            }
        };

        // Record content-length to set initial buffer size.
        // Use case-insensitive comparison since HTTP/2
        // requires headers to be lowercase.
        if name.eq_ignore_ascii_case("content-length") {
            self.capacity = value.parse().ok();
        }

        self.headers.push((name.into(), value.into()));
        true
    }

    fn progress(&mut self, dltotal: f64, dlnow: f64, ultotal: f64, ulnow: f64) -> bool {
        if let Some(ref updater) = self.updater {
            updater.update(Progress::from_curl(dltotal, dlnow, ultotal, ulnow));
        }
        true
    }
}

impl MonitorProgress for Buffered {
    fn monitor_progress(&mut self, updater: ProgressUpdater) {
        self.updater = Some(updater);
    }
}

fn split_header(header: &[u8]) -> Result<(&str, &str), Utf8Error> {
    let header = str::from_utf8(header)?.splitn(2, ':').collect::<Vec<_>>();
    Ok(if header.len() > 1 {
        (header[0], header[1].trim())
    } else {
        (header[0].trim(), "")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::progress::ProgressReporter;

    #[test]
    fn test_read() {
        let data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0];

        let mut buf1 = [0xFF; 5];
        let mut buf2 = [0xFF; 3];
        let mut buf3 = [0xFF; 4];

        let mut handler = Buffered::with_payload(Some(data.into()));

        assert_eq!(handler.read(&mut buf1[..]).unwrap(), 5);
        assert_eq!(handler.read(&mut buf2[..]).unwrap(), 3);
        assert_eq!(handler.read(&mut buf3[..]).unwrap(), 2);

        assert_eq!(&buf1[..], &[1, 2, 3, 4, 5][..]);
        assert_eq!(&buf2[..], &[6, 7, 8][..]);
        assert_eq!(&buf3[..], &[9, 0, 0xFF, 0xFF][..]);
    }

    #[test]
    fn test_write() {
        let mut handler = Buffered::new();

        let expected = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0];

        assert_eq!(handler.write(&expected[..4]).unwrap(), 4);
        assert_eq!(handler.write(&expected[4..]).unwrap(), 6);

        let data = handler.take_data();
        assert_eq!(&expected[..], &*data);

        assert!(handler.take_data().is_empty());
    }

    #[test]
    fn test_headers() {
        let mut handler = Buffered::new();

        assert!(handler.header(&b"Content-Length: 1234\r\n"[..]));
        assert!(handler.header(&[1, 2, 58, 3, 4][..])); // Byte 58 is ASCII colon.
        assert!(handler.header(&[0xFF, 0xFF][..])); // Invalid UTF-8 sequence.
        assert!(handler.header(&b"X-No-Value\r\n"[..]));

        let headers = handler.headers();
        let expected = [
            ("Content-Length".into(), "1234".into()),
            (
                String::from_utf8(vec![1, 2]).unwrap(),
                String::from_utf8(vec![3, 4]).unwrap(),
            ),
            ("X-No-Value".into(), "".into()),
        ];

        assert_eq!(expected, headers);
    }

    #[test]
    fn test_capacity() {
        let mut handler = Buffered::new();
        let _ = handler.write(&[1, 2, 3][..]).unwrap();
        assert_eq!(handler.received.get().unwrap().capacity(), DEFAULT_CAPACITY);

        let mut handler = Buffered::new();

        let _ = handler.header(&b"Content-Length: 42\r\n"[..]);
        assert_eq!(handler.capacity, Some(42));

        let _ = handler.write(&[1, 2, 3][..]).unwrap();
        assert_eq!(handler.received.get().unwrap().capacity(), 42);
    }

    #[test]
    fn test_progress() {
        let reporter = ProgressReporter::with_callback(|_| ());

        let mut handler = Buffered::new();
        handler.monitor_progress(reporter.updater());
        let _ = handler.progress(1.0, 2.0, 3.0, 4.0);

        // Note that Progress struct has different argument order.
        let expected = Progress::new(2, 1, 4, 3);
        let progress = reporter.aggregate();
        assert_eq!(progress, expected);
    }
}
