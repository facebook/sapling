// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::convert::From;
use std::fmt::Debug;
use std::io::{self, Cursor};
use std::str::FromStr;

use futures::stream::Stream;
use slog::{Drain, Logger};
use slog_term;
use tokio_core::reactor::Core;
use tokio_io::AsyncRead;

use async_compression::{CompressorType, ZSTD_DEFAULT_LEVEL};
use async_compression::membuf::MemBuf;
use mercurial_types::{NodeHash, Path, NULL_HASH};
use partial_io::{GenWouldBlock, PartialAsyncRead, PartialWithErrors};
use quickcheck::{QuickCheck, StdGen};
use rand;

use Bundle2Item;
use bundle2::Bundle2Stream;
use bundle2_encode::Bundle2EncodeBuilder;
use changegroup;
use errors::*;
use part_encode::PartEncodeBuilder;
use part_header::PartHeaderBuilder;
use types::StreamHeader;
use utils::get_compression_param;

const BZIP2_BUNDLE2: &[u8] = include_bytes!("fixtures/bzip2.bin");
const UNCOMP_BUNDLE2: &[u8] = include_bytes!("fixtures/uncompressed.bin");
const UNKNOWN_COMPRESSION_BUNDLE2: &[u8] = include_bytes!(
    "fixtures/unknown-compression.\
     bin"
);

const CHANGESET1_HASH_STR: &str = "b2040b24fd5cdfaf36e3164ddc357e834167b14a";
const CHANGESET2_HASH_STR: &str = "415ab71954c98ea93dab4b8f61f04ca57bc5c33c";
const MANIFEST1_HASH_STR: &str = "afcff2144f55cfa5d9b04ac4ed6598f26035aa77";
const MANIFEST2_HASH_STR: &str = "aa93dc3435cbfecd0c4c245b80b2a0b9ed35a015";
const ABC_HASH_STR: &str = "b80de5d138758541c5f05265ad144ab9fa86d1db";
const DEF_HASH_STR: &str = "bb969a19e8853962b4347bea4c24796324f10d8b";

#[test]
fn test_parse_bzip2() {
    let rng = StdGen::new(rand::thread_rng(), 20);
    let mut quickcheck = QuickCheck::new().gen(rng);
    quickcheck.quickcheck(parse_bzip2 as fn(PartialWithErrors<GenWouldBlock>) -> ());
}

fn parse_bzip2(read_ops: PartialWithErrors<GenWouldBlock>) {
    parse_bundle(BZIP2_BUNDLE2, Some("BZ"), read_ops);
}

#[test]
fn test_parse_uncompressed() {
    let rng = StdGen::new(rand::thread_rng(), 20);
    let mut quickcheck = QuickCheck::new().gen(rng);
    quickcheck.quickcheck(
        parse_uncompressed as fn(PartialWithErrors<GenWouldBlock>) -> (),
    );
}

fn parse_uncompressed(read_ops: PartialWithErrors<GenWouldBlock>) {
    parse_bundle(UNCOMP_BUNDLE2, None, read_ops);
}

#[test]
fn test_parse_unknown_compression() {
    let mut core = Core::new().unwrap();
    let bundle2_buf = MemBuf::from(Vec::from(UNKNOWN_COMPRESSION_BUNDLE2));
    let outer_stream_err = parse_stream_start(&mut core, bundle2_buf, Some("IL")).unwrap_err();
    assert_matches!(outer_stream_err.kind(),
                    &ErrorKind::Bundle2Decode(ref msg) if msg == "unknown compression 'IL'");
}

#[test]
fn test_empty_bundle_roundtrip_zstd() {
    empty_bundle_roundtrip(CompressorType::Zstd {
        level: ZSTD_DEFAULT_LEVEL,
    });
}

#[test]
fn test_empty_bundle_roundtrip_uncompressed() {
    empty_bundle_roundtrip(CompressorType::Uncompressed);
}

fn empty_bundle_roundtrip(ct: CompressorType) {
    // Encode an empty bundle.
    let cursor = Cursor::new(Vec::with_capacity(32 * 1024));
    let mut builder = Bundle2EncodeBuilder::new(cursor);
    builder.set_compressor_type(ct);
    builder
        .add_stream_param("Foo".into(), "123".into())
        .unwrap();
    builder
        .add_stream_param("bar".into(), "456".into())
        .unwrap();
    let encode_fut = builder.build();

    let mut core = Core::new().unwrap();
    let mut buf = core.run(encode_fut).unwrap();
    buf.set_position(0);

    // Now decode it.
    let logger = make_root_logger();
    let stream = Bundle2Stream::new(buf, logger);
    let (item, stream) = core.run(stream.into_future()).unwrap();

    let mut mparams = HashMap::new();
    let mut aparams = HashMap::new();
    mparams.insert("foo".into(), "123".into());
    mparams.insert("compression".into(), get_compression_param(&ct).into());
    aparams.insert("bar".into(), "456".into());
    let expected_header = StreamHeader {
        m_stream_params: mparams,
        a_stream_params: aparams,
    };

    assert_eq!(item, Some(Bundle2Item::Start(expected_header)));

    let (item, _stream) = core.run(stream.into_future()).unwrap();
    assert!(item.is_none());
}

#[test]
fn test_unknown_part_zstd() {
    unknown_part(CompressorType::Zstd {
        level: ZSTD_DEFAULT_LEVEL,
    });
}

#[test]
fn test_unknown_part_uncompressed() {
    unknown_part(CompressorType::Uncompressed);
}

fn unknown_part(ct: CompressorType) {
    let cursor = Cursor::new(Vec::with_capacity(32 * 1024));
    let mut builder = Bundle2EncodeBuilder::new(cursor);

    builder.set_compressor_type(ct);

    let unknown_part = PartEncodeBuilder::mandatory("unknown:unknown").unwrap();

    builder.add_part(unknown_part);
    let encode_fut = builder.build();

    let mut core = Core::new().unwrap();
    let mut buf = core.run(encode_fut).unwrap();
    buf.set_position(0);

    let logger = make_root_logger();
    let stream = Bundle2Stream::new(buf, logger);
    let parts = Vec::new();

    let decode_fut = stream
        .map_err(|e| -> () { panic!("unexpected error: {}", e) })
        .forward(parts);
    let (stream, parts) = core.run(decode_fut).unwrap();

    // Only the stream header should have been returned.
    let mut m_stream_params = HashMap::new();
    m_stream_params.insert("compression".into(), get_compression_param(&ct).into());
    let expected = StreamHeader {
        m_stream_params: m_stream_params,
        a_stream_params: HashMap::new(),
    };
    assert_eq!(parts, vec![Bundle2Item::Start(expected)]);

    // Make sure the error was accumulated.
    let stream = stream.into_inner();
    let app_errors = stream.app_errors();
    assert_eq!(app_errors.len(), 1);
    assert_matches!(app_errors[0].kind(),
                    &ErrorKind::BundleUnknownPart(ref header)
                    if header.part_type() == "UNKNOWN:UNKNOWN");
}

fn parse_bundle(
    input: &[u8],
    compression: Option<&str>,
    read_ops: PartialWithErrors<GenWouldBlock>,
) {
    let mut core = Core::new().unwrap();

    let bundle2_buf = MemBuf::from(Vec::from(input));
    let partial_read = PartialAsyncRead::new(bundle2_buf, read_ops);
    let stream = parse_stream_start(&mut core, partial_read, compression).unwrap();

    let (res, stream) = core.next_stream(stream);
    let res = res.unwrap();

    let mut header = PartHeaderBuilder::new("CHANGEGROUP").unwrap();
    header.add_mparam("version", "02").unwrap();
    header.add_aparam("nbchanges", "2").unwrap();
    let header = header.build(0);
    assert_eq!(res, Bundle2Item::Header(header));

    let stream = verify_cg2(&mut core, stream);

    let (res, stream) = core.next_stream(stream);
    assert!(res.is_none());

    // Make sure the stream is fused.
    let (res, _) = core.next_stream(stream);
    assert!(res.is_none());
}

fn verify_cg2<R: AsyncRead>(core: &mut Core, stream: Bundle2Stream<R>) -> Bundle2Stream<R> {
    let (res, stream) = next_cg2_part(core, stream);
    assert_eq!(*res.section(), changegroup::Section::Changeset);
    let chunk = res.chunk();

    // Verify that changesets parsed correctly.
    let changeset1_hash = NodeHash::from_str(CHANGESET1_HASH_STR).unwrap();
    assert_eq!(chunk.node, changeset1_hash);
    assert_eq!(chunk.p1, NULL_HASH);
    assert_eq!(chunk.p2, NULL_HASH);
    assert_eq!(chunk.base, NULL_HASH);
    assert_eq!(chunk.linknode, changeset1_hash);
    let frags = chunk.delta.fragments();
    assert_eq!(frags.len(), 1);
    assert_eq!(frags[0].start, 0);
    assert_eq!(frags[0].end, 0);
    assert_eq!(frags[0].content.len(), 98);

    let (res, stream) = next_cg2_part(core, stream);
    assert_eq!(*res.section(), changegroup::Section::Changeset);
    let chunk = res.chunk();

    let changeset2_hash = NodeHash::from_str(CHANGESET2_HASH_STR).unwrap();
    assert_eq!(chunk.node, changeset2_hash);
    assert_eq!(chunk.p1, changeset1_hash);
    assert_eq!(chunk.p2, NULL_HASH);
    assert_eq!(chunk.base, NULL_HASH);
    assert_eq!(chunk.linknode, changeset2_hash);
    let frags = chunk.delta.fragments();
    assert_eq!(frags.len(), 1);
    assert_eq!(frags[0].start, 0);
    assert_eq!(frags[0].end, 0);
    assert_eq!(frags[0].content.len(), 102);

    let (res, stream) = next_cg2_part(core, stream);
    assert_matches!(
        res,
        changegroup::Part::SectionEnd(changegroup::Section::Changeset)
    );

    // Verify basic properties of manifests.
    let (res, stream) = next_cg2_part(core, stream);
    assert_eq!(*res.section(), changegroup::Section::Manifest);
    let chunk = res.chunk();

    let manifest1_hash = NodeHash::from_str(MANIFEST1_HASH_STR).unwrap();
    assert_eq!(chunk.node, manifest1_hash);
    assert_eq!(chunk.p1, NULL_HASH);
    assert_eq!(chunk.p2, NULL_HASH);
    assert_eq!(chunk.base, NULL_HASH);
    assert_eq!(chunk.linknode, changeset1_hash);

    let (res, stream) = next_cg2_part(core, stream);
    assert_eq!(*res.section(), changegroup::Section::Manifest);
    let chunk = res.chunk();

    let manifest2_hash = NodeHash::from_str(MANIFEST2_HASH_STR).unwrap();
    assert_eq!(chunk.node, manifest2_hash);
    assert_eq!(chunk.p1, manifest1_hash);
    assert_eq!(chunk.p2, NULL_HASH);
    assert_eq!(chunk.base, manifest1_hash); // In this case there's a delta.
    assert_eq!(chunk.linknode, changeset2_hash);

    let (res, stream) = next_cg2_part(core, stream);
    assert_matches!(
        res,
        changegroup::Part::SectionEnd(changegroup::Section::Manifest)
    );

    // Filelog section
    let (res, stream) = next_cg2_part(core, stream);
    assert_eq!(*res.section(), changegroup::Section::Filelog(path(b"abc")));
    let chunk = res.chunk();

    let abch = NodeHash::from_str(ABC_HASH_STR).unwrap();
    assert_eq!(chunk.node, abch);
    assert_eq!(chunk.p1, NULL_HASH);
    assert_eq!(chunk.p2, NULL_HASH);
    assert_eq!(chunk.base, NULL_HASH);
    assert_eq!(chunk.linknode, changeset1_hash);
    assert_eq!(chunk.delta.fragments().len(), 0); // empty file

    let (res, stream) = next_cg2_part(core, stream);
    assert_matches!(res,
                    changegroup::Part::SectionEnd(ref section)
                    if *section == changegroup::Section::Filelog(path(b"abc")));

    let (res, stream) = next_cg2_part(core, stream);
    assert_eq!(*res.section(), changegroup::Section::Filelog(path(b"def")));
    let chunk = res.chunk();

    let defh = NodeHash::from_str(DEF_HASH_STR).unwrap();
    assert_eq!(chunk.node, defh);
    assert_eq!(chunk.p1, NULL_HASH);
    assert_eq!(chunk.p2, NULL_HASH);
    assert_eq!(chunk.base, NULL_HASH);
    assert_eq!(chunk.linknode, changeset2_hash);
    assert_eq!(chunk.delta.fragments().len(), 1);

    // That's it, wrap this up.
    let (res, stream) = next_cg2_part(core, stream);
    assert_matches!(res,
                    changegroup::Part::SectionEnd(ref section)
                    if *section == changegroup::Section::Filelog(path(b"def")));

    let (res, stream) = next_cg2_part(core, stream);
    assert_matches!(res, changegroup::Part::End);

    stream
}

fn path(bytes: &[u8]) -> Path {
    Path::new(bytes).unwrap()
}

fn parse_stream_start<R: AsyncRead>(
    core: &mut Core,
    reader: R,
    compression: Option<&str>,
) -> Result<Bundle2Stream<R>> {
    let mut m_stream_params = HashMap::new();
    let a_stream_params = HashMap::new();
    if let Some(compression) = compression {
        m_stream_params.insert("compression".into(), compression.into());
    }
    let expected = StreamHeader {
        m_stream_params: m_stream_params,
        a_stream_params: a_stream_params,
    };

    let logger = make_root_logger();

    let stream = Bundle2Stream::new(reader, logger);
    match core.run(stream.into_future()) {
        Ok((item, stream)) => {
            let stream_start = item.unwrap();
            assert_eq!(stream_start.stream_header(), expected);
            Ok(stream)
        }
        Err((e, _)) => Err(e),
    }
}

fn make_root_logger() -> Logger {
    let plain = slog_term::PlainSyncDecorator::new(io::stdout());
    Logger::root(slog_term::FullFormat::new(plain).build().fuse(), o!())
}

fn next_cg2_part<R: AsyncRead>(
    core: &mut Core,
    stream: Bundle2Stream<R>,
) -> (changegroup::Part, Bundle2Stream<R>) {
    let (res, stream) = core.next_stream(stream);
    (res.unwrap().inner_part().cg2_part(), stream)
}

trait CoreExt {
    fn next_stream<S: Stream>(&mut self, stream: S) -> (Option<S::Item>, S)
    where
        <S as Stream>::Error: Debug;
}

impl CoreExt for Core {
    fn next_stream<S: Stream>(&mut self, stream: S) -> (Option<S::Item>, S)
    where
        <S as Stream>::Error: Debug,
    {
        match self.run(stream.into_future()) {
            Ok((res, stream)) => (res, stream),
            Err((e, _)) => {
                panic!("stream failed to produce the next value! {:?}", e);
            }
        }
    }
}
