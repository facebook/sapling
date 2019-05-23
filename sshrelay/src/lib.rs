// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate bytes;
extern crate futures;
extern crate futures_ext;
#[macro_use]
extern crate maplit;
extern crate netstring;

#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate tokio_io;
extern crate uuid;

use std::collections::HashMap;
use std::env::var;
use std::io;

use bytes::{BufMut, Bytes, BytesMut};
use futures::{sink::Wait, sync::mpsc};
use futures_ext::BoxStream;
use tokio_io::codec::{Decoder, Encoder};
use uuid::Uuid;

use netstring::{NetstringDecoder, NetstringEncoder};

// Multiplex stdin/out/err over a single stream using netstring as framing
#[derive(Debug)]
pub struct SshDecoder(NetstringDecoder);

#[derive(Debug)]
pub struct SshEncoder(NetstringEncoder<Bytes>);

pub struct Stdio {
    pub preamble: Preamble,
    pub stdin: BoxStream<Bytes, io::Error>,
    pub stdout: mpsc::Sender<Bytes>,
    pub stderr: mpsc::Sender<Bytes>,
}

pub struct SenderBytesWrite {
    pub chan: Wait<mpsc::Sender<Bytes>>,
}

impl io::Write for SenderBytesWrite {
    fn flush(&mut self) -> io::Result<()> {
        self.chan
            .flush()
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e))
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.chan
            .send(Bytes::from(buf))
            .map(|_| buf.len())
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e))
    }
}

// Common information for a connection
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Preamble {
    // Name of the repo to connect to
    pub reponame: String,
    // Additional information that will be send to the server. Examples: user/host identity.
    pub misc: HashMap<String, String>,
}

impl Preamble {
    pub fn new(
        reponame: String,
        session_uuid: Uuid,
        unix_username: Option<String>,
        source_hostname: Option<String>,
        ssh_env_vars: SshEnvVars,
    ) -> Self {
        let mut misc = hashmap! {"session_uuid".to_owned() => format!("{}", session_uuid)};
        if let Some(unix_username) = unix_username {
            misc.insert("unix_username".to_owned(), unix_username);
        }
        if let Some(source_hostname) = source_hostname {
            misc.insert("source_hostname".to_owned(), source_hostname);
        }

        ssh_env_vars.add_into_map(&mut misc);

        Self { reponame, misc }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SshStream {
    Stdin,
    Stdout,
    Stderr,
    Preamble(Preamble),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SshMsg(SshStream, Bytes);

impl SshMsg {
    pub fn new(stream: SshStream, data: Bytes) -> Self {
        SshMsg(stream, data)
    }

    pub fn from_slice<T>(stream: SshStream, t: T) -> Self
    where
        T: AsRef<[u8]>,
    {
        Self::new(stream, Bytes::from(t.as_ref()))
    }

    pub fn stream(&self) -> SshStream {
        self.0.clone()
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
        SshDecoder(NetstringDecoder::new())
    }
}

fn ioerr_cvt(err: netstring::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("{}", err))
}

impl Decoder for SshDecoder {
    type Item = SshMsg;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> io::Result<Option<SshMsg>> {
        if let Some(mut data) = self.0.decode(buf).map_err(ioerr_cvt)? {
            if data.len() == 0 {
                return Ok(None);
            }
            match data.split_to(1)[0] {
                0 => Ok(Some(SshMsg(SshStream::Stdin, data.freeze()))),
                1 => Ok(Some(SshMsg(SshStream::Stdout, data.freeze()))),
                2 => Ok(Some(SshMsg(SshStream::Stderr, data.freeze()))),
                3 => {
                    let data = data.freeze();
                    let strdata = match std::str::from_utf8(&data) {
                        Ok(data) => data,
                        Err(err) => {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                format!("expected valid utf8 input for preamble: {}", err),
                            ));
                        }
                    };
                    let preamble: Preamble = serde_json::from_str(strdata)?;
                    Ok(Some(SshMsg(SshStream::Preamble(preamble), Bytes::new())))
                }
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
    pub fn new() -> Self {
        SshEncoder(NetstringEncoder::new())
    }
}

impl Encoder for SshEncoder {
    type Item = SshMsg;
    type Error = io::Error;

    fn encode(&mut self, msg: SshMsg, buf: &mut BytesMut) -> io::Result<()> {
        let mut v = BytesMut::with_capacity(1 + msg.1.len());
        match msg.0 {
            SshStream::Stdin => {
                v.put_u8(0);
                v.put_slice(&msg.1);
                Ok(self.0.encode(v.freeze(), buf).map_err(ioerr_cvt)?)
            }
            SshStream::Stdout => {
                v.put_u8(1);
                v.put_slice(&msg.1);
                Ok(self.0.encode(v.freeze(), buf).map_err(ioerr_cvt)?)
            }
            SshStream::Stderr => {
                v.put_u8(2);
                v.put_slice(&msg.1);
                Ok(self.0.encode(v.freeze(), buf).map_err(ioerr_cvt)?)
            }
            SshStream::Preamble(preamble) => {
                // msg.1 is ignored in preamble
                debug_assert!(msg.1.len() == 0, "preamble ignores additional bytes");
                v.put_u8(3);
                let preamble = serde_json::to_vec(&preamble)?;
                v.extend_from_slice(&preamble);
                Ok(self.0.encode(v.freeze(), buf).map_err(ioerr_cvt)?)
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct SshEnvVars {
    pub ssh_cert_principals: Option<String>,
    pub ssh_original_command: Option<String>,
    pub ssh_client: Option<String>,
}

impl SshEnvVars {
    const SSH_CERT_PRINCIPALS: &'static str = "SSH_CERT_PRINCIPALS";
    const SSH_ORIGINAL_COMMAND: &'static str = "SSH_ORIGINAL_COMMAND";
    const SSH_CLIENT: &'static str = "SSH_CLIENT";

    pub fn new_from_env() -> Self {
        Self {
            ssh_cert_principals: var(Self::SSH_CERT_PRINCIPALS).ok(),
            ssh_original_command: var(Self::SSH_ORIGINAL_COMMAND).ok(),
            ssh_client: var(Self::SSH_CLIENT).ok(),
        }
    }

    pub fn add_into_map(self, map: &mut HashMap<String, String>) {
        let Self {
            ssh_cert_principals,
            ssh_original_command,
            ssh_client,
        } = self;

        if let Some(v) = ssh_cert_principals {
            map.insert(Self::SSH_CERT_PRINCIPALS.to_string(), v);
        }

        if let Some(v) = ssh_original_command {
            map.insert(Self::SSH_ORIGINAL_COMMAND.to_string(), v);
        }

        if let Some(v) = ssh_client {
            map.insert(Self::SSH_CLIENT.to_string(), v);
        }
    }

    pub fn from_map(map: &HashMap<String, String>) -> Self {
        Self {
            ssh_cert_principals: map.get(Self::SSH_CERT_PRINCIPALS).cloned(),
            ssh_original_command: map.get(Self::SSH_ORIGINAL_COMMAND).cloned(),
            ssh_client: map.get(Self::SSH_CLIENT).cloned(),
        }
    }
}

#[cfg(test)]
mod test {
    use bytes::{BufMut, BytesMut};
    use tokio_io::codec::{Decoder, Encoder};

    use super::SshStream::*;
    use super::*;

    trait ToBytes: AsRef<[u8]> {
        fn bytes(&self) -> Bytes {
            Bytes::from(self.as_ref())
        }
    }

    impl<T> ToBytes for T where T: AsRef<[u8]> {}

    #[test]
    fn encode_simple() {
        let mut buf = BytesMut::with_capacity(1024);
        let mut encoder = SshEncoder::new();

        encoder
            .encode(SshMsg::new(Stdin, b"ls -l".bytes()), &mut buf)
            .expect("encode failed");

        assert_eq!(buf.as_ref(), b"6:\x00ls -l,");
    }

    #[test]
    fn encode_zero() {
        let mut buf = BytesMut::with_capacity(1024);
        let mut encoder = SshEncoder::new();

        encoder
            .encode(SshMsg::new(Stdin, b"".bytes()), &mut buf)
            .expect("encode failed");

        assert_eq!(buf.as_ref(), b"1:\x00,");
    }

    #[test]
    fn encode_one() {
        let mut buf = BytesMut::with_capacity(1024);
        let mut encoder = SshEncoder::new();

        encoder
            .encode(SshMsg::new(Stdin, b"X".bytes()), &mut buf)
            .expect("encode failed");

        assert_eq!(buf.as_ref(), b"2:\x00X,");
    }

    #[test]
    fn encode_multi() {
        let mut buf = BytesMut::with_capacity(1024);
        let mut encoder = SshEncoder::new();

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
    fn decode_simple() {
        let mut buf = BytesMut::with_capacity(1024);
        buf.put_slice(b"6:\x00ls -l,");

        let mut decoder = SshDecoder::new();

        match decoder.decode(&mut buf) {
            Ok(Some(ref res)) if res == &SshMsg::new(Stdin, b"ls -l".bytes()) => (),
            bad => panic!("decode failed: {:?}", bad.as_ref()),
        }
    }

    #[test]
    fn decode_zero() {
        let mut buf = BytesMut::with_capacity(1024);
        buf.put_slice(b"1:\x00,");

        let mut decoder = SshDecoder::new();

        match decoder.decode(&mut buf) {
            Ok(Some(ref res)) if res == &SshMsg::new(Stdin, b"".bytes()) => (),
            bad => panic!("decode failed: {:?}", bad.as_ref()),
        }
    }

    #[test]
    fn decode_one() {
        let mut buf = BytesMut::with_capacity(1024);
        buf.put_slice(b"2:\x00X,");

        let mut decoder = SshDecoder::new();

        match decoder.decode(&mut buf) {
            Ok(Some(ref res)) if res == &SshMsg::new(Stdin, b"X".bytes()) => (),
            bad => panic!("decode failed: {:?}", bad.as_ref()),
        }
    }

    #[test]
    fn decode_multi() {
        let mut buf = BytesMut::with_capacity(1024);
        buf.put_slice(b"2:\x00X,2:\x01Y,2:\x02Z,");

        let mut decoder = SshDecoder::new();

        match decoder.decode(&mut buf) {
            Ok(Some(ref res)) if res == &SshMsg::new(Stdin, b"X".bytes()) => (),
            bad => panic!("decode failed: {:?}", bad.as_ref()),
        }
        match decoder.decode(&mut buf) {
            Ok(Some(ref res)) if res == &SshMsg::new(Stdout, b"Y".bytes()) => (),
            bad => panic!("decode failed: {:?}", bad.as_ref()),
        }
        match decoder.decode(&mut buf) {
            Ok(Some(ref res)) if res == &SshMsg::new(Stderr, b"Z".bytes()) => (),
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
            Err(_err) => (),
        }
    }

    #[test]
    fn decode_short_framing() {
        let mut buf = BytesMut::with_capacity(1024);
        buf.put_slice(b"3:\x02X,");

        let mut decoder = SshDecoder::new();

        match decoder.decode(&mut buf) {
            Ok(None) => (),
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
            Err(_err) => (),
        }
    }
}
