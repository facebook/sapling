/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Nom parser for Mercurial revlogs

use std::fmt::Debug;
use std::io::Read;

use bitflags::bitflags;
use flate2::read::ZlibDecoder;
use mercurial_types::HgNodeHash;
use mercurial_types::bdiff::Delta;
use nom::Err;
use nom::IResult;
use nom::Needed;
use nom::Parser as _;
use nom::branch::alt;
use nom::bytes::streaming::tag;
use nom::bytes::streaming::take;
use nom::combinator::peek;
use nom::error::ErrorKind;
use nom::error::ParseError;
use nom::multi::length_data;
use nom::multi::many0;
use nom::number::streaming::be_u16;
use nom::number::streaming::be_u32;
use nom::sequence::preceded;

use crate::revlog::lz4::lz4_decompress;
use crate::revlog::revidx::RevIdx;

#[derive(Debug, PartialEq)]
pub enum Error {
    BadVersion,
    BadFeatures,
    BadZlib(String),
    BadLZ4(String),
    Nom(ErrorKind),
}

impl ParseError<&[u8]> for Error {
    fn from_error_kind(_input: &[u8], kind: ErrorKind) -> Self {
        Error::Nom(kind)
    }

    fn append(_input: &[u8], _kind: ErrorKind, other: Self) -> Self {
        other
    }
}

// `Revlog` features
bitflags! {
    #[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone, Copy)]
    pub struct Features: u16 {
        const INLINE        = 1 << 0;
        const GENERAL_DELTA = 1 << 1;
    }
}

// Per-revision flags
bitflags! {
    #[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone, Copy)]
    pub struct IdxFlags: u16 {
        const OCTOPUS_MERGE = 1 << 12;
        const EXTSTORED     = 1 << 13;
        const CENSORED      = 1 << 15;
    }
}

/// Revlog version number
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Version {
    Revlog0 = 0,
    RevlogNG = 1,
}

/// Revlog header
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Header {
    pub version: Version,
    pub features: Features,
}

/// Entry entry for a revision
#[derive(Copy, Clone, Debug)]
pub struct Entry {
    pub offset: u64,         // offset of content (delta/literal) in datafile (or inlined)
    pub flags: IdxFlags,     // unused?
    pub compressed_len: u32, // compressed content size
    pub len: Option<u32>,    // size of final file (after applying deltas)
    pub baserev: Option<RevIdx>, // base/previous rev for deltas (None if literal)
    pub linkrev: RevIdx,     // changeset id
    pub p1: Option<RevIdx>,  // parent p1
    pub p2: Option<RevIdx>,  // parent p2
    pub nodeid: HgNodeHash,  // nodeid
}

impl Entry {
    pub fn nodeid(&self) -> HgNodeHash {
        self.nodeid
    }
}

// Parse the revlog header
pub fn header(input: &[u8]) -> IResult<&[u8], Header, Error> {
    let (input, features) = be_u16(input)?;
    let (input, version) = be_u16(input)?;

    let vers = match version {
        0 => Version::Revlog0,
        1 => Version::RevlogNG,
        _ => return Err(Err::Failure(Error::BadVersion)),
    };

    let features = match Features::from_bits(features) {
        Some(f) => f,
        None => return Err(Err::Failure(Error::BadFeatures)),
    };

    let header = Header {
        version: vers,
        features,
    };

    Ok((input, header))
}

pub fn indexng_size() -> usize {
    6 + 2 + 4 + 4 + 4 + 4 + 4 + 4 + 32
}

// Parse an "NG" revlog entry
pub fn indexng(input: &[u8]) -> IResult<&[u8], Entry, Error> {
    let (input, offset) = be_u48(input)?; // XXX if first, then only 2 bytes, implied 0 in top 4
    let (input, flags) = be_u16(input)?; // ?
    let (input, compressed_length) = be_u32(input)?;
    let (input, uncompressed_length) = be_u32(input)?;
    let (input, baserev) = be_u32(input)?;
    let (input, linkrev) = be_u32(input)?;
    let (input, p1) = be_u32(input)?;
    let (input, p2) = be_u32(input)?;
    let (input, hash) = take(32usize).parse(input)?;

    let entry = Entry {
        offset,
        flags: IdxFlags::from_bits(flags).expect("bad rev idx flags"),
        compressed_len: compressed_length,
        len: Some(uncompressed_length),
        baserev: if baserev == !0 {
            None
        } else {
            Some(baserev.into())
        },
        linkrev: linkrev.into(),
        p1: if p1 == !0 { None } else { Some(p1.into()) },
        p2: if p2 == !0 { None } else { Some(p2.into()) },
        nodeid: HgNodeHash::from_bytes(&hash[..20]).expect("bad bytes for sha"),
    };

    Ok((input, entry))
}

pub fn index0_size() -> usize {
    4 + 4 + 4 + 4 + 4 + 4 + 4 + 20
}

// Parse an original revlog entry
pub fn index0(input: &[u8]) -> IResult<&[u8], Entry, Error> {
    let (input, _header) = header(input)?;
    let (input, offset) = be_u32(input)?;
    let (input, compressed_length) = be_u32(input)?;
    let (input, baserev) = be_u32(input)?;
    let (input, linkrev) = be_u32(input)?;
    let (input, p1) = be_u32(input)?;
    let (input, p2) = be_u32(input)?;
    let (input, hash) = take(20usize)(input)?;

    let entry = Entry {
        offset: offset as u64,
        flags: IdxFlags::empty(),
        compressed_len: compressed_length,
        len: None,
        baserev: if baserev == !0 {
            None
        } else {
            Some(baserev.into())
        },
        linkrev: linkrev.into(),
        p1: if p1 == !0 { None } else { Some(p1.into()) },
        p2: if p2 == !0 { None } else { Some(p2.into()) },
        nodeid: HgNodeHash::from_bytes(&hash[..20]).expect("bad bytes for sha"),
    };

    Ok((input, entry))
}

// Parse a single Delta
pub fn delta(input: &[u8]) -> IResult<&[u8], Delta, Error> {
    let (input, start) = be_u32(input)?;
    let (input, end) = be_u32(input)?;
    let (input, content) = length_data(be_u32).parse(input)?;

    let delta = Delta {
        start: start as usize,
        end: end as usize,
        content: content.into(),
    };

    Ok((input, delta))
}

// Parse 0 or more deltas
fn deltas(input: &[u8]) -> IResult<&[u8], Vec<Delta>, Error> {
    many0(delta).parse(input)
}

// A chunk of data data that contains some Deltas; the caller defines the framing bytes
// bounding the input.
pub fn deltachunk(input: &[u8]) -> IResult<&[u8], Vec<Delta>, Error> {
    let (input, dv) = many0(alt((
        // uncompressed with explicit 'u' header
        preceded(tag("u"), deltas),
        // uncompressed with included initial 0x00
        preceded(peek(tag("\0")), deltas),
        // compressed; 'x' part of the zlib stream
        preceded(peek(tag("x")), |i| zlib_decompress(i, deltas)),
        // compressed w/ lz4
        preceded(tag("4"), |i| lz4_decompress(i, deltas)),
    )))
    .parse(input)?;

    Ok((input, dv.into_iter().flatten().collect()))
}

fn remains_owned(input: &[u8]) -> IResult<&[u8], Vec<u8>, Error> {
    Ok((&[], input.to_owned()))
}

// Parse some literal data, possibly compressed
pub fn literal(input: &[u8]) -> IResult<&[u8], Vec<u8>, Error> {
    alt((
        preceded(peek(tag("\0")), remains_owned),
        preceded(peek(tag("x")), |i| zlib_decompress(i, remains_owned)),
        preceded(tag("4"), |i| lz4_decompress(i, remains_owned)),
        preceded(tag("u"), remains_owned),
    ))
    .parse(input)
}

// Remap error to remove reference to `data`
pub fn detach_result<'inp, 'out, O: 'out, E: 'out>(
    res: IResult<&'inp [u8], O, E>,
    rest: &'out [u8],
) -> IResult<&'out [u8], O, E> {
    match res {
        Ok((_, o)) => Ok((rest, o)),
        Err(err) => Err(err),
    }
}

/// Unpack a chunk of data and apply a parse function to the output.
fn zlib_decompress<'a, P, R: 'a>(i: &'a [u8], parse: P) -> IResult<&'a [u8], R, Error>
where
    for<'p> P: Fn(&'p [u8]) -> IResult<&'p [u8], R, Error>,
{
    let mut data = Vec::new();

    let inused = {
        let mut zdec = ZlibDecoder::new(i);

        match zdec.read_to_end(&mut data) {
            Ok(_) => zdec.total_in() as usize,
            Err(err) => return Err(Err::Failure(Error::BadZlib(err.to_string()))),
        }
    };

    let remains = &i[inused..];

    detach_result(parse(&data[..]), remains)
}

/// Parse a 6 byte big-endian offset
#[inline]
#[allow(clippy::identity_op)]
fn be_u48(i: &[u8]) -> IResult<&[u8], u64, Error> {
    if i.len() < 6 {
        Err(Err::Incomplete(Needed::new(6)))
    } else {
        let res = ((i[0] as u64) << 40)
            + ((i[1] as u64) << 32)
            + ((i[2] as u64) << 24)
            + ((i[3] as u64) << 16)
            + ((i[4] as u64) << 8)
            + ((i[5] as u64) << 0);
        Ok((&i[6..], res))
    }
}

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;

    use super::Features;
    use super::Header;
    use super::Version;
    use super::header;

    #[mononoke::test]
    fn test_header_0() {
        let d = [0x00, 0x00, 0x00, 0x00];
        assert_eq!(
            header(&d[..]),
            Ok((
                &b""[..],
                Header {
                    version: Version::Revlog0,
                    features: Features::empty(),
                }
            )),
        )
    }

    #[mononoke::test]
    fn test_header_1() {
        let d = [0x00, 0x00, 0x00, 0x01];
        assert_eq!(
            header(&d[..]),
            Ok((
                &b""[..],
                Header {
                    version: Version::RevlogNG,
                    features: Features::empty(),
                }
            )),
        )
    }

    #[mononoke::test]
    fn test_header_feat_1() {
        let d = [0x00, 0x01, 0x00, 0x01];
        assert_eq!(
            header(&d[..]),
            Ok((
                &b""[..],
                Header {
                    version: Version::RevlogNG,
                    features: Features::INLINE,
                }
            )),
        )
    }

    #[mononoke::test]
    fn test_header_feat_2() {
        let d = [0x00, 0x02, 0x00, 0x01];
        assert_eq!(
            header(&d[..]),
            Ok((
                &b""[..],
                Header {
                    version: Version::RevlogNG,
                    features: Features::GENERAL_DELTA,
                }
            )),
        )
    }

    #[mononoke::test]
    fn test_header_feat_3() {
        let d = [0x00, 0x03, 0x00, 0x01];
        assert_eq!(
            header(&d[..]),
            Ok((
                &b""[..],
                Header {
                    version: Version::RevlogNG,
                    features: Features::INLINE | Features::GENERAL_DELTA,
                }
            )),
        )
    }
}
