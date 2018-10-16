// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Support for lz4revlog

use super::parser::{detach_result, Error};
use nom::{self, IResult};
use pylz4;

pub fn lz4_decompress<'a, P, R: 'a>(i: &'a [u8], parse: P) -> IResult<&'a [u8], R, Error>
where
    for<'p> P: Fn(&'p [u8]) -> IResult<&'p [u8], R, Error>,
{
    match pylz4::decompress(i) {
        Ok((decompressed, remains)) => detach_result(parse(&decompressed[..]), remains),
        Err(_err) => {
            return IResult::Error(nom::Err::Code(nom::ErrorKind::Custom(
                super::parser::Badness::BadLZ4,
            )));
        }
    }
}
