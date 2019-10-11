/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

// Support for lz4revlog

use super::parser::{detach_result, Error};
use nom::{self, IResult};

pub fn lz4_decompress<'a, P, R: 'a>(i: &'a [u8], parse: P) -> IResult<&'a [u8], R, Error>
where
    for<'p> P: Fn(&'p [u8]) -> IResult<&'p [u8], R, Error>,
{
    // A previous implementation of lz4 decompress returned a remaining set of data, but the
    // implementation was incorrect and remains was always an empty byte array. On top of that
    // LZ4 compression doesn't have the concept of knowing when it's decompression stream ends,
    // so the concept of 'remaining' doesn't exist for LZ4 unless we enrich the data stream - which
    // wasn't happening. This is left here to remain a compatible interface with ZSTD compression
    // which does use remains for detach_result.
    let remains: &[u8] = &[];

    match lz4_pyframe::decompress(i) {
        Ok(decompressed) => detach_result(parse(&decompressed[..]), &remains),
        Err(_err) => {
            return IResult::Error(nom::Err::Code(nom::ErrorKind::Custom(
                super::parser::Badness::BadLZ4,
            )));
        }
    }
}
