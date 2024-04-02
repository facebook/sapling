/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mercurial_types::Delta;
use mercurial_types::HgNodeHash;
use mercurial_types::NonRootMPath;
use mercurial_types::RevFlags;

pub mod packer;
pub mod unpacker;
pub use unpacker::CgVersion;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Section {
    Changeset,
    Manifest,
    Treemanifest,
    Filelog(NonRootMPath),
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

    use futures::StreamExt;
    use futures::TryStreamExt;
    use partial_io::quickcheck_types::GenWouldBlock;
    use partial_io::quickcheck_types::PartialWithErrors;
    use partial_io::PartialAsyncRead;
    use partial_io::PartialAsyncWrite;
    use quickcheck::Gen;
    use quickcheck::QuickCheck;
    use quickcheck::TestResult;
    use slog::o;
    use slog::Discard;
    use slog::Logger;
    use tokio_util::codec::FramedRead;
    use tokio_util::codec::FramedWrite;

    use super::*;
    use crate::chunk::ChunkDecoder;
    use crate::chunk::NewChunkEncoder;
    use crate::quickcheck_types::CgPartSequence;

    #[test]
    fn test_roundtrip() {
        // Each test case gets pretty big (O(size**2) parts (because of
        // filelogs), each part with O(size) deltas), so reduce the size a bit
        // and generate a smaller number of test cases.
        let gen = Gen::new(50);
        let mut quickcheck = QuickCheck::new().gen(gen).tests(50);
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
        let gen = Gen::new(200);
        let mut quickcheck = QuickCheck::new().gen(gen).tests(1);
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
        let fut = async move {
            // Encode this sequence.
            let cursor = Cursor::new(Vec::with_capacity(32 * 1024));
            let partial_write = PartialAsyncWrite::new(cursor, write_ops);
            let packer = packer::changegroup_packer(seq.to_stream());
            let mut sink = FramedWrite::new(partial_write, NewChunkEncoder);
            let unpacker_version = seq.version().clone();

            packer.forward(&mut sink).await.unwrap();

            let mut cursor = sink.into_inner().into_inner();

            // Decode it.
            cursor.set_position(0);

            let partial_read = PartialAsyncRead::new(cursor, read_ops);
            let chunks = FramedRead::new(partial_read, ChunkDecoder)
                .map_ok(|chunk| chunk.into_bytes().expect("expected normal chunk"));

            let logger = Logger::root(Discard, o!());
            let unpacker = unpacker::CgUnpacker::new(logger, unpacker_version);
            let part_stream = crate::utils::decode_stream(chunks, unpacker);

            let parts: Vec<_> = part_stream.try_collect().await.unwrap();
            if seq != parts[..] {
                TestResult::failed()
            } else {
                TestResult::passed()
            }
        };

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(fut)
    }
}
