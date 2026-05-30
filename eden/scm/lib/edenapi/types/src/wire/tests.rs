/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

pub(crate) mod support {
    pub(crate) use quickcheck::Arbitrary;
    pub(crate) use quickcheck::Gen;
    pub(crate) use quickcheck::quickcheck;
    pub(crate) use serde_json;

    pub(crate) use crate::wire::ToApi;
    pub(crate) use crate::wire::ToWire;

    pub(crate) fn check_wire_roundtrip<T>(original: T) -> bool
    where
        T: ToWire + Clone + PartialEq,
        <<T as ToWire>::Wire as ToApi>::Error: std::fmt::Debug,
    {
        let wire = original.clone().to_wire();
        let roundtrip = wire.to_api().unwrap();
        original == roundtrip
    }

    fn json_hash(json: &str) -> u64 {
        json.bytes().fold(0xcbf29ce484222325, |hash, byte| {
            hash.wrapping_mul(0x100000001b3) ^ u64::from(byte)
        })
    }

    pub(crate) fn wire_json_hash<Wire>() -> u64
    where
        Wire: ToApi + Arbitrary + serde::Serialize,
        <Wire as ToApi>::Api: ToWire + Clone + PartialEq + Arbitrary + std::fmt::Debug + 'static,
        <<Wire as ToApi>::Api as ToWire>::Wire: ToApi,
        <<<Wire as ToApi>::Api as ToWire>::Wire as ToApi>::Error: std::fmt::Debug,
    {
        let mut g = Gen::from_size_and_seed(5, 42);
        let json = serde_json::to_string(&Wire::arbitrary(&mut g)).unwrap();

        println!("Checking wire roundtrip");
        quickcheck(
            check_wire_roundtrip::<<Wire as ToApi>::Api> as fn(<Wire as ToApi>::Api) -> bool,
        );

        json_hash(&json)
    }
}

macro_rules! wire_json_hashes {
    ($($wire: ty),* $(,)?) => {{
        vec![$($crate::wire::tests::support::wire_json_hash::<$wire>()),*]
    }};
}
pub(crate) use wire_json_hashes;
