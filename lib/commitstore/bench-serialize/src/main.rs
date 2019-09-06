// Copyright (c) Facebook, Inc. and its affiliates.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2.

use abomonation_derive::Abomonation;
use minibench::{bench, elapsed, measure, Measure};
use serde_derive::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Abomonation, Clone)]
struct Node([u8; 20]);

impl From<[u8; 20]> for Node {
    fn from(value: [u8; 20]) -> Node {
        Node(value)
    }
}

impl AsRef<[u8]> for Node {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

#[derive(Serialize, Deserialize, Debug, Abomonation, Clone)]
struct Commit {
    node: Node,

    parents: Vec<Node>,

    #[serde(with = "serde_bytes")]
    metadata: Vec<u8>,
}

fn main() {
    let commits: Vec<Commit> = (1..100000u32)
        .map(|i| Commit {
            node: [i as u8; 20].into(),
            parents: (0..(i % 3)).map(|j| [j as u8; 20].into()).collect(),
            metadata: format!("Commit {}", i).as_bytes().to_vec(),
        })
        .collect();

    let mut out = Vec::with_capacity(1024);
    let mut bench_algorithm =
        |name: &str, ser: fn(&Commit, &mut Vec<u8>), de: fn(&mut [u8]) -> Commit| {
            bench(format!("serialize   by {}", name), || {
                <measure::Both<measure::WallClock, measure::Bytes>>::measure(|| {
                    let bytes: usize = commits
                        .iter()
                        .map(|commit| {
                            out.clear();
                            ser(commit, &mut out);
                            out.len()
                        })
                        .sum();
                    bytes as u64
                })
            });

            let mut serialized: Vec<Vec<u8>> = {
                commits
                    .iter()
                    .map(|commit| {
                        out.clear();
                        ser(commit, &mut out);
                        out.clone()
                    })
                    .collect()
            };
            bench(format!("deserialize by {}", name), || {
                elapsed(|| {
                    for mut data in &mut serialized {
                        let _ = de(&mut data);
                    }
                })
            });
        };

    bench_algorithm(
        "cbor",
        |c, w| serde_cbor::ser::to_writer(w, c).unwrap(),
        |d| serde_cbor::de::from_slice(d).unwrap(),
    );

    bench_algorithm(
        "cbor-packed",
        |c, w| serde_cbor::ser::to_writer_packed(w, c).unwrap(),
        |d| serde_cbor::de::from_slice(d).unwrap(),
    );

    bench_algorithm(
        "bincode",
        |c, w| bincode::serialize_into(w, c).unwrap(),
        |d| bincode::deserialize(d).unwrap(),
    );

    bench_algorithm(
        "mincode",
        |c, w| mincode::serialize_into(w, c).unwrap(),
        |d| mincode::deserialize(d).unwrap(),
    );

    bench_algorithm(
        "handwritten",
        |c, w| handwritten::encode(w, c),
        |d| handwritten::decode(d),
    );

    bench_algorithm(
        "abomonation",
        |c, w| unsafe { abomonation::encode(c, w).unwrap() },
        |d| unsafe { abomonation::decode::<Commit>(d).unwrap().0.clone() },
    );
}

mod handwritten {
    // The handwritten format is simple:
    // - [u8; N] gets serialized into N bytes.
    // - integers are serialized using vlqencoding.
    // - Vec<u8> gets serialized as length using vlqencoding,
    //   followed by the raw bytes.
    // The format is probably similar to mincode [1].
    // [1]: bincode, but using vlqencoding for integers.
    //      https://github.com/Boscop/mincode

    use super::*;
    use std::io::{Cursor, Read, Write};
    use vlqencoding::{VLQDecode, VLQEncode};

    pub(crate) fn encode(result: &mut Vec<u8>, commit: &Commit) {
        result.write_all(&commit.node.as_ref()).unwrap();
        result.write_vlq(commit.parents.len()).unwrap();
        for parent in &commit.parents {
            result.write_all(parent.as_ref()).unwrap();
        }
        result.write_vlq(commit.metadata.len()).unwrap();
        result.write_all(&commit.metadata).unwrap();
    }

    pub(crate) fn decode(buf: &[u8]) -> Commit {
        let mut reader = Cursor::new(buf);
        let mut node = [0u8; 20];
        reader.read_exact(&mut node).unwrap();
        let parent_len = reader.read_vlq().unwrap();
        let mut parents = Vec::with_capacity(parent_len);
        for _ in 0..parent_len {
            let mut parent = [0u8; 20];
            reader.read_exact(&mut parent).unwrap();
            parents.push(parent.into());
        }
        let meta_len = reader.read_vlq().unwrap();
        let mut metadata = Vec::with_capacity(meta_len);
        reader.read_exact(&mut metadata).unwrap();
        Commit {
            node: node.into(),
            parents,
            metadata,
        }
    }
}
