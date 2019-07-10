// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io::{self, BufReader, Cursor, Read, Write};

use assert_matches::assert_matches;
use bzip2;
use flate2;
use futures::{Async, Poll};
use quickcheck::{quickcheck, Arbitrary, Gen, TestResult};
use tokio;
use tokio_io::io::read_to_end;
use tokio_io::AsyncWrite;

use crate::retry::retry_write;

use crate::compressor::{Compressor, CompressorType};
use crate::decompressor::Decompressor;
use crate::membuf::MemBuf;
use crate::metered::{MeteredRead, MeteredWrite};

quickcheck! {
    fn test_bzip2_roundtrip(cmprs: BzipCompression, input: Vec<u8>) -> TestResult {
        roundtrip(CompressorType::Bzip2(cmprs.0), &input)
    }

    fn test_gzip_roundtrip(cmprs: GzipCompression, input: Vec<u8>) -> TestResult {
        roundtrip(CompressorType::Gzip(cmprs.0), &input)
    }

    fn test_zstd_roundtrip(input: Vec<u8>) -> TestResult {
        roundtrip(CompressorType::Zstd { level: 0 }, &input)
    }

    fn test_bzip_overreading(
        cmprs: BzipCompression,
        compressable_input: Vec<u8>,
        extra_input: Vec<u8>
    ) -> TestResult {
        check_overreading(
            CompressorType::Bzip2(cmprs.0),
            compressable_input.as_slice(),
            extra_input.as_slice(),
        )
    }

    fn test_gzip_overreading(
        cmprs: GzipCompression,
        compressable_input: Vec<u8>,
        extra_input: Vec<u8>
    ) -> TestResult {
        check_overreading(
            CompressorType::Gzip(cmprs.0),
            compressable_input.as_slice(),
            extra_input.as_slice(),
        )
    }
}

#[derive(Debug, Clone)]
struct BzipCompression(bzip2::Compression);
impl Arbitrary for BzipCompression {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        BzipCompression(
            g.choose(&[
                bzip2::Compression::Fastest,
                bzip2::Compression::Best,
                bzip2::Compression::Default,
            ])
            .unwrap()
            .clone(),
        )
    }
}

#[derive(Debug, Clone)]
struct GzipCompression(flate2::Compression);
impl Arbitrary for GzipCompression {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        GzipCompression(
            g.choose(&[
                flate2::Compression::none(),
                flate2::Compression::fast(),
                flate2::Compression::best(),
            ])
            .unwrap()
            .clone(),
        )
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

    let decoder = {
        let mut read_buf = BufReader::new(MeteredRead::new(MemBuf::new(32 * 1024)));

        assert_matches!(read_buf.get_mut().get_mut().write_buf(compressed_buf.get_ref()),
                        Ok(l) if l as u64 == compressed_buf.position());
        read_buf.get_mut().get_mut().mark_eof();

        MeteredRead::new(Decompressor::new(read_buf, ct.decompressor_type()))
    };

    let result = Vec::with_capacity(32 * 1024);
    let read_future = read_to_end(decoder, result);

    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    let (decoder, result) = runtime.block_on(read_future).unwrap();
    assert_eq!(decoder.total_thru(), input.len() as u64);
    assert_eq!(
        decoder.get_ref().get_ref().get_ref().total_thru(),
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

fn check_overreading(c_type: CompressorType, data: &[u8], extra: &[u8]) -> TestResult {
    const EXTRA_SPACE: usize = 8;

    let mut decompressor = {
        let mut compressor = Compressor::new(Cursor::new(Vec::new()), c_type);
        compressor.write_all(data).unwrap();
        compressor.flush().unwrap();
        let mut compressed = compressor.try_finish().unwrap().into_inner();

        for u in extra {
            compressed.push(*u);
        }

        Decompressor::new(
            BufReader::new(Cursor::new(compressed)),
            c_type.decompressor_type(),
        )
    };

    {
        let mut buf = vec![0u8; data.len() + extra.len() + EXTRA_SPACE];
        let mut expected = Vec::new();
        expected.extend_from_slice(data);
        expected.extend_from_slice(vec![0u8; extra.len() + EXTRA_SPACE].as_slice());

        if !(decompressor.read(buf.as_mut_slice()).unwrap() == data.len() && buf == expected) {
            return TestResult::error(format!("decoding failed, buf: {:?}", buf));
        }
    }

    {
        let mut buf = vec![0u8; data.len() + extra.len() + EXTRA_SPACE];

        if !(decompressor.read(buf.as_mut_slice()).unwrap() == 0 && buf == vec![0u8; buf.len()]) {
            return TestResult::error(format!("detecting eof failed, buf: {:?}", buf));
        }
    }

    {
        let mut buf = Vec::new();

        let mut remainder = decompressor.into_inner();
        if !(remainder.read_to_end(&mut buf).unwrap() == extra.len() && buf.as_slice() == extra) {
            return TestResult::error(format!("leaving remainder failed, buf: {:?}", buf));
        }
    }

    TestResult::passed()
}
