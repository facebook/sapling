/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blake2::digest::FixedOutput;
use blake2::digest::Mac;
use blake2::Blake2bMac;
use edenapi_types::ContentId;

pub fn calc_contentid(data: &[u8]) -> ContentId {
    let mut hash = Blake2bMac::new_from_slice(b"content").expect("key to be less than 32 bytes");
    hash.update(data);
    let mut ret = [0; ContentId::len()];
    hash.finalize_into((&mut ret).into());
    ContentId::from_byte_array(ret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_blake2() {
        #[rustfmt::skip]
        assert_eq!(
            calc_contentid(b"abc"),
            ContentId::from([
                0x22, 0x8d, 0x7e, 0xfd, 0x5e, 0x3c, 0x1a, 0xcd,
                0xf4, 0x0e, 0x52, 0x43, 0x3f, 0x72, 0x8f, 0x53,
                0x78, 0x90, 0x0e, 0x41, 0xd4, 0xea, 0xe7, 0x14,
                0x64, 0x1f, 0x6f, 0x04, 0x0d, 0xee, 0x69, 0x3e,
            ])
        );
    }
}
