// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io::{self, Cursor, Write};

use bzip2;
use quickcheck::TestResult;
use futures::{Async, Poll};
use tokio_core::reactor::Core;
use tokio_io::AsyncWrite;
use tokio_io::io::read_to_end;

use retry::retry_write;

use membuf::MemBuf;
use metered::{MeteredRead, MeteredWrite};
use compressor::{Compressor, CompressorType};
use decompressor::Decompressor;
use ZSTD_DEFAULT_LEVEL;

quickcheck! {
    fn test_bzip2_roundtrip(input: Vec<u8>) -> TestResult {
        roundtrip(CompressorType::Bzip2(bzip2::Compression::Default), &input)
    }

    fn test_noop_roundtrip(input: Vec<u8>) -> TestResult {
        roundtrip(CompressorType::Uncompressed, &input)
    }

    fn test_zstd_roundtrip(input: Vec<u8>) -> TestResult {
        roundtrip(CompressorType::Zstd { level: ZSTD_DEFAULT_LEVEL }, &input)
    }
}

fn roundtrip(ct: CompressorType, input: &[u8]) -> TestResult {
    let compressed_buf = MeteredWrite::new(Cursor::new(Vec::with_capacity(32 * 1024)));
    let mut compressor = MeteredWrite::new(Compressor::new(compressed_buf, ct));
    let res = compressor.write_all(input);
    assert_matches!(res, Ok(()));
    assert_eq!(compressor.total_thru(), input.len() as u64);

    let compressed_buf = compressor.into_inner().try_finish().unwrap();
    assert_eq!(
        compressed_buf.total_thru(),
        compressed_buf.get_ref().position()
    );
    // Turn the MeteredWrite<Cursor> into a Cursor
    let compressed_buf = compressed_buf.into_inner();

    let read_buf = MeteredRead::new(MemBuf::new(32 * 1024));
    let mut decoder = MeteredRead::new(Decompressor::new(read_buf, ct.decompressor_type()));

    assert_matches!(decoder.get_mut().get_mut().get_mut().write_buf(compressed_buf.get_ref()),
                    Ok(l) if l as u64 == compressed_buf.position());
    decoder.get_mut().get_mut().get_mut().mark_eof();

    let result = Vec::with_capacity(32 * 1024);
    let read_future = read_to_end(decoder, result);

    let mut core = Core::new().unwrap();
    let (decoder, result) = core.run(read_future).unwrap();
    assert_eq!(decoder.total_thru(), input.len() as u64);
    assert_eq!(
        decoder.get_ref().get_ref().total_thru(),
        compressed_buf.position()
    );
    assert_eq!(input, result.as_slice());
    TestResult::passed()
}

struct FinishAfterCountTestWriter {
    counter: u8,
    fail_with: Option<io::ErrorKind>,
}

impl FinishAfterCountTestWriter {
    pub fn new_ok() -> FinishAfterCountTestWriter {
        FinishAfterCountTestWriter {
            counter: 0,
            fail_with: None,
        }
    }
    pub fn new_failed(error_kind: io::ErrorKind) -> FinishAfterCountTestWriter {
        FinishAfterCountTestWriter {
            counter: 0,
            fail_with: Some(error_kind),
        }
    }
}

impl Write for FinishAfterCountTestWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        if self.counter == 3 {
            self.fail_with.map_or(Ok(42), |k| Err(io::Error::from(k)))
        } else {
            self.counter += 1;
            Err(io::Error::from(io::ErrorKind::Interrupted))
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl AsyncWrite for FinishAfterCountTestWriter {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        Ok(Async::NotReady)
    }
}

#[test]
fn test_ok_after_retry() {
    let mut writer = FinishAfterCountTestWriter::new_ok();
    let buffer = [0; 10];
    let status = retry_write(&mut writer, &buffer).expect("should succeed");
    assert_eq!(status, 42);
}

#[test]
fn test_fail_after_retry() {
    let mut writer = FinishAfterCountTestWriter::new_failed(io::ErrorKind::NotFound);
    let buffer = [0; 10];
    let status = retry_write(&mut writer, &buffer);
    assert_eq!(status.unwrap_err().kind(), io::ErrorKind::NotFound);
}
