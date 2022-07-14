/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;
use futures::sync::mpsc;
use futures_ext::BoxStream;
use metadata::Metadata;
use std::io;
use std::sync::Arc;
use tokio_util::codec::Decoder;
use tokio_util::codec::Encoder;
use zstd::stream::raw::Encoder as ZstdEncoder;
use zstd::stream::raw::InBuffer;
use zstd::stream::raw::Operation;
use zstd::stream::raw::OutBuffer;

use netstring::NetstringDecoder;
use netstring::NetstringEncoder;

// Multiplex stdin/out/err over a single stream using netstring as framing
#[derive(Debug)]
pub struct SshDecoder(NetstringDecoder);

pub struct SshEncoder {
    netstring: NetstringEncoder<Bytes>,
    compressor: Option<ZstdEncoder<'static>>,
}

pub struct Stdio {
    pub metadata: Arc<Metadata>,
    pub stdin: BoxStream<Bytes, io::Error>,
    pub stdout: mpsc::Sender<Bytes>,
    pub stderr: mpsc::UnboundedSender<Bytes>,
}

pub struct SenderBytesWrite {
    pub chan: mpsc::UnboundedSender<Bytes>,
}

impl io::Write for SenderBytesWrite {
    fn flush(&mut self) -> io::Result<()> {
        // Nothing to do. We don't control what happens in our receiver.
        Ok(())
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.chan
            .unbounded_send(Bytes::copy_from_slice(buf))
            .map(|_| buf.len())
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e))
    }
}

// Matches Iostream in Mercurial mononokepeer.py
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IoStream {
    Stdin,
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SshMsg(IoStream, Bytes);

impl SshMsg {
    pub fn new(stream: IoStream, data: Bytes) -> Self {
        SshMsg(stream, data)
    }

    pub fn from_slice<T>(stream: IoStream, t: T) -> Self
    where
        T: AsRef<[u8]>,
    {
        Self::new(stream, Bytes::copy_from_slice(t.as_ref()))
    }

    pub fn stream(&self) -> IoStream {
        self.0.clone()
    }

    pub fn stream_ref(&self) -> &IoStream {
        &self.0
    }

    pub fn data(self) -> Bytes {
        self.1
    }
}

impl AsRef<[u8]> for SshMsg {
    fn as_ref(&self) -> &[u8] {
        self.1.as_ref()
    }
}

impl SshDecoder {
    pub fn new() -> Self {
        SshDecoder(NetstringDecoder::default())
    }
}

fn ioerr_cvt(err: anyhow::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("{}", err))
}

impl Decoder for SshDecoder {
    type Item = SshMsg;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> io::Result<Option<SshMsg>> {
        if let Some(mut data) = self.0.decode(buf).map_err(ioerr_cvt)? {
            if data.is_empty() {
                return Ok(None);
            }
            match data.split_to(1)[0] {
                0 => Ok(Some(SshMsg(IoStream::Stdin, data.freeze()))),
                1 => Ok(Some(SshMsg(IoStream::Stdout, data.freeze()))),
                2 => Ok(Some(SshMsg(IoStream::Stderr, data.freeze()))),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "bad ssh stream",
                )),
            }
        } else {
            Ok(None)
        }
    }
}

impl SshEncoder {
    pub fn new(compression_level: Option<i32>) -> Result<Self> {
        match compression_level {
            Some(level) => Ok(SshEncoder {
                netstring: NetstringEncoder::default(),
                compressor: Some(ZstdEncoder::new(level)?),
            }),
            _ => Ok(SshEncoder {
                netstring: NetstringEncoder::default(),
                compressor: None,
            }),
        }
    }

    fn compress_into<'a>(&mut self, out: &mut BytesMut, input: &'a [u8]) -> Result<()> {
        match &mut self.compressor {
            Some(compressor) => {
                let buflen = zstd_safe::compress_bound(input.len());
                if buflen >= zstd_safe::dstream_out_size() {
                    return Err(anyhow!(
                        "block is too big to compress in to a single zstd block"
                    ));
                }

                let mut src = InBuffer::around(input);
                let mut dst = vec![0u8; buflen];
                let mut dst = OutBuffer::around(&mut dst);

                while src.pos < src.src.len() {
                    compressor.run(&mut src, &mut dst)?;
                }
                loop {
                    let remaining = compressor.flush(&mut dst)?;
                    if remaining == 0 {
                        break;
                    }
                }
                out.put_slice(dst.as_slice());
            }
            None => out.put_slice(input),
        };

        Ok(())
    }
}

impl Encoder<SshMsg> for SshEncoder {
    type Error = io::Error;

    fn encode(&mut self, msg: SshMsg, buf: &mut BytesMut) -> io::Result<()> {
        let mut v = BytesMut::with_capacity(1 + msg.1.len());

        match msg.0 {
            IoStream::Stdin => {
                v.put_u8(0);
                self.compress_into(&mut v, &msg.1).map_err(ioerr_cvt)?;
                Ok(self.netstring.encode(v.freeze(), buf).map_err(ioerr_cvt)?)
            }
            IoStream::Stdout => {
                v.put_u8(1);
                self.compress_into(&mut v, &msg.1).map_err(ioerr_cvt)?;
                Ok(self.netstring.encode(v.freeze(), buf).map_err(ioerr_cvt)?)
            }
            IoStream::Stderr => {
                v.put_u8(2);
                self.compress_into(&mut v, &msg.1).map_err(ioerr_cvt)?;
                Ok(self.netstring.encode(v.freeze(), buf).map_err(ioerr_cvt)?)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use bytes::BufMut;
    use bytes::BytesMut;
    use tokio_util::codec::Decoder;
    use tokio_util::codec::Encoder;

    use super::IoStream::*;
    use super::*;

    trait ToBytes: AsRef<[u8]> {
        fn bytes(&self) -> Bytes {
            Bytes::copy_from_slice(self.as_ref())
        }
    }

    impl<T> ToBytes for T where T: AsRef<[u8]> {}

    #[test]
    fn encode_simple() {
        let mut buf = BytesMut::with_capacity(1024);
        let mut encoder = SshEncoder::new(None).unwrap();

        encoder
            .encode(SshMsg::new(Stdin, b"ls -l".bytes()), &mut buf)
            .expect("encode failed");

        assert_eq!(buf.as_ref(), b"6:\x00ls -l,");
    }

    #[test]
    fn encode_zero() {
        let mut buf = BytesMut::with_capacity(1024);
        let mut encoder = SshEncoder::new(None).unwrap();

        encoder
            .encode(SshMsg::new(Stdin, b"".bytes()), &mut buf)
            .expect("encode failed");

        assert_eq!(buf.as_ref(), b"1:\x00,");
    }

    #[test]
    fn encode_one() {
        let mut buf = BytesMut::with_capacity(1024);
        let mut encoder = SshEncoder::new(None).unwrap();

        encoder
            .encode(SshMsg::new(Stdin, b"X".bytes()), &mut buf)
            .expect("encode failed");

        assert_eq!(buf.as_ref(), b"2:\x00X,");
    }

    #[test]
    fn encode_multi() {
        let mut buf = BytesMut::with_capacity(1024);
        let mut encoder = SshEncoder::new(None).unwrap();

        encoder
            .encode(SshMsg::new(Stdin, b"X".bytes()), &mut buf)
            .expect("encode failed");
        encoder
            .encode(SshMsg::new(Stdout, b"Y".bytes()), &mut buf)
            .expect("encode failed");
        encoder
            .encode(SshMsg::new(Stderr, b"Z".bytes()), &mut buf)
            .expect("encode failed");

        assert_eq!(buf.as_ref(), b"2:\x00X,2:\x01Y,2:\x02Z,");
    }

    #[test]
    fn encode_compressed() {
        let mut buf = BytesMut::with_capacity(1024);
        let mut encoder = SshEncoder::new(Some(3)).unwrap();

        encoder
            .encode(
                SshMsg::new(
                    Stdin,
                    b"hello hello hello hello hello hello hello hello hello".bytes(),
                ),
                &mut buf,
            )
            .expect("encode failed");
        assert_eq!(buf.as_ref(), b"22:\x00\x28\xb5\x2f\xfd\x00\x58\x64\x00\x00\x30\x68\x65\x6c\x6c\x6f\x20\x01\x00\x24\x2a\x45\x2c");
    }

    #[test]
    fn encode_compressed_too_big() {
        let mut buf = BytesMut::with_capacity(1024);
        let mut encoder = SshEncoder::new(Some(3)).unwrap();

        // 1MB, which is larger then 128KB zstd streaming buffer
        let message = vec![0u8; 1048576];
        let result = encoder.encode(SshMsg::new(Stdin, message.as_slice().bytes()), &mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn decode_simple() {
        let mut buf = BytesMut::with_capacity(1024);
        buf.put_slice(b"6:\x00ls -l,");

        let mut decoder = SshDecoder::new();

        match decoder.decode(&mut buf) {
            Ok(Some(ref res)) if res == &SshMsg::new(Stdin, b"ls -l".bytes()) => {}
            bad => panic!("decode failed: {:?}", bad.as_ref()),
        }
    }

    #[test]
    fn decode_zero() {
        let mut buf = BytesMut::with_capacity(1024);
        buf.put_slice(b"1:\x00,");

        let mut decoder = SshDecoder::new();

        match decoder.decode(&mut buf) {
            Ok(Some(ref res)) if res == &SshMsg::new(Stdin, b"".bytes()) => {}
            bad => panic!("decode failed: {:?}", bad.as_ref()),
        }
    }

    #[test]
    fn decode_one() {
        let mut buf = BytesMut::with_capacity(1024);
        buf.put_slice(b"2:\x00X,");

        let mut decoder = SshDecoder::new();

        match decoder.decode(&mut buf) {
            Ok(Some(ref res)) if res == &SshMsg::new(Stdin, b"X".bytes()) => {}
            bad => panic!("decode failed: {:?}", bad.as_ref()),
        }
    }

    #[test]
    fn decode_multi() {
        let mut buf = BytesMut::with_capacity(1024);
        buf.put_slice(b"2:\x00X,2:\x01Y,2:\x02Z,");

        let mut decoder = SshDecoder::new();

        match decoder.decode(&mut buf) {
            Ok(Some(ref res)) if res == &SshMsg::new(Stdin, b"X".bytes()) => {}
            bad => panic!("decode failed: {:?}", bad.as_ref()),
        }
        match decoder.decode(&mut buf) {
            Ok(Some(ref res)) if res == &SshMsg::new(Stdout, b"Y".bytes()) => {}
            bad => panic!("decode failed: {:?}", bad.as_ref()),
        }
        match decoder.decode(&mut buf) {
            Ok(Some(ref res)) if res == &SshMsg::new(Stderr, b"Z".bytes()) => {}
            bad => panic!("decode failed: {:?}", bad.as_ref()),
        }
    }

    #[test]
    fn decode_bad() {
        let mut buf = BytesMut::with_capacity(1024);
        buf.put_slice(b"2:\x03X,");

        let mut decoder = SshDecoder::new();

        match decoder.decode(&mut buf) {
            Ok(bad) => panic!("unexpected success: {:?}", bad),
            Err(_err) => {}
        }
    }

    #[test]
    fn decode_short_framing() {
        let mut buf = BytesMut::with_capacity(1024);
        buf.put_slice(b"3:\x02X,");

        let mut decoder = SshDecoder::new();

        match decoder.decode(&mut buf) {
            Ok(None) => {}
            bad => panic!("bad framing: {:?}", bad),
        }
    }

    #[test]
    fn decode_broken_framing() {
        let mut buf = BytesMut::with_capacity(1024);
        buf.put_slice(b"1:\x02X,");

        let mut decoder = SshDecoder::new();

        match decoder.decode(&mut buf) {
            Ok(bad) => panic!("unexpected success: {:?}", bad),
            Err(_err) => {}
        }
    }
}
