/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use serde_derive::Deserialize;
use serde_derive::Serialize;

use crate::hgid::HgId;
use crate::path::RepoPathBuf;

#[derive(
    Clone,
    Debug,
    Default,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize
)]
#[cfg_attr(
    any(test, feature = "for-tests"),
    derive(quickcheck_arbitrary_derive::Arbitrary)
)]
pub struct Key {
    // Name is usually a file or directory path
    pub path: RepoPathBuf,
    // HgId is always a 20 byte hash. This will be changed to a fix length array later.
    #[serde(alias = "node")]
    #[serde(rename = "node")]
    pub hgid: HgId,
}

impl Key {
    pub fn new(path: RepoPathBuf, hgid: HgId) -> Self {
        Key { path, hgid }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", &self.hgid, self.path)
    }
}

#[cfg(any(test, feature = "for-tests"))]
pub mod mocks {
    use lazy_static::lazy_static;

    use super::Key;
    use crate::hgid::mocks::ONES;
    use crate::hgid::mocks::THREES;
    use crate::hgid::mocks::TWOS;
    use crate::testutil::*;

    lazy_static! {
        pub static ref FOO_KEY: Key = Key::new(repo_path_buf("foo"), ONES);
        pub static ref BAR_KEY: Key = Key::new(repo_path_buf("bar"), TWOS);
        pub static ref BAZ_KEY: Key = Key::new(repo_path_buf("baz"), THREES);
    }
}

#[cfg(test)]
mod tests {
    use mocks::*;

    use super::*;

    #[test]
    fn display_key() {
        let foo = "1111111111111111111111111111111111111111 foo";
        let bar = "2222222222222222222222222222222222222222 bar";
        let baz = "3333333333333333333333333333333333333333 baz";
        assert_eq!(format!("{}", &*FOO_KEY), foo);
        assert_eq!(format!("{}", &*BAR_KEY), bar);
        assert_eq!(format!("{}", &*BAZ_KEY), baz);
    }

    #[test]
    fn test_serde_with_using_cbor() {
        // Note: this test is for CBOR. Other serializers like mincode
        // or Thrift would have different backwards compatibility!
        use serde_cbor::de::from_slice as decode;
        use serde_cbor::ser::to_vec as encode;

        #[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
        struct Orig(
            #[serde(with = "crate::serde_with::key::tuple")] Key,
            #[serde(with = "crate::serde_with::key::tuple")] Option<Key>,
        );

        #[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
        struct Bytes(
            #[serde(with = "crate::serde_with::key::bytes")] Key,
            #[serde(with = "crate::serde_with::key::bytes")] Option<Key>,
        );

        #[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
        struct Hex(
            #[serde(with = "crate::serde_with::key::hex")] Key,
            #[serde(with = "crate::serde_with::key::hex")] Option<Key>,
        );

        let key: Key = Key {
            path: RepoPathBuf::from_string("foo".to_string()).unwrap(),
            hgid: crate::hgid::mocks::CS,
        };
        let orig = Orig(key.clone(), Some(key.clone()));
        let bytes = Bytes(key.clone(), Some(key.clone()));
        let hex = Hex(key.clone(), Some(key.clone()));

        let cbor_orig = encode(&orig).unwrap();
        let cbor_bytes = encode(&bytes).unwrap();
        let cbor_hex = encode(&hex).unwrap();

        assert_eq!(cbor_orig.len(), 113);
        assert_eq!(cbor_bytes.len(), 73);
        assert_eq!(cbor_hex.len(), 115);

        // Orig cannot decode bytes or hex.
        assert_eq!(&decode::<Orig>(&cbor_orig).unwrap().0, &key);
        decode::<Orig>(&cbor_bytes).unwrap_err();
        decode::<Orig>(&cbor_hex).unwrap_err();

        // Bytes can decode all 3 formats.
        assert_eq!(&decode::<Bytes>(&cbor_orig).unwrap().0, &key);
        assert_eq!(&decode::<Bytes>(&cbor_bytes).unwrap().0, &key);
        assert_eq!(&decode::<Bytes>(&cbor_hex).unwrap().0, &key);

        // Hex can decode all 3 formats.
        assert_eq!(&decode::<Hex>(&cbor_orig).unwrap().0, &key);
        assert_eq!(&decode::<Hex>(&cbor_bytes).unwrap().0, &key);
        assert_eq!(&decode::<Hex>(&cbor_hex).unwrap().0, &key);
    }
}
