/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(result_flattening)]
mod priority;

use anyhow::{anyhow, Error, Result};
use std::io;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::{BufMut, Bytes, BytesMut};
use clientinfo::ClientInfo;
use futures::sync::mpsc;
use futures_ext::BoxStream;
use permission_checker::{MononokeIdentitySet, MononokeIdentitySetExt};
use session_id::{generate_session_id, SessionId};
use tokio::time::timeout;
use tokio_util::codec::{Decoder, Encoder};
use trust_dns_resolver::TokioAsyncResolver;
use zstd::stream::raw::{Encoder as ZstdEncoder, InBuffer, Operation, OutBuffer};

use netstring::{NetstringDecoder, NetstringEncoder};

pub use priority::Priority;

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

#[derive(Clone, Debug, Default)]
pub struct Metadata {
    session_id: SessionId,
    is_trusted_client: bool,
    identities: MononokeIdentitySet,
    priority: Priority,
    client_debug: bool,
    client_ip: Option<IpAddr>,
    client_hostname: Option<String>,
    revproxy_region: Option<String>,
    raw_encoded_cats: Option<String>,
    client_info: Option<ClientInfo>,
}

impl Metadata {
    pub async fn new(
        session_id: Option<&String>,
        is_trusted_client: bool,
        identities: MononokeIdentitySet,
        priority: Priority,
        client_debug: bool,
        client_ip: IpAddr,
    ) -> Self {
        let session_id: SessionId = match session_id {
            Some(id) => SessionId::from_string(id.to_owned()),
            None => generate_session_id(),
        };

        // Hostname of the client is for non-critical use only. We're doing best-effort lookup here:
        // 1) We're extracting it from identities (which requires no remote calls)
        let client_hostname = if let Some(client_hostname) = identities.hostname() {
            Some(client_hostname.to_string())
        }
        // 2) If it's not there we're trying to look it up via reverse dns with timeout of 1s.
        else {
            timeout(Duration::from_secs(1), Metadata::reverse_lookup(client_ip))
                .await
                .map_err(Error::from)
                .flatten()
                .ok()
        };

        Self {
            session_id,
            is_trusted_client,
            identities,
            priority,
            client_debug,
            client_ip: Some(client_ip),
            client_hostname,
            revproxy_region: None,
            raw_encoded_cats: None,
            client_info: None,
        }
    }

    // Reverse lookups an IP to associated hostname. Trailing dots are stripped
    // to remain compatible with historical logging and common usage of reverse
    // hostnames in other logs (even though trailing dot is technically more correct)
    async fn reverse_lookup(client_ip: IpAddr) -> Result<String> {
        // This parses /etc/resolv.conf on each request. Given that this should be in
        // the page cache and the parsing of the text is very minimal, this shouldn't
        // impact performance much. In case this does lead to performance issues we
        // could start caching this, which for now would be preferred to avoid as this
        // might lead to unexpected behavior if the system configuration changes.
        let resolver = TokioAsyncResolver::tokio_from_system_conf()?;
        resolver
            .reverse_lookup(client_ip)
            .await?
            .iter()
            .next()
            .map(|name| name.to_string().trim_end_matches('.').to_string())
            .ok_or_else(|| anyhow!("failed to do reverse lookup"))
    }

    pub fn add_raw_encoded_cats(&mut self, raw_encoded_cats: String) -> &mut Self {
        self.raw_encoded_cats = Some(raw_encoded_cats);
        self
    }

    pub fn add_revproxy_region(&mut self, revproxy_region: String) -> &mut Self {
        self.revproxy_region = Some(revproxy_region);
        self
    }

    pub fn add_client_info(&mut self, client_info: ClientInfo) -> &mut Self {
        self.client_info = Some(client_info);
        self
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub fn identities(&self) -> &MononokeIdentitySet {
        &self.identities
    }

    pub fn raw_encoded_cats(&self) -> &Option<String> {
        &self.raw_encoded_cats
    }

    pub fn is_trusted_client(&self) -> bool {
        self.is_trusted_client
    }

    pub fn set_identities(mut self, identities: MononokeIdentitySet) -> Self {
        self.identities = identities;
        self
    }

    pub fn priority(&self) -> &Priority {
        &self.priority
    }

    pub fn revproxy_region(&self) -> &Option<String> {
        &self.revproxy_region
    }

    pub fn client_debug(&self) -> bool {
        self.client_debug
    }

    pub fn client_ip(&self) -> Option<&IpAddr> {
        self.client_ip.as_ref()
    }

    pub fn client_hostname(&self) -> Option<&str> {
        self.client_hostname.as_deref()
    }

    pub fn set_client_hostname(mut self, client_hostname: Option<String>) -> Self {
        self.client_hostname = client_hostname;
        self
    }

    pub fn unix_name(&self) -> Option<&str> {
        for identity in self.identities() {
            if identity.id_type() == "USER" {
                return Some(identity.id_data());
            }
        }

        None
    }

    pub fn sandcastle_alias(&self) -> Option<&str> {
        self.client_info
            .as_ref()
            .and_then(|ci| ci.fb.sandcastle_alias())
    }

    pub fn clientinfo_u64tag(&self) -> Option<u64> {
        self.client_info.as_ref()?.u64token
    }

    pub fn sandcastle_nonce(&self) -> Option<&str> {
        self.client_info
            .as_ref()
            .and_then(|ci| ci.fb.sandcastle_nonce())
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
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "bad ssh stream",
                    ));
                }
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
    use bytes::{BufMut, BytesMut};
    use tokio_util::codec::{Decoder, Encoder};

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
