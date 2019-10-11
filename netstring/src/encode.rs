/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::io::{Cursor, Write};
use std::marker::PhantomData;

use bytes::{BufMut, BytesMut};
use tokio_io::codec::Encoder;

use crate::errors::*;

/// A Netstring encoder.
///
/// The items can be anything that can be referenced as a `[u8]`.
#[derive(Debug)]
pub struct NetstringEncoder<Out>
where
    Out: AsRef<[u8]>,
{
    _marker: PhantomData<Out>,
}

impl<Out> NetstringEncoder<Out>
where
    Out: AsRef<[u8]>,
{
    pub fn new() -> Self {
        NetstringEncoder {
            _marker: PhantomData,
        }
    }
}

impl<Out> Encoder for NetstringEncoder<Out>
where
    Out: AsRef<[u8]>,
{
    type Item = Out;
    type Error = Error;

    fn encode(&mut self, msg: Self::Item, buf: &mut BytesMut) -> Result<()> {
        let msg = msg.as_ref();

        // Assume that 20 digits is long enough for the length
        // <len> ':' <payload> ','
        buf.reserve(20 + 1 + msg.len() + 1);

        unsafe {
            let adv = {
                let mut wr = Cursor::new(buf.bytes_mut());
                write!(wr, "{}:", msg.len()).expect("write to slice failed?");
                wr.position() as usize
            };
            buf.advance_mut(adv);
        }

        buf.put_slice(msg);
        buf.put_u8(b',');
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::NetstringDecoder;
    use bytes::BytesMut;
    use quickcheck::quickcheck;
    use tokio_io::codec::Decoder;

    use super::*;

    #[test]
    fn encode_simple() {
        let mut buf = BytesMut::with_capacity(1);

        let mut codec = NetstringEncoder::<&[u8]>::new();

        assert!(codec.encode(b"hello, world", &mut buf).is_ok());
        assert_eq!(buf.as_ref(), b"12:hello, world,");
    }

    #[test]
    fn encode_zero() {
        let mut buf = BytesMut::with_capacity(1);

        let mut codec = NetstringEncoder::<&[u8]>::new();

        assert!(codec.encode(b"", &mut buf).is_ok());
        assert_eq!(buf.as_ref(), b"0:,");
    }

    #[test]
    fn encode_multiple() {
        let mut buf = BytesMut::with_capacity(1);

        let mut codec = NetstringEncoder::<&[u8]>::new();

        assert!(codec.encode(b"hello, ", &mut buf).is_ok());
        assert!(codec.encode(b"world!", &mut buf).is_ok());
        assert_eq!(buf.as_ref(), b"7:hello, ,6:world!,");
    }

    quickcheck! {
        fn roundtrip(s: Vec<u8>) -> bool {
            let mut buf = BytesMut::with_capacity(1);
            let mut enc = NetstringEncoder::new();

            assert!(enc.encode(&s, &mut buf).is_ok(), "encode failed");

            let mut dec = NetstringDecoder::new();
            let out = dec.decode(&mut buf).expect("decode failed").expect("incomplete");

            s == out
        }
    }
}
