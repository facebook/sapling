/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use mercurial_types::{Delta, HgNodeHash, MPath, RevFlags};

pub mod packer;
pub mod unpacker;
pub use unpacker::CgVersion;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Section {
    Changeset,
    Manifest,
    Treemanifest,
    Filelog(MPath),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Part {
    CgChunk(Section, CgDeltaChunk),
    SectionEnd(Section),
    End,
}

impl Part {
    pub fn is_section_end(&self) -> bool {
        match self {
            &Part::SectionEnd(_) => true,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CgDeltaChunk {
    pub node: HgNodeHash,
    pub p1: HgNodeHash,
    pub p2: HgNodeHash,
    pub base: HgNodeHash,
    pub linknode: HgNodeHash,
    pub delta: Delta,
    pub flags: Option<RevFlags>,
}

#[cfg(test)]
mod test {
    use std::io::Cursor;

    use futures::{Future, Stream};
    use quickcheck::{QuickCheck, StdGen, TestResult};
    use tokio_codec::{FramedRead, FramedWrite};

    use futures_ext::StreamLayeredExt;
    use partial_io::{GenWouldBlock, PartialAsyncRead, PartialAsyncWrite, PartialWithErrors};

    use crate::chunk::{ChunkDecoder, ChunkEncoder};
    use crate::quickcheck_types::CgPartSequence;
    use slog::{o, Discard, Logger};

    use super::*;

    #[test]
    fn test_roundtrip() {
        // Each test case gets pretty big (O(size**2) parts (because of
        // filelogs), each part with O(size) deltas), so reduce the size a bit
        // and generate a smaller number of test cases.
        let rng = StdGen::new(rand::thread_rng(), 50);
        let mut quickcheck = QuickCheck::new().gen(rng).tests(50);
        // Use NoErrors here because:
        // - AsyncWrite impls aren't supposed to return Interrupted errors.
        // - WouldBlock would require parking and unparking the task, which
        //   isn't yet supported in partial-io.
        quickcheck.quickcheck(
            roundtrip
                as fn(
                    CgPartSequence,
                    PartialWithErrors<GenWouldBlock>,
                    PartialWithErrors<GenWouldBlock>,
                ) -> TestResult,
        );
    }

    #[test]
    fn test_roundtrip_giant() {
        // Test a smaller number of cases with much larger inputs.
        let rng = StdGen::new(rand::thread_rng(), 200);
        let mut quickcheck = QuickCheck::new().gen(rng).tests(1);
        quickcheck.quickcheck(
            roundtrip
                as fn(
                    CgPartSequence,
                    PartialWithErrors<GenWouldBlock>,
                    PartialWithErrors<GenWouldBlock>,
                ) -> TestResult,
        );
    }

    fn roundtrip(
        seq: CgPartSequence,
        write_ops: PartialWithErrors<GenWouldBlock>,
        read_ops: PartialWithErrors<GenWouldBlock>,
    ) -> TestResult {
        // Encode this sequence.
        let cursor = Cursor::new(Vec::with_capacity(32 * 1024));
        let partial_write = PartialAsyncWrite::new(cursor, write_ops);
        let packer = packer::CgPacker::new(seq.to_stream().and_then(|x| x));
        let sink = FramedWrite::new(partial_write, ChunkEncoder);
        let unpacker_version = seq.version().clone();

        let fut = packer
            .forward(sink)
            .map_err(|e| -> () { panic!("unexpected error: {}", e) })
            .and_then(move |(_, sink)| {
                let mut cursor = sink.into_inner().into_inner();

                // Decode it.
                cursor.set_position(0);

                let partial_read = PartialAsyncRead::new(cursor, read_ops);
                let chunks = FramedRead::new(partial_read, ChunkDecoder)
                    .map(|chunk| chunk.into_bytes().expect("expected normal chunk"));

                let logger = Logger::root(Discard, o!());
                let unpacker = unpacker::CgUnpacker::new(logger, unpacker_version);
                let part_stream = chunks.decode(unpacker);

                let parts = Vec::new();
                part_stream
                    .map_err(|e| -> () { panic!("unexpected error: {}", e) })
                    .forward(parts)
            })
            .map(move |(_, parts)| {
                if seq != parts[..] {
                    return TestResult::failed();
                } else {
                    TestResult::passed()
                }
            });

        let mut runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime.block_on(fut);
        runtime.shutdown_on_idle();
        result.unwrap()
    }
}
