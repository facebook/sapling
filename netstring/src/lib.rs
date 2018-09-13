// Copyright 2004-present Facebook. All Rights Reserved.

//! Tokio-based implementation of netstrings
//!
//! [Netstring](http://cr.yp.to/proto/netstrings.txt) is an extremely simple mechanism for
//! delimiting messages in a stream.
//!
//! Each message has the form "7:message," where the initial decimal number is the size of the
//! payload, followed by a ':', then the payload, and a terminating ','. There is no error
//! checking or correction other than the requirement that the message be followed by a comma.

extern crate bytes;
#[macro_use]
extern crate error_chain;
#[cfg(test)]
#[macro_use]
extern crate quickcheck;
extern crate tokio_io;

use std::io::{Cursor, Write};
use std::marker::PhantomData;

use bytes::{BufMut, BytesMut};
use tokio_io::codec::{Decoder, Encoder};

mod errors;
pub use errors::*;

#[derive(Debug, Copy, Clone)]
enum State {
    Num(usize),  // waiting for a complete number
    Body(usize), // waiting for remaining body and comma
}

/// A Netstring decoder.
///
/// The items are always a `BytesMut` for now.
#[derive(Debug, Copy, Clone)]
pub struct NetstringDecoder {
    state: Option<State>,
}

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

#[derive(Debug)]
struct Slice(usize, usize);
impl Slice {
    fn new(base: usize, len: usize) -> Self {
        Slice(base, len)
    }
    fn len(&self) -> usize {
        self.1
    }
    fn start(&self) -> usize {
        self.0
    }
    fn end(&self) -> usize {
        self.0 + self.1
    }
}

impl NetstringDecoder {
    pub fn new() -> Self {
        NetstringDecoder {
            state: Some(State::Num(0)),
        }
    }

    /// Decode parser. This maintains the internal state machine which tracks what we've seen
    /// before. It will return as much output as it can on each call, or None if nothing can be
    /// returned. The second part of the tuple is the amount of the input buffer we have consumed;
    /// it is always at least as much as the output option.
    fn decode_buf<'a>(&mut self, buf: &'a [u8]) -> Result<(usize, Option<(bool, Slice)>)> {
        let mut consumed = 0;
        loop {
            let state = self.state
                .take()
                .ok_or(ErrorKind::NetstringDecode("bad state"))?;

            let buf = &buf[consumed..];

            let (next, ret): (State, Option<Option<(bool, Slice)>>) = match state {
                State::Num(mut cur) => {
                    let mut next = None;

                    for (idx, inp) in buf.iter().enumerate() {
                        match *inp {
                            digit @ b'0'...b'9' => cur = cur * 10 + ((digit - b'0') as usize),
                            b':' => {
                                next = Some((idx + 1, State::Body(cur)));
                                break;
                            }
                            _ => bail!(ErrorKind::NetstringDecode("Bad character in payload size")),
                        }
                    }

                    if let Some((eaten, next)) = next {
                        // We got a complete length, so we can continue without returning
                        // anything.
                        consumed += eaten;
                        (next, None)
                    } else {
                        // Partial input number - consume what we have then return indicating
                        // we need more.
                        consumed += buf.len();

                        (State::Num(cur), Some(None))
                    }
                }

                State::Body(len) => {
                    // length of payload + ','
                    if buf.len() >= len + 1 {
                        // We have up to the end of the buffer, so we can return it and
                        // start expecting the next buffer.
                        let v = Slice::new(consumed, len);

                        ensure!(buf[len] == b',', ErrorKind::NetstringDecode("missing ','"));
                        consumed += len + 1;

                        (State::Num(0), Some(Some((true, v))))
                    } else {
                        // Consume as much of the input as we can, and leave the state set up
                        // to handle the rest as it arrives.
                        let v = Slice::new(consumed, buf.len());
                        consumed += v.len();

                        (State::Body(len - v.len()), Some(Some((false, v))))
                    }
                }
            };

            self.state = Some(next);
            if let Some(ret) = ret {
                return Ok((consumed, ret));
            }
        }
    }
}

impl Decoder for NetstringDecoder {
    type Item = BytesMut;
    type Error = Error;

    /// Decode a netstring. Is left in a broken state if it ever returns an error,
    /// as it implies the framing is broken on the stream and the whole thing needs
    /// to be reset.
    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>> {
        // The Decoder API can't deal with partial results, so if we don't get a complete
        // result we roll back the internal state to this.
        let saved = *self;

        let (consumed, ret) = self.decode_buf(buf.as_ref())?;

        match ret {
            Some((true, slice)) => {
                // Got a complete result from complete input
                debug_assert!(
                    slice.end() <= buf.len(),
                    "slice {:?} end {} after buf {}",
                    slice,
                    slice.end(),
                    buf.len()
                );
                debug_assert!(
                    slice.end() <= consumed,
                    "slice {:?} consumed {}",
                    slice,
                    consumed
                );

                let mut ret = buf.split_to(slice.end());

                if consumed > slice.end() {
                    let _ = buf.split_to(consumed - slice.end());
                }

                let _ = ret.split_to(slice.start());

                Ok(Some(ret))
            }
            Some((false, _)) | None => {
                // Either partial result or incomplete input - roll back state and ask for more.
                *self = saved;
                Ok(None)
            }
        }
    }
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
    use bytes::BytesMut;
    use tokio_io::codec::{Decoder, Encoder};

    use super::*;

    #[test]
    fn decode_simple() {
        let mut buf = BytesMut::with_capacity(1);
        buf.put_slice(b"5:hello,");

        let mut codec = NetstringDecoder::new();

        match codec.decode(&mut buf) {
            Ok(Some(ref res)) if res.as_ref() == b"hello" => (),
            bad => panic!(
                "decode failed: {:?}",
                bad.as_ref().map(|x| x.as_ref().map(BytesMut::as_ref))
            ),
        }
    }

    #[test]
    fn decode_multiple() {
        let mut buf = BytesMut::with_capacity(1);

        buf.put_slice(b"5:hello,5:world,");

        let mut codec = NetstringDecoder::new();

        match codec.decode(&mut buf) {
            Ok(Some(ref res)) if res.as_ref() == b"hello" => (),
            bad => panic!(
                "decode failed: {:?}",
                bad.as_ref().map(|x| x.as_ref().map(BytesMut::as_ref))
            ),
        }

        match codec.decode(&mut buf) {
            Ok(Some(ref res)) if res.as_ref() == b"world" => (),
            bad => panic!(
                "decode failed: {:?}",
                bad.as_ref().map(|x| x.as_ref().map(BytesMut::as_ref))
            ),
        }
    }

    #[test]
    fn decode_zero() {
        let mut buf = BytesMut::with_capacity(1);

        buf.put_slice(b"0:,");

        let mut codec = NetstringDecoder::new();

        match codec.decode(&mut buf) {
            Ok(Some(ref res)) if res.as_ref() == b"" => (),
            bad => panic!(
                "decode failed: {:?}",
                bad.as_ref().map(|x| x.as_ref().map(BytesMut::as_ref))
            ),
        }

        match codec.decode(&mut buf) {
            Ok(None) => (),
            bad => panic!(
                "decode failed: {:?}",
                bad.as_ref().map(|x| x.as_ref().map(BytesMut::as_ref))
            ),
        }
    }

    #[test]
    fn decode_partial_len_digits() {
        let mut buf = BytesMut::with_capacity(1);
        buf.put_slice(b"1");

        let mut codec = NetstringDecoder::new();

        match codec.decode(&mut buf) {
            Ok(None) => (),
            bad => panic!(
                "decode failed: {:?}",
                bad.as_ref().map(|x| x.as_ref().map(BytesMut::as_ref))
            ),
        }

        buf.put_slice(b"2:hello, world,");

        match codec.decode(&mut buf) {
            Ok(Some(ref res)) if res.as_ref() == b"hello, world" => (),
            bad => panic!(
                "decode failed: {:?}",
                bad.as_ref().map(|x| x.as_ref().map(BytesMut::as_ref))
            ),
        }
    }

    #[test]
    fn decode_partial_len_colon() {
        let mut buf = BytesMut::with_capacity(1);
        buf.put_slice(b"12");

        let mut codec = NetstringDecoder::new();

        match codec.decode(&mut buf) {
            Ok(None) => (),
            bad => panic!(
                "decode failed: {:?}",
                bad.as_ref().map(|x| x.as_ref().map(BytesMut::as_ref))
            ),
        }

        buf.put_slice(b":hello, world,");

        match codec.decode(&mut buf) {
            Ok(Some(ref res)) if res.as_ref() == b"hello, world" => (),
            bad => panic!(
                "decode failed: {:?}",
                bad.as_ref().map(|x| x.as_ref().map(BytesMut::as_ref))
            ),
        }
    }

    #[test]
    fn decode_partial_body() {
        let mut buf = BytesMut::with_capacity(1);
        buf.put_slice(b"12:hello,");

        let mut codec = NetstringDecoder::new();

        match codec.decode(&mut buf) {
            Ok(None) => (),
            bad => panic!(
                "decode failed: {:?}",
                bad.as_ref().map(|x| x.as_ref().map(BytesMut::as_ref))
            ),
        }

        buf.put_slice(b" world,");

        match codec.decode(&mut buf) {
            Ok(Some(ref res)) if res.as_ref() == b"hello, world" => (),
            bad => panic!(
                "decode failed: {:?}",
                bad.as_ref().map(|x| x.as_ref().map(BytesMut::as_ref))
            ),
        }
    }

    #[test]
    fn decode_partial_comma() {
        let mut buf = BytesMut::with_capacity(1);
        buf.put_slice(b"12:hello, world");

        let mut codec = NetstringDecoder::new();

        match codec.decode(&mut buf) {
            Ok(None) => (),
            bad => panic!(
                "decode failed: {:?}",
                bad.as_ref().map(|x| x.as_ref().map(BytesMut::as_ref))
            ),
        }

        buf.put_slice(b",");

        match codec.decode(&mut buf) {
            Ok(Some(ref res)) if res.as_ref() == b"hello, world" => (),
            bad => panic!(
                "decode failed: {:?}",
                bad.as_ref().map(|x| x.as_ref().map(BytesMut::as_ref))
            ),
        }
    }

    #[test]
    fn decode_bad_len() {
        let mut buf = BytesMut::with_capacity(1);
        buf.put_slice(b"0x12:hello, world,");

        let mut codec = NetstringDecoder::new();

        match codec.decode(&mut buf) {
            Err(e) => println!("got expected error {:?}", e),
            bad => panic!(
                "decode succeeded: {:?}",
                bad.as_ref().map(|x| x.as_ref().map(BytesMut::as_ref))
            ),
        }
    }

    #[test]
    fn decode_bad_comma() {
        let mut buf = BytesMut::with_capacity(1);
        buf.put_slice(b"12:hello, worldx");

        let mut codec = NetstringDecoder::new();

        match codec.decode(&mut buf) {
            Err(e) => println!("got expected error {:?}", e),
            bad => panic!(
                "decode succeeded: {:?}",
                bad.as_ref().map(|x| x.as_ref().map(BytesMut::as_ref))
            ),
        }
    }

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
