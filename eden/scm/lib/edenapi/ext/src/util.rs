/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use blake2::digest::Update;
use blake2::digest::VariableOutput;
use blake2::VarBlake2b;
use edenapi_types::ContentId;

pub fn calc_contentid(data: &[u8]) -> ContentId {
    let mut hash = VarBlake2b::new_keyed(b"content", ContentId::len());
    hash.update(data);
    let mut ret = [0u8; ContentId::len()];
    hash.finalize_variable(|res| {
        if let Err(e) = ret.as_mut().write_all(res) {
            panic!(
                "{}-byte array must work with {}-byte blake2b: {:?}",
                ContentId::len(),
                ContentId::len(),
                e
            );
        }
    });
    ContentId::from(ret)
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
