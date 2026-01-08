/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use types::Id20;

/// Extracted fields embedded in an [`Id20`].
///
/// | Field | TYPE   | FACTOR_BITS | ID      | EXTRA    |
/// | Width | 2 bits | 6 bits      | 8 bytes | 11 bytes |
///
/// The `EXTRA` field can be used to store extra configuration. For blobs,
/// it contains 8-byte SALT:
///
/// | Field | SALT    | RESERVED |
/// | Width | 8 bytes | 3 bytes  |
///
/// For commits and trees, `EXTRA` is currently `RESERVED`:
///
/// | Field | RESERVED  |
/// | Width | 11 bytes  |
///
/// The `RESERVED` field might be used for other purposes in the future.
/// Currently, it's all 0s.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct IdFields {
    pub kind: ObjectKind,

    /// `factor_bits` decides the size of the repo. See also related code in
    /// `virtual_tree`. Example sizes:
    ///
    /// | bits | Commits | Files |  Dirs |
    /// |------|---------|-------|-------|
    /// |    6 |    1.9M |  0.9M |  0.1M |
    /// |    7 |    3.9M |  1.8M |  0.2M |
    /// |    8 |    7.8M |  3.6M |  0.5M |
    /// |    9 |   15.6M |  7.3M |  0.9M |
    /// |   10 |   31.2M | 14.7M |  1.8M |
    /// |   11 |   62.4M | 29.3M |  3.6M |
    pub factor_bits: u8,

    /// The actual u64 id used by `virtual_tree`.
    pub id8: u64,
}

const OFFSET_ID8: usize = 1;
const OFFSET_EXTRA: usize = OFFSET_ID8 + 8;

// "BLOB" objects use EXTRA as SALT + RESERVED. See also into_id20_with_salt.
const OFFSET_BLOB_SALT: usize = OFFSET_EXTRA;
const OFFSET_BLOB_RESERVED: usize = OFFSET_BLOB_SALT + 8;

// Non-blob objects use EXTRA as RESERVED
const OFFSET_OTHER_RESERVED: usize = OFFSET_EXTRA;

impl IdFields {
    /// Extract fields from a compatible `Id20`.
    pub fn maybe_from_id20(id20: Id20) -> Option<Self> {
        let bytes = id20.into_byte_array();
        let kind = match bytes[0] & 0b1100_0000 {
            0 => ObjectKind::Blob,
            0b0100_0000 => ObjectKind::SymlinkBlob,
            0b1000_0000 => ObjectKind::Tree,
            0b1100_0000 => ObjectKind::Commit,
            _ => return None,
        };
        let factor_bits = bytes[0] & 0x3f;
        let reserved_start = match kind {
            ObjectKind::Blob | ObjectKind::SymlinkBlob => OFFSET_BLOB_RESERVED,
            _ => OFFSET_OTHER_RESERVED,
        };
        let reserved = &bytes[reserved_start..];
        if reserved.iter().any(|v| *v != 0) {
            return None;
        }
        let id8 = u64::from_le_bytes(bytes[OFFSET_ID8..OFFSET_EXTRA].try_into().unwrap());
        Some(Self {
            kind,
            factor_bits,
            id8,
        })
    }

    /// Generate another `IdFields` with the same `factor_bits` but specified
    /// `kind` and `id8`.
    pub fn with_kind_id8(&self, kind: ObjectKind, id8: u64) -> Self {
        Self {
            kind,
            factor_bits: self.factor_bits,
            id8,
        }
    }

    /// Similar to `into()`, but also mix-in extra `salt`.
    /// Intended to be only used by `Blob` and `SymlinkBlob` types.
    pub fn into_id20_with_salt(self, salt: u64) -> Id20 {
        assert!(matches!(
            self.kind,
            ObjectKind::Blob | ObjectKind::SymlinkBlob
        ));
        let id20 = Id20::from(self);
        let mut id20_array = id20.into_byte_array();
        id20_array[OFFSET_BLOB_SALT..OFFSET_BLOB_RESERVED].copy_from_slice(&salt.to_le_bytes());
        Id20::from_byte_array(id20_array)
    }

    /// Check if two (usually commits) are compatible. This means they share
    /// a same `factor_bits`. If we add more info in `RESERVED` in the future,
    /// check the `RESERVED` too.
    pub fn is_compatible_with(&self, other: &Self) -> bool {
        self.factor_bits == other.factor_bits
    }
}

impl From<IdFields> for Id20 {
    fn from(id_fields: IdFields) -> Self {
        let mut bytes: [u8; _] = [0; Id20::len()];
        bytes[0] = ((id_fields.kind as u8) << 6) | id_fields.factor_bits;
        (bytes[OFFSET_ID8..OFFSET_EXTRA]).copy_from_slice(&id_fields.id8.to_le_bytes());
        Id20::from_byte_array(bytes)
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[repr(u8)]
pub enum ObjectKind {
    Blob = 0,
    SymlinkBlob = 1,
    Tree = 2,
    Commit = 3,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_roundtrip() {
        for kind in [
            ObjectKind::Blob,
            ObjectKind::SymlinkBlob,
            ObjectKind::Tree,
            ObjectKind::Commit,
        ] {
            let fields = IdFields {
                kind,
                factor_bits: 15,
                id8: 12345678,
            };
            let id20 = Id20::from(fields);
            let fields2 = IdFields::maybe_from_id20(id20).unwrap();
            assert_eq!(fields2, fields);
        }
    }

    #[test]
    fn test_with_salt_roundtrip() {
        for kind in [ObjectKind::Blob, ObjectKind::SymlinkBlob] {
            let fields = IdFields {
                kind,
                factor_bits: 34,
                id8: 0xabcdeeff11223344,
            };
            let id20 = fields.into_id20_with_salt(0xfedcba0987654321);
            let fields2 = IdFields::maybe_from_id20(id20).unwrap();
            assert_eq!(fields2, fields);
        }
    }
}
