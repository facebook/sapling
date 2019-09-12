// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::convert::From;
use std::fmt::Debug;
use std::io::{BufRead, BufReader, Cursor};
use std::iter::Iterator;
use std::str::FromStr;

use assert_matches::assert_matches;
use futures::stream;
use futures::stream::Stream;
use futures_ext::BoxStream;
use tokio::runtime::Runtime;
use tokio_io::AsyncRead;

use crate::parts::phases_part;
use async_compression::membuf::MemBuf;
use async_compression::{Bzip2Compression, CompressorType, FlateCompression};
use mercurial_types::{HgChangesetId, HgNodeHash, HgPhase, MPath, RepoPath, NULL_HASH};
use partial_io::{GenWouldBlock, PartialAsyncRead, PartialWithErrors};
use quickcheck::rand;
use quickcheck::{QuickCheck, StdGen};

use crate::bundle2::{Bundle2Stream, StreamEvent};
use crate::bundle2_encode::Bundle2EncodeBuilder;
use crate::changegroup;
use crate::errors::*;
use crate::part_encode::PartEncodeBuilder;
use crate::part_header::{PartHeaderBuilder, PartHeaderType};
use crate::types::StreamHeader;
use crate::utils::get_compression_param;
use crate::wirepack;
use crate::Bundle2Item;
use context::CoreContext;

const BZIP2_BUNDLE2: &[u8] = include_bytes!("fixtures/bzip2.bin");
const UNCOMP_BUNDLE2: &[u8] = include_bytes!("fixtures/uncompressed.bin");
const UNKNOWN_COMPRESSION_BUNDLE2: &[u8] = include_bytes!("fixtures/unknown-compression.bin");
const WIREPACK_BUNDLE2: &[u8] = include_bytes!("fixtures/wirepack.bin");

const CHANGESET1_HASH_STR: &str = "b2040b24fd5cdfaf36e3164ddc357e834167b14a";
const CHANGESET2_HASH_STR: &str = "415ab71954c98ea93dab4b8f61f04ca57bc5c33c";
const MANIFEST1_HASH_STR: &str = "afcff2144f55cfa5d9b04ac4ed6598f26035aa77";
const MANIFEST2_HASH_STR: &str = "aa93dc3435cbfecd0c4c245b80b2a0b9ed35a015";
const ABC_HASH_STR: &str = "b80de5d138758541c5f05265ad144ab9fa86d1db";
const DEF_HASH_STR: &str = "bb969a19e8853962b4347bea4c24796324f10d8b";

#[derive(PartialEq)]
struct ByteBuf<'a>(&'a [u8]);

impl<'a> Debug for ByteBuf<'a> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            fmt.write_fmt(format_args!(r"\x{:02x}", byte))?;
        }
        Ok(())
    }
}

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
    quickcheck.quickcheck(parse_uncompressed as fn(PartialWithErrors<GenWouldBlock>) -> ());
}

fn parse_uncompressed(read_ops: PartialWithErrors<GenWouldBlock>) {
    parse_bundle(UNCOMP_BUNDLE2, None, read_ops);
}

#[test]
fn test_parse_unknown_compression() {
    let mut runtime = Runtime::new().unwrap();
    let bundle2_buf = BufReader::new(MemBuf::from(Vec::from(UNKNOWN_COMPRESSION_BUNDLE2)));
    let outer_stream_err = parse_stream_start(&mut runtime, bundle2_buf, Some("IL")).unwrap_err();
    assert_matches!(outer_stream_err.downcast::<ErrorKind>().unwrap(),
                    ErrorKind::Bundle2Decode(ref msg) if msg == "unknown compression 'IL'");
}

#[test]
fn test_empty_bundle_roundtrip_bzip() {
    empty_bundle_roundtrip(Some(CompressorType::Bzip2(Bzip2Compression::Default)));
}

#[test]
fn test_empty_bundle_roundtrip_gzip() {
    empty_bundle_roundtrip(Some(CompressorType::Gzip(FlateCompression::best())));
}

#[test]
fn test_empty_bundle_roundtrip_uncompressed() {
    empty_bundle_roundtrip(None);
}

fn empty_bundle_roundtrip(ct: Option<CompressorType>) {
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

    let mut runtime = Runtime::new().unwrap();
    let mut buf = runtime.block_on(encode_fut).unwrap();
    buf.set_position(0);

    // Now decode it.
    let ctx = CoreContext::test_mock();
    let stream = Bundle2Stream::new(ctx, buf);
    let (item, stream) = runtime.block_on(stream.into_future()).unwrap();

    let mut mparams = HashMap::new();
    let mut aparams = HashMap::new();
    mparams.insert("foo".into(), "123".into());
    mparams.insert("compression".into(), get_compression_param(&ct).into());
    aparams.insert("bar".into(), "456".into());
    let expected_header = StreamHeader {
        m_stream_params: mparams,
        a_stream_params: aparams,
    };

    assert_matches!(
        item,
        Some(StreamEvent::Next(Bundle2Item::Start(ref header))) if header == &expected_header
    );

    let (item, stream) = runtime.block_on(stream.into_future()).unwrap();
    assert_matches!(item, Some(StreamEvent::Done(_)));

    let (item, _stream) = runtime.block_on(stream.into_future()).unwrap();
    assert!(item.is_none());
}

#[test]
fn test_phases_part_encording() {
    let ctx = CoreContext::test_mock();
    let phases_entries = stream::iter_ok(vec![
        (
            HgChangesetId::from_bytes(b"bbbbbbbbbbbbbbbbbbbb").unwrap(),
            HgPhase::Public,
        ),
        (
            HgChangesetId::from_bytes(b"cccccccccccccccccccc").unwrap(),
            HgPhase::Public,
        ),
        (
            HgChangesetId::from_bytes(b"aaaaaaaaaaaaaaaaaaaa").unwrap(),
            HgPhase::Draft,
        ),
    ]);

    let cursor = Cursor::new(Vec::new());
    let mut builder = Bundle2EncodeBuilder::new(cursor);
    builder.set_compressor_type(None);

    let part = phases_part(ctx.clone(), phases_entries).unwrap();
    builder.add_part(part);

    let mut cursor = Runtime::new().unwrap().block_on(builder.build()).unwrap();
    cursor.set_position(0);
    let buf = cursor.fill_buf().unwrap();

    let res = b"HG20\x00\x00\x00\x0eCompression\x3dUN\x00\x00\x00\x12\x0bPHASE-HEADS\x00\x00\x00\x00\x00\x00\x00\x00\x00H\x00\x00\x00\x00bbbbbbbbbbbbbbbbbbbb\x00\x00\x00\x00cccccccccccccccccccc\x00\x00\x00\x01aaaaaaaaaaaaaaaaaaaa\x00\x00\x00\x00\x00\x00\x00\x00";
    assert_eq!(
        ByteBuf(buf),
        ByteBuf(res),
        "Compare phase-heads bundle2 part encoding against binary representation from mercurial"
    );
}

#[test]
fn test_unknown_part_bzip() {
    unknown_part(Some(CompressorType::Bzip2(Bzip2Compression::Default)));
}

#[test]
fn test_unknown_part_gzip() {
    unknown_part(Some(CompressorType::Gzip(FlateCompression::best())));
}

#[test]
fn test_unknown_part_uncompressed() {
    unknown_part(None);
}

fn unknown_part(ct: Option<CompressorType>) {
    let cursor = Cursor::new(Vec::with_capacity(32 * 1024));
    let mut builder = Bundle2EncodeBuilder::new(cursor);

    builder.set_compressor_type(ct);

    let unknown_part = PartEncodeBuilder::mandatory(PartHeaderType::Listkeys).unwrap();

    builder.add_part(unknown_part);
    let encode_fut = builder.build();

    let mut runtime = Runtime::new().unwrap();
    let mut buf = runtime.block_on(encode_fut).unwrap();
    buf.set_position(0);

    let ctx = CoreContext::test_mock();
    let stream = Bundle2Stream::new(ctx, buf);
    let parts = Vec::new();

    let decode_fut = stream
        .map_err(|e| -> () { panic!("unexpected error: {:?}", e) })
        .forward(parts);
    let (stream, parts) = runtime.block_on(decode_fut).unwrap();

    // Only the stream header should have been returned.
    let mut m_stream_params = HashMap::new();
    m_stream_params.insert("compression".into(), get_compression_param(&ct).into());
    let expected = StreamHeader {
        m_stream_params,
        a_stream_params: HashMap::new(),
    };

    let mut parts = parts.into_iter();
    assert_matches!(
        parts.next().unwrap().into_next().unwrap(),
        Bundle2Item::Start(ref header) if header == &expected
    );
    assert_matches!(parts.next(), Some(StreamEvent::Done(_)));
    assert!(parts.next().is_none());

    // Make sure the error was accumulated.
    let stream = stream.into_inner();
    let app_errors = stream.app_errors();
    assert_eq!(app_errors.len(), 1);
    assert_matches!(&app_errors[0],
                    &ErrorKind::BundleUnknownPart(ref header)
                    if header.part_type() == &PartHeaderType::Listkeys && header.mandatory());
}

fn parse_bundle(
    input: &[u8],
    compression: Option<&str>,
    read_ops: PartialWithErrors<GenWouldBlock>,
) {
    let mut runtime = Runtime::new().unwrap();

    let bundle2_buf = MemBuf::from(Vec::from(input));
    let partial_read = BufReader::new(PartialAsyncRead::new(bundle2_buf, read_ops));
    let stream = parse_stream_start(&mut runtime, partial_read, compression).unwrap();

    let (stream, cg2s) = {
        let (res, stream) = runtime.next_stream(stream);
        let mut header = PartHeaderBuilder::new(PartHeaderType::Changegroup, true).unwrap();
        header.add_mparam("version", "02").unwrap();
        header.add_aparam("nbchanges", "2").unwrap();
        let header = header.build(0);
        let cg2s = match res.unwrap().into_next().unwrap() {
            Bundle2Item::Changegroup(h, cg2s) => {
                assert_eq!(h, header);
                cg2s
            }
            bad => panic!("Unexpected bundle2 item: {:?}", bad),
        };
        (stream, cg2s)
    };

    verify_cg2(&mut runtime, cg2s);

    let (res, stream) = runtime.next_stream(stream);
    assert_matches!(res, Some(StreamEvent::Done(_)));

    let (res, stream) = runtime.next_stream(stream);
    assert!(res.is_none());

    // Make sure the stream is fused.
    let (res, stream) = runtime.next_stream(stream);
    assert!(res.is_none());

    assert!(stream.app_errors().is_empty());
}

fn verify_cg2(runtime: &mut Runtime, stream: BoxStream<changegroup::Part, Error>) {
    let (res, stream) = runtime.next_stream(stream);
    let res = res.expect("expected part");

    assert_eq!(*res.section(), changegroup::Section::Changeset);
    let chunk = res.chunk();

    // Verify that changesets parsed correctly.
    let changeset1_hash = HgNodeHash::from_str(CHANGESET1_HASH_STR).unwrap();
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

    let (res, stream) = runtime.next_stream(stream);
    let res = res.expect("expected part");

    assert_eq!(*res.section(), changegroup::Section::Changeset);
    let chunk = res.chunk();

    let changeset2_hash = HgNodeHash::from_str(CHANGESET2_HASH_STR).unwrap();
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

    let (res, stream) = runtime.next_stream(stream);
    let res = res.expect("expected part");

    assert_matches!(
        res,
        changegroup::Part::SectionEnd(changegroup::Section::Changeset)
    );

    // Verify basic properties of manifests.
    let (res, stream) = runtime.next_stream(stream);
    let res = res.expect("expected part");

    assert_eq!(*res.section(), changegroup::Section::Manifest);
    let chunk = res.chunk();

    let manifest1_hash = HgNodeHash::from_str(MANIFEST1_HASH_STR).unwrap();
    assert_eq!(chunk.node, manifest1_hash);
    assert_eq!(chunk.p1, NULL_HASH);
    assert_eq!(chunk.p2, NULL_HASH);
    assert_eq!(chunk.base, NULL_HASH);
    assert_eq!(chunk.linknode, changeset1_hash);

    let (res, stream) = runtime.next_stream(stream);
    let res = res.expect("expected part");

    assert_eq!(*res.section(), changegroup::Section::Manifest);
    let chunk = res.chunk();

    let manifest2_hash = HgNodeHash::from_str(MANIFEST2_HASH_STR).unwrap();
    assert_eq!(chunk.node, manifest2_hash);
    assert_eq!(chunk.p1, manifest1_hash);
    assert_eq!(chunk.p2, NULL_HASH);
    assert_eq!(chunk.base, manifest1_hash); // In this case there's a delta.
    assert_eq!(chunk.linknode, changeset2_hash);

    let (res, stream) = runtime.next_stream(stream);
    let res = res.expect("expected part");

    assert_matches!(
        res,
        changegroup::Part::SectionEnd(changegroup::Section::Manifest)
    );

    // Filelog section
    let (res, stream) = runtime.next_stream(stream);
    let res = res.expect("expected part");

    assert_eq!(*res.section(), changegroup::Section::Filelog(path(b"abc")));
    let chunk = res.chunk();

    let abch = HgNodeHash::from_str(ABC_HASH_STR).unwrap();
    assert_eq!(chunk.node, abch);
    assert_eq!(chunk.p1, NULL_HASH);
    assert_eq!(chunk.p2, NULL_HASH);
    assert_eq!(chunk.base, NULL_HASH);
    assert_eq!(chunk.linknode, changeset1_hash);
    assert_eq!(chunk.delta.fragments().len(), 0); // empty file

    let (res, stream) = runtime.next_stream(stream);
    let res = res.expect("expected part");

    assert_matches!(res,
                    changegroup::Part::SectionEnd(ref section)
                    if *section == changegroup::Section::Filelog(path(b"abc")));

    let (res, stream) = runtime.next_stream(stream);
    let res = res.expect("expected part");

    assert_eq!(*res.section(), changegroup::Section::Filelog(path(b"def")));
    let chunk = res.chunk();

    let defh = HgNodeHash::from_str(DEF_HASH_STR).unwrap();
    assert_eq!(chunk.node, defh);
    assert_eq!(chunk.p1, NULL_HASH);
    assert_eq!(chunk.p2, NULL_HASH);
    assert_eq!(chunk.base, NULL_HASH);
    assert_eq!(chunk.linknode, changeset2_hash);
    assert_eq!(chunk.delta.fragments().len(), 1);

    // That's it, wrap this up.
    let (res, stream) = runtime.next_stream(stream);
    let res = res.expect("expected part");

    assert_matches!(res,
                    changegroup::Part::SectionEnd(ref section)
                    if *section == changegroup::Section::Filelog(path(b"def")));

    let (res, stream) = runtime.next_stream(stream);
    let res = res.expect("expected part");

    assert_matches!(res, changegroup::Part::End);

    let (res, _) = runtime.next_stream(stream);
    assert!(
        res.is_none(),
        "after the End part this stream should be empty"
    );
}

#[test]
fn test_parse_wirepack() {
    let rng = StdGen::new(rand::thread_rng(), 20);
    let mut quickcheck = QuickCheck::new().gen(rng);
    quickcheck.quickcheck(parse_wirepack as fn(PartialWithErrors<GenWouldBlock>) -> ());
}

fn parse_wirepack(read_ops: PartialWithErrors<GenWouldBlock>) {
    let mut runtime = Runtime::new().unwrap();

    let cursor = Cursor::new(WIREPACK_BUNDLE2);
    let partial_read = BufReader::new(PartialAsyncRead::new(cursor, read_ops));

    let stream = parse_stream_start(&mut runtime, partial_read, None).unwrap();

    let stream = {
        let (res, stream) = runtime.next_stream(stream);
        match res {
            Some(StreamEvent::Next(Bundle2Item::Changegroup(_, cg2s))) => {
                runtime.block_on(cg2s.for_each(|_| Ok(()))).unwrap();
            }
            bad => panic!("Unexpected Bundle2Item: {:?}", bad),
        }
        stream
    };

    let (stream, wirepacks) = {
        let (res, stream) = runtime.next_stream(stream);
        // Header
        let mut header = PartHeaderBuilder::new(PartHeaderType::B2xTreegroup2, true).unwrap();
        header.add_mparam("version", "1").unwrap();
        header.add_mparam("cache", "False").unwrap();
        header.add_mparam("category", "manifests").unwrap();
        let header = header.build(1);
        let wirepacks = match res.unwrap().into_next().unwrap() {
            Bundle2Item::B2xTreegroup2(h, wirepacks) => {
                assert_eq!(h, header);
                wirepacks
            }
            bad => panic!("Unexpected bundle2 item: {:?}", bad),
        };
        (stream, wirepacks)
    };

    // These are a few identifiers present in the bundle.
    let baz_dir = RepoPath::dir("baz").unwrap();
    let baz_hash = HgNodeHash::from_str("dcb9fa4bb7cdb673cd5752088b48d4c3f9c1fc23").unwrap();
    let root_hash = HgNodeHash::from_str("7d315c7a04cce5404f7ef16bf55eb7f4e90d159f").unwrap();
    let root_p1 = HgNodeHash::from_str("e313fc172615835d205f5881f8f34dd9bb0f0092").unwrap();

    let (res, wirepacks) = runtime.next_stream(wirepacks);
    let res = res.expect("expected part");

    // First entries received are for the directory "baz".
    let (path, entry_count) = res.unwrap_history_meta();
    assert_eq!(path, baz_dir);
    assert_eq!(entry_count, 1);

    let (res, wirepacks) = runtime.next_stream(wirepacks);
    let res = res.expect("expected part");

    let history_entry = res.unwrap_history();
    assert_eq!(history_entry.node, baz_hash);
    assert_eq!(history_entry.p1, NULL_HASH);
    assert_eq!(history_entry.p2, NULL_HASH);
    assert_eq!(history_entry.linknode, NULL_HASH);
    assert_eq!(history_entry.copy_from, None);

    let (res, wirepacks) = runtime.next_stream(wirepacks);
    let res = res.expect("expected part");

    let (path, entry_count) = res.unwrap_data_meta();
    assert_eq!(path, baz_dir);
    assert_eq!(entry_count, 1);

    let (res, wirepacks) = runtime.next_stream(wirepacks);
    let res = res.expect("expected part");

    let data_entry = res.unwrap_data();
    assert_eq!(path, baz_dir);
    assert_eq!(data_entry.node, baz_hash);
    assert_eq!(data_entry.delta_base, NULL_HASH);
    let fragments = data_entry.delta.fragments();
    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0].start, 0);
    assert_eq!(fragments[0].end, 0);
    assert_eq!(fragments[0].content.len(), 46);

    let (res, wirepacks) = runtime.next_stream(wirepacks);
    let res = res.expect("expected part");

    // Next entries received are for the root manifest.
    let (path, entry_count) = res.unwrap_history_meta();
    assert_eq!(path, RepoPath::root());
    assert_eq!(entry_count, 1);

    let (res, wirepacks) = runtime.next_stream(wirepacks);
    let res = res.expect("expected part");

    let history_entry = res.unwrap_history();
    assert_eq!(history_entry.node, root_hash);
    assert_eq!(history_entry.p1, root_p1);
    assert_eq!(history_entry.p2, NULL_HASH);
    assert_eq!(history_entry.linknode, NULL_HASH);
    assert_eq!(history_entry.copy_from, None);

    let (res, wirepacks) = runtime.next_stream(wirepacks);
    let res = res.expect("expected part");

    let (path, entry_count) = res.unwrap_data_meta();
    assert_eq!(path, RepoPath::root());
    assert_eq!(entry_count, 1);

    let (res, wirepacks) = runtime.next_stream(wirepacks);
    let res = res.expect("expected part");

    let data_entry = res.unwrap_data();
    assert_eq!(data_entry.node, root_hash);
    assert_eq!(data_entry.delta_base, NULL_HASH);
    let fragments = data_entry.delta.fragments();
    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0].start, 0);
    assert_eq!(fragments[0].end, 0);
    assert_eq!(fragments[0].content.len(), 136);

    let (res, wirepacks) = runtime.next_stream(wirepacks);
    let res = res.expect("expected part");

    // Finally the end.
    assert_eq!(res, wirepack::Part::End);
    let (res, _) = runtime.next_stream(wirepacks);
    assert!(
        res.is_none(),
        "after the End part this stream should be empty"
    );

    let (res, stream) = runtime.next_stream(stream);
    assert_matches!(res, Some(StreamEvent::Done(_)));
    assert!(stream.app_errors().is_empty());
}

fn path(bytes: &[u8]) -> MPath {
    MPath::new(bytes).unwrap()
}

fn parse_stream_start<R: AsyncRead + BufRead + 'static + Send>(
    runtime: &mut Runtime,
    reader: R,
    compression: Option<&str>,
) -> Result<Bundle2Stream<R>> {
    let mut m_stream_params = HashMap::new();
    let a_stream_params = HashMap::new();
    if let Some(compression) = compression {
        m_stream_params.insert("compression".into(), compression.into());
    }
    let expected = StreamHeader {
        m_stream_params,
        a_stream_params,
    };

    let ctx = CoreContext::test_mock();
    let stream = Bundle2Stream::new(ctx, reader);
    match runtime.block_on(stream.into_future()) {
        Ok((item, stream)) => {
            let stream_start = item.unwrap();
            assert_eq!(stream_start.into_next().unwrap().unwrap_start(), expected);
            Ok(stream)
        }
        Err((e, _)) => Err(e),
    }
}

trait RuntimeExt {
    fn next_stream<S>(&mut self, stream: S) -> (Option<S::Item>, S)
    where
        S: Stream + Send + 'static,
        <S as Stream>::Item: Send,
        <S as Stream>::Error: Debug + Send;
}

impl RuntimeExt for Runtime {
    fn next_stream<S>(&mut self, stream: S) -> (Option<S::Item>, S)
    where
        S: Stream + Send + 'static,
        <S as Stream>::Item: Send,
        <S as Stream>::Error: Debug + Send,
    {
        match self.block_on(stream.into_future()) {
            Ok((res, stream)) => (res, stream),
            Err((e, _)) => {
                panic!("stream failed to produce the next value! {:?}", e);
            }
        }
    }
}
