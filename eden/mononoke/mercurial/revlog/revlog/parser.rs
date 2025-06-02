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
use nom::alt;
use nom::call;
use nom::do_parse;
use nom::error::ErrorKind;
use nom::error::ParseError;
use nom::length_data;
use nom::many0;
use nom::map;
use nom::named;
use nom::number::streaming::be_u16;
use nom::number::streaming::be_u32;
use nom::peek;
use nom::tag;
use nom::take;

use super::lz4;
use crate::revlog::revidx::RevIdx;

// #[derive(Copy, Clone, Debug, Eq, PartialEq)]
// pub enum Badness {
// Version,
// Features,
// BadZlib,
// }
//

#[derive(Debug, PartialEq)]
pub enum Error {
    Custom(u32),
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

// hack until I work out how to propagate proper E type
#[allow(non_upper_case_globals, non_snake_case, dead_code)]
pub mod Badness {
    use super::Error;
    pub const Version: Error = Error::Custom(1);
    pub const Features: Error = Error::Custom(2);
    pub const BadZlib: Error = Error::Custom(3);
    pub const BadLZ4: Error = Error::Custom(4);
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
named!(pub header<&[u8], Header, ()>,
    do_parse!(
        features: be_u16 >>
        version: be_u16 >>
        ({
            let vers = match version {
                0 => Version::Revlog0,
                1 => Version::RevlogNG,
                _ => panic!("bad version"),
            };

            let features = match Features::from_bits(features) {
                Some(f) => f,
                None => panic!("bad features"),
            };

            Header {
                version: vers,
                features,
            }
        })
    )
);

pub fn indexng_size() -> usize {
    6 + 2 + 4 + 4 + 4 + 4 + 4 + 4 + 32
}

// Parse an "NG" revlog entry
named!(pub indexng<&[u8], Entry, ()>,
    do_parse!(
        offset: be_u48 >>    // XXX if first, then only 2 bytes, implied 0 in top 4
        flags: be_u16 >>     // ?
        compressed_length: be_u32 >>
        uncompressed_length: be_u32 >>
        baserev: be_u32 >>
        linkrev: be_u32 >>
        p1: be_u32 >>
        p2: be_u32 >>
        hash: take!(32) >>
        ({
            Entry {
                offset,
                flags: IdxFlags::from_bits(flags).expect("bad rev idx flags"),
                compressed_len: compressed_length,
                len: Some(uncompressed_length),
                baserev: if baserev == !0 { None } else { Some(baserev.into()) },
                linkrev: linkrev.into(),
                p1: if p1 == !0 { None } else { Some(p1.into()) },
                p2: if p2 == !0 { None } else { Some(p2.into()) },
                nodeid: HgNodeHash::from_bytes(&hash[..20]).expect("bad bytes for sha"),
            }
        })
    )
);

pub fn index0_size() -> usize {
    4 + 4 + 4 + 4 + 4 + 4 + 4 + 20
}

// Parse an original revlog entry
named!(pub index0<&[u8], Entry, ()>,
    do_parse!(
        _header: header >>
        offset: be_u32 >>
        compressed_length: be_u32 >>
        baserev: be_u32 >>
        linkrev: be_u32 >>
        p1: be_u32 >>
        p2: be_u32 >>
        hash: take!(20) >>
        ({
            Entry {
                offset: offset as u64,
                flags: IdxFlags::empty(),
                compressed_len: compressed_length,
                len: None,
                baserev: if baserev == !0 { None } else { Some(baserev.into()) },
                linkrev: linkrev.into(),
                p1: if p1 == !0 { None } else { Some(p1.into()) },
                p2: if p2 == !0 { None } else { Some(p2.into()) },
                nodeid: HgNodeHash::from_bytes(&hash[..20]).expect("bad bytes for sha"),
            }
        })
    )
);

// Parse a single Delta
named!(pub delta<&[u8], Delta, Error>,
    do_parse!(
        start: be_u32 >>
        end: be_u32 >>
        content: length_data!(be_u32) >>
        ({
            Delta {
                start: start as usize,
                end: end as usize,
                content: content.into(),
            }
        })
    )
);

// Parse 0 or more deltas
named!(deltas<&[u8], Vec<Delta>, Error>, many0!(delta));

// A chunk of data data that contains some Deltas; the caller defines the framing bytes
// bounding the input.
named!(pub deltachunk<&[u8], Vec<Delta>, Error>,
    map!(
        many0!(
            alt!(
                do_parse!(tag!(b"u") >> d: deltas >> (d)) |                                  // uncompressed with explicit 'u' header
                do_parse!(peek!(tag!(b"\0")) >> d: deltas >> (d)) |                          // uncompressed with included initial 0x00
                do_parse!(peek!(tag!(b"x")) >> d: call!(zlib_decompress, deltas) >> (d)) |  // compressed; 'x' part of the zlib stream
                do_parse!(tag!(b"4") >> d: call!(lz4::lz4_decompress, deltas) >> (d))       // compressed w/ lz4
            )
        ),
        |dv: Vec<_>| dv.into_iter().flatten().collect())
);

fn remains(i: &[u8]) -> IResult<&[u8], &[u8], Error> {
    Ok((&i[..0], i))
}

named!(remains_owned<&[u8], Vec<u8>, Error>, map!(remains, |x: &[u8]| x.into()));

// Parse some literal data, possibly compressed
named!(pub literal<&[u8], Vec<u8>, Error>,
    alt!(
        do_parse!(peek!(tag!(b"\0")) >> d: remains >> (d.into())) |
        do_parse!(peek!(tag!(b"x")) >> d: call!(zlib_decompress, remains_owned) >> (d)) |
        do_parse!(tag!(b"4") >> d: call!(lz4::lz4_decompress, remains_owned) >> (d)) |
        do_parse!(tag!(b"u") >> d: remains >> (d.into()))
    )
);

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
fn zlib_decompress<'a, P, R: 'a, E: Debug + 'a>(i: &'a [u8], parse: P) -> IResult<&'a [u8], R, E>
where
    for<'p> P: Fn(&'p [u8]) -> IResult<&'p [u8], R, E>,
{
    let mut data = Vec::new();

    let inused = {
        let mut zdec = ZlibDecoder::new(i);

        match zdec.read_to_end(&mut data) {
            Ok(_) => zdec.total_in() as usize,
            Err(err) => panic!("zdec failed {:?}", err),
        }
    };

    let remains = &i[inused..];

    detach_result(parse(&data[..]), remains)
}

/// Parse a 6 byte big-endian offset
#[inline]
#[allow(clippy::identity_op)]
fn be_u48(i: &[u8]) -> IResult<&[u8], u64, ()> {
    if i.len() < 6 {
        Err(Err::Incomplete(Needed::Size(6)))
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
