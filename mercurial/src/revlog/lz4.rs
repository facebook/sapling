// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Support for lz4revlog

use super::parser::{detach_result, Error};
use nom::{self, IResult};
use pylz4;

pub fn lz4_decompress<P, R>(i: &[u8], parse: P) -> IResult<&[u8], R, Error>
where
    for<'a> P: Fn(&'a [u8]) -> IResult<&'a [u8], R, Error> + 'a,
{
    match pylz4::decompress(i) {
        Ok((decompressed, remains)) => detach_result(parse(&decompressed[..]), remains),
        Err(_err) => {
            return IResult::Error(nom::ErrorKind::Custom(super::parser::Badness::BadLZ4));
        }
    }
}
