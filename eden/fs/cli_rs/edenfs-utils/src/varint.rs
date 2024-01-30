/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Read;

use byteorder::BigEndian;
use byteorder::ReadBytesExt;

#[allow(dead_code)]
const VARINT_MAX_BYTES: usize = 5;

#[allow(dead_code)]
pub fn encode_varint(value: u32) -> Result<([u8; VARINT_MAX_BYTES], usize), std::io::Error> {
    let mut count = 0;
    let mut v = value;
    let mut buffer = [0u8; VARINT_MAX_BYTES];
    loop {
        let byte = v & 0x7F;
        v >>= 7;
        if v == 0 {
            buffer[count] = byte as u8;
            count += 1;
            break;
        } else {
            buffer[count] = (byte | 0x80) as u8;
        }
        count += 1;
    }
    Ok((buffer, count))
}

#[allow(dead_code)]
pub fn decode_varint(buf: &mut impl Read) -> Result<(u32, usize), std::io::Error> {
    let mut value = 0;
    let mut shift = 0;
    let mut bytes_read = 0;
    loop {
        bytes_read += 1;
        let byte = buf.read_uint::<BigEndian>(1)?;
        value |= (byte & 0x7F) << shift;
        shift += 7;
        if (byte & 0x80) == 0 {
            break;
        }
    }
    assert!(bytes_read <= VARINT_MAX_BYTES);
    Ok((value as u32, bytes_read))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::BufReader;

    use lazy_static::lazy_static;

    lazy_static! {
        // (unencoded int, (encoded bytes, encoded length))
        static ref CASES: HashMap<u32, ([u8; VARINT_MAX_BYTES], usize)> = HashMap::from([
            (0, ([0x0, 0x0, 0x0, 0x0, 0x0], 1)),
            (1, ([0x1, 0x0, 0x0, 0x0, 0x0], 1)),
            (128, ([0x80, 0x1, 0x0, 0x0, 0x0], 2)),
            (u32::MAX, ([0xff, 0xff, 0xff, 0xff, 0x0f], 5)),
        ]);
    }

    use super::*;

    #[test]
    fn test_encode_varints() {
        for (input, output) in CASES.iter() {
            let res = encode_varint(*input).unwrap();
            assert_eq!(res, *output);
        }
    }

    #[test]
    fn test_decode_varints() {
        for (output, input) in CASES.iter() {
            let res = decode_varint(&mut BufReader::new(input.0.as_ref())).unwrap();
            assert_eq!(res, (*output, input.1));
        }
    }
}
