/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::io::Cursor;

use bytes::{Buf, Bytes};

pub trait BufExt: Buf {
    /// Reset buffer back to the beginning.
    fn reset(self) -> Self;
}

impl BufExt for Cursor<Bytes> {
    fn reset(self) -> Self {
        Cursor::new(self.into_inner())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_bytes_reset() {
        let b = Bytes::from(b"hello, world".to_vec());
        let mut c = Cursor::new(b);

        assert_eq!(c.remaining(), 12);
        assert_eq!(c.get_u8(), b'h');

        c.advance(5);
        assert_eq!(c.remaining(), 6);
        assert_eq!(c.get_u8(), b' ');

        let mut c = c.reset();
        assert_eq!(c.remaining(), 12);
        assert_eq!(c.get_u8(), b'h');
    }

    #[test]
    fn test_empty_bytes_reset() {
        let b = Bytes::from(Vec::new());
        let c = Cursor::new(b);

        assert_eq!(c.remaining(), 0);

        let c = c.reset();
        assert_eq!(c.remaining(), 0);
    }
}
