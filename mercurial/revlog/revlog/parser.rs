// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Nom parser for Mercurial revlogs

use std::fmt::Debug;
use std::io::Read;

use bitflags::bitflags;
use flate2::read::ZlibDecoder;
use nom::{be_u16, be_u32, Err, ErrorKind, IResult, Needed, *};

use mercurial_types::{bdiff::Delta, HgNodeHash};

use crate::revlog::revidx::RevIdx;

use super::lz4;

// #[derive(Copy, Clone, Debug, Eq, PartialEq)]
// pub enum Badness {
// IO,
// Version,
// Features,
// BadZlib,
// }
//

pub type Error = u32;

// hack until I work out how to propagate proper E type
#[allow(non_upper_case_globals, non_snake_case, dead_code)]
pub mod Badness {
    use super::Error;
    pub const IO: Error = 0;
    pub const Version: Error = 1;
    pub const Features: Error = 2;
    pub const BadZlib: Error = 3;
    pub const BadLZ4: Error = 4;
}

// `Revlog` features
bitflags! {
    pub struct Features: u16 {
        const INLINE        = 1 << 0;
        const GENERAL_DELTA = 1 << 1;
    }
}

// Per-revision flags
bitflags! {
    pub struct IdxFlags: u16 {
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
named!(pub header<Header>,
    do_parse!(
        features: return_error!(ErrorKind::Custom(Badness::IO), be_u16) >>
        version: return_error!(ErrorKind::Custom(Badness::IO), be_u16) >>
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
                features: features,
            }
        }))
);

pub fn indexng_size() -> usize {
    6 + 2 + 4 + 4 + 4 + 4 + 4 + 4 + 32
}

// Parse an "NG" revlog entry
named!(pub indexng<Entry>,
    do_parse!(
        offset: return_error!(ErrorKind::Custom(Badness::IO), be_u48) >>    // XXX if first, then only 2 bytes, implied 0 in top 4
        flags: return_error!(ErrorKind::Custom(Badness::IO), be_u16) >>     // ?
        compressed_length: return_error!(ErrorKind::Custom(Badness::IO), be_u32) >>
        uncompressed_length: return_error!(ErrorKind::Custom(Badness::IO), be_u32) >>
        baserev: return_error!(ErrorKind::Custom(Badness::IO), be_u32) >>
        linkrev: return_error!(ErrorKind::Custom(Badness::IO), be_u32) >>
        p1: return_error!(ErrorKind::Custom(Badness::IO), be_u32) >>
        p2: return_error!(ErrorKind::Custom(Badness::IO), be_u32) >>
        hash: take!(32) >>
        ({
            Entry {
                offset: offset,
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
named!(pub index0<Entry>,
    do_parse!(
        _header: header >>
        offset: return_error!(ErrorKind::Custom(Badness::IO), be_u32) >>
        compressed_length: return_error!(ErrorKind::Custom(Badness::IO), be_u32) >>
        baserev: return_error!(ErrorKind::Custom(Badness::IO), be_u32) >>
        linkrev: return_error!(ErrorKind::Custom(Badness::IO), be_u32) >>
        p1: return_error!(ErrorKind::Custom(Badness::IO), be_u32) >>
        p2: return_error!(ErrorKind::Custom(Badness::IO), be_u32) >>
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
named!(pub delta<Delta>,
    do_parse!(
        start: be_u32 >>
        end: be_u32 >>
        content: length_bytes!(be_u32) >>
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
named!(deltas<Vec<Delta>>, many0!(delta));

// A chunk of data data that contains some Deltas; the caller defines the framing bytes
// bounding the input.
named!(pub deltachunk<Vec<Delta> >,
    map!(
        many0!(
            alt!(
                do_parse!(tag!(b"u") >> d: deltas >> (d)) |                                  // uncompressed with explicit 'u' header
                do_parse!(peek!(tag!(b"\0")) >> d: deltas >> (d)) |                          // uncompressed with included initial 0x00
                do_parse!(peek!(tag!(b"x")) >> d: apply!(zlib_decompress, deltas) >> (d)) |  // compressed; 'x' part of the zlib stream
                do_parse!(tag!(b"4") >> d: apply!(lz4::lz4_decompress, deltas) >> (d))       // compressed w/ lz4
            )
        ),
        |dv: Vec<_>| dv.into_iter().flat_map(|x| x).collect())
);

fn remains(i: &[u8]) -> IResult<&[u8], &[u8]> {
    IResult::Done(&i[..0], i)
}

named!(remains_owned<Vec<u8>>, map!(remains, |x: &[u8]| x.into()));

// Parse some literal data, possibly compressed
named!(pub literal<Vec<u8> >,
    alt!(
        do_parse!(peek!(tag!(b"\0")) >> d: remains >> (d.into())) |
        do_parse!(peek!(tag!(b"x")) >> d: apply!(zlib_decompress, remains_owned) >> (d)) |
        do_parse!(tag!(b"4") >> d: apply!(lz4::lz4_decompress, remains_owned) >> (d)) |
        do_parse!(tag!(b"u") >> d: remains >> (d.into()))
    )
);

// Remap error to remove reference to `data`
pub fn detach_result<'inp, 'out, O: 'out, E: 'out>(
    res: IResult<&'inp [u8], O, E>,
    rest: &'out [u8],
) -> IResult<&'out [u8], O, E> {
    match res {
        IResult::Done(_, o) => IResult::Done(rest, o),
        IResult::Incomplete(n) => IResult::Incomplete(n),
        IResult::Error(Err::Code(e))
        | IResult::Error(Err::Node(e, _))
        | IResult::Error(Err::Position(e, _))
        | IResult::Error(Err::NodePosition(e, ..)) => IResult::Error(Err::Code(e)),
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
fn be_u48(i: &[u8]) -> IResult<&[u8], u64> {
    if i.len() < 6 {
        IResult::Incomplete(Needed::Size(6))
    } else {
        let res = ((i[0] as u64) << 40)
            + ((i[1] as u64) << 32)
            + ((i[2] as u64) << 24)
            + ((i[3] as u64) << 16)
            + ((i[4] as u64) << 8)
            + ((i[5] as u64) << 0);
        IResult::Done(&i[6..], res)
    }
}

#[cfg(test)]
mod test {
    use super::{header, Features, Header, Version};
    use nom::IResult;

    #[test]
    fn test_header_0() {
        let d = [0x00, 0x00, 0x00, 0x00];
        assert_eq!(
            header(&d[..]),
            IResult::Done(
                &b""[..],
                Header {
                    version: Version::Revlog0,
                    features: Features::empty(),
                }
            )
        )
    }

    #[test]
    fn test_header_1() {
        let d = [0x00, 0x00, 0x00, 0x01];
        assert_eq!(
            header(&d[..]),
            IResult::Done(
                &b""[..],
                Header {
                    version: Version::RevlogNG,
                    features: Features::empty(),
                }
            )
        )
    }

    #[test]
    fn test_header_feat_1() {
        let d = [0x00, 0x01, 0x00, 0x01];
        assert_eq!(
            header(&d[..]),
            IResult::Done(
                &b""[..],
                Header {
                    version: Version::RevlogNG,
                    features: Features::INLINE,
                }
            )
        )
    }

    #[test]
    fn test_header_feat_2() {
        let d = [0x00, 0x02, 0x00, 0x01];
        assert_eq!(
            header(&d[..]),
            IResult::Done(
                &b""[..],
                Header {
                    version: Version::RevlogNG,
                    features: Features::GENERAL_DELTA,
                }
            )
        )
    }

    #[test]
    fn test_header_feat_3() {
        let d = [0x00, 0x03, 0x00, 0x01];
        assert_eq!(
            header(&d[..]),
            IResult::Done(
                &b""[..],
                Header {
                    version: Version::RevlogNG,
                    features: Features::INLINE | Features::GENERAL_DELTA,
                }
            )
        )
    }
}
