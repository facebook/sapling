// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use mercurial_types::{Delta, HgNodeHash, MPath};

pub mod packer;
pub mod unpacker;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Section {
    Changeset,
    Manifest,
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
}

#[cfg(test)]
mod test {
    use std::io::{self, Cursor};

    use futures::Stream;
    use quickcheck::{QuickCheck, StdGen, TestResult};
    use quickcheck::rand;
    use slog::{Drain, Logger};
    use slog_term;
    use tokio_codec::{FramedRead, FramedWrite};
    use tokio_core::reactor::Core;

    use futures_ext::StreamLayeredExt;
    use partial_io::{GenWouldBlock, PartialAsyncRead, PartialAsyncWrite, PartialWithErrors};

    use chunk::{ChunkDecoder, ChunkEncoder};
    use quickcheck_types::Cg2PartSequence;

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
                    Cg2PartSequence,
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
                    Cg2PartSequence,
                    PartialWithErrors<GenWouldBlock>,
                    PartialWithErrors<GenWouldBlock>,
                ) -> TestResult,
        );
    }

    fn roundtrip(
        seq: Cg2PartSequence,
        write_ops: PartialWithErrors<GenWouldBlock>,
        read_ops: PartialWithErrors<GenWouldBlock>,
    ) -> TestResult {
        // Encode this sequence.
        let cursor = Cursor::new(Vec::with_capacity(32 * 1024));
        let partial_write = PartialAsyncWrite::new(cursor, write_ops);
        let packer = packer::Cg2Packer::new(seq.to_stream().and_then(|x| x));
        let sink = FramedWrite::new(partial_write, ChunkEncoder);
        let encode_fut = packer.forward(sink);

        let mut core = Core::new().unwrap();
        let (_, sink) = core.run(encode_fut).unwrap();
        let mut cursor = sink.into_inner().into_inner();

        // Decode it.
        cursor.set_position(0);

        let partial_read = PartialAsyncRead::new(cursor, read_ops);
        let chunks = FramedRead::new(partial_read, ChunkDecoder)
            .map(|chunk| chunk.into_bytes().expect("expected normal chunk"));

        let logger = make_root_logger();
        let unpacker = unpacker::Cg2Unpacker::new(logger);
        let part_stream = chunks.decode(unpacker);

        let parts = Vec::new();
        let decode_fut = part_stream
            .map_err(|e| -> () { panic!("unexpected error: {}", e) })
            .forward(parts);

        let (_, parts) = core.run(decode_fut).unwrap();

        if seq != parts[..] {
            return TestResult::failed();
        }

        TestResult::passed()
    }

    fn make_root_logger() -> Logger {
        let plain = slog_term::PlainSyncDecorator::new(io::stdout());
        Logger::root(slog_term::FullFormat::new(plain).build().fuse(), o!())
    }
}
