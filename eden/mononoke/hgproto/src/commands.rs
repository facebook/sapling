/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Mercurial protocol service framework
//!
//! To implement a Mercurial service, implement `HgCommands` and then use it to handle incominng
//! connections.
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::io::Cursor;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_stream::try_stream;
use bytes::Buf;
use bytes::Bytes;
use bytes::BytesMut;
use futures::channel::oneshot;
use futures::future;
use futures::future::ok;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::pin_mut;
use futures::stream::once;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::Stream;
use mercurial_bundles::bundle2;
use mercurial_bundles::bundle2::bundle2_stream;
use mercurial_bundles::bundle2::Bundle2Stream;
use mercurial_bundles::bundle2::StreamEvent;
use mercurial_bundles::Bundle2Item;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::NonRootMPath;
use qps::Qps;
use slog::Logger;
use tokio::io::AsyncBufRead;
use tokio::io::AsyncBufReadExt;
use tokio_util::codec::Decoder;
use tokio_util::io::StreamReader;

use crate::dechunker::Dechunker;
use crate::errors::*;
use crate::GetbundleArgs;
use crate::GettreepackArgs;
use crate::SingleRequest;
use crate::SingleResponse;

pub struct HgCommandHandler<H> {
    logger: Logger,
    commands: H,
    qps: Option<Arc<Qps>>,
    src_region: Option<String>,
}

impl<H: HgCommands + Send + Sync + 'static> HgCommandHandler<H> {
    pub fn new(
        logger: Logger,
        commands: H,
        qps: Option<Arc<Qps>>,
        src_region: Option<String>,
    ) -> Self {
        HgCommandHandler {
            logger,
            commands,
            qps,
            src_region,
        }
    }

    /// Handles a single command (not batched) by returning a stream of responses and a future
    /// resolving to the remainder unused input available only after the entire stream of responses
    /// have been consumed.
    pub fn handle<S>(
        &self,
        req: SingleRequest,
        instream: StreamReader<S, Bytes>,
    ) -> (
        BoxStream<'static, Result<SingleResponse>>,
        BoxFuture<'static, Result<StreamReader<S, Bytes>>>,
    )
    where
        S: Stream<Item = Result<Bytes, io::Error>> + Send + Unpin + 'static,
    {
        let hgcmds = &self.commands;

        if let (Some(qps), Some(src_region)) = (self.qps.as_ref(), self.src_region.as_ref()) {
            let _res = qps.bump(src_region);
        }

        match req {
            SingleRequest::Between { pairs } => (
                hgcmds
                    .between(pairs)
                    .map_ok(SingleResponse::Between)
                    .into_stream()
                    .boxed(),
                ok(instream).boxed(),
            ),
            SingleRequest::Branchmap => (
                hgcmds
                    .branchmap()
                    .map_ok(SingleResponse::Branchmap)
                    .into_stream()
                    .boxed(),
                ok(instream).boxed(),
            ),
            SingleRequest::Capabilities => (
                hgcmds
                    .capabilities()
                    .map_ok(SingleResponse::Capabilities)
                    .into_stream()
                    .boxed(),
                ok(instream).boxed(),
            ),
            SingleRequest::ClientTelemetry { args } => (
                hgcmds
                    .clienttelemetry(args)
                    .map_ok(SingleResponse::ClientTelemetry)
                    .into_stream()
                    .boxed(),
                ok(instream).boxed(),
            ),
            SingleRequest::Debugwireargs { one, two, all_args } => (
                self.debugwireargs(one, two, all_args)
                    .map_ok(SingleResponse::Debugwireargs)
                    .into_stream()
                    .boxed(),
                ok(instream).boxed(),
            ),
            SingleRequest::Getbundle(args) => (
                hgcmds
                    .getbundle(args)
                    .map_ok(SingleResponse::Getbundle)
                    .boxed(),
                ok(instream).boxed(),
            ),
            SingleRequest::Heads => (
                hgcmds
                    .heads()
                    .map_ok(SingleResponse::Heads)
                    .into_stream()
                    .boxed(),
                ok(instream).boxed(),
            ),
            SingleRequest::Hello => (
                hgcmds
                    .hello()
                    .map_ok(SingleResponse::Hello)
                    .into_stream()
                    .boxed(),
                ok(instream).boxed(),
            ),
            SingleRequest::Listkeys { namespace } => (
                hgcmds
                    .listkeys(namespace)
                    .map_ok(SingleResponse::Listkeys)
                    .into_stream()
                    .boxed(),
                ok(instream).boxed(),
            ),
            SingleRequest::ListKeysPatterns {
                namespace,
                patterns,
            } => (
                hgcmds
                    .listkeyspatterns(namespace, patterns)
                    .map_ok(SingleResponse::ListKeysPatterns)
                    .into_stream()
                    .boxed(),
                ok(instream).boxed(),
            ),
            SingleRequest::Lookup { key } => (
                hgcmds
                    .lookup(key)
                    .map_ok(SingleResponse::Lookup)
                    .into_stream()
                    .boxed(),
                ok(instream).boxed(),
            ),
            SingleRequest::Known { nodes } => (
                hgcmds
                    .known(nodes)
                    .map_ok(SingleResponse::Known)
                    .into_stream()
                    .boxed(),
                ok(instream).boxed(),
            ),
            SingleRequest::Knownnodes { nodes } => (
                hgcmds
                    .knownnodes(nodes)
                    .map_ok(SingleResponse::Known)
                    .into_stream()
                    .boxed(),
                ok(instream).boxed(),
            ),
            SingleRequest::Unbundle { heads } => self.handle_unbundle(instream, heads, None, None),
            SingleRequest::UnbundleReplay {
                heads,
                replaydata,
                respondlightly,
            } => self.handle_unbundle(instream, heads, Some(respondlightly), Some(replaydata)),
            SingleRequest::Gettreepack(args) => (
                hgcmds
                    .gettreepack(args)
                    .map_ok(SingleResponse::Gettreepack)
                    .boxed(),
                ok(instream).boxed(),
            ),
            SingleRequest::StreamOutShallow { tag } => (
                hgcmds
                    .stream_out_shallow(tag)
                    .map_ok(SingleResponse::StreamOutShallow)
                    .boxed(),
                ok(instream).boxed(),
            ),
            SingleRequest::GetpackV1 => {
                let (reqs, instream) = decode_getpack_arg_stream(instream);
                (
                    hgcmds
                        .getpackv1(reqs)
                        .map_ok(SingleResponse::Getpackv1)
                        .boxed(),
                    instream,
                )
            }
            SingleRequest::GetpackV2 => {
                let (reqs, instream) = decode_getpack_arg_stream(instream);
                (
                    hgcmds
                        .getpackv2(reqs)
                        .map_ok(SingleResponse::Getpackv2)
                        .boxed(),
                    instream,
                )
            }
            SingleRequest::GetCommitData { nodes } => (
                hgcmds
                    .getcommitdata(nodes)
                    .map_ok(SingleResponse::GetCommitData)
                    .boxed(),
                ok(instream).boxed(),
            ),
        }
    }

    fn handle_unbundle<S>(
        &self,
        instream: StreamReader<S, Bytes>,
        heads: Vec<String>,
        respondlightly: Option<bool>,
        replaydata: Option<String>,
    ) -> (
        BoxStream<'static, Result<SingleResponse>>,
        BoxFuture<'static, Result<StreamReader<S, Bytes>>>,
    )
    where
        S: Stream<Item = Result<Bytes, io::Error>> + Send + Unpin + 'static,
    {
        let hgcmds = &self.commands;
        let dechunker = Dechunker::new(instream);

        let bundle2stream = bundle2_stream(self.logger.clone(), dechunker, None);
        let (bundle2stream, remainder) = extract_remainder_from_bundle2(bundle2stream);

        let remainder = async move {
            let (bytes, mut remainder) = remainder.await?;
            if !bytes.is_empty() {
                return Err(ErrorKind::UnconsumedData(
                    String::from_utf8_lossy(bytes.as_ref()).into_owned(),
                )
                .into());
            }
            let buf = remainder.fill_buf().await?;
            if !buf.is_empty() {
                return Err(
                    ErrorKind::UnconsumedData(String::from_utf8_lossy(buf).into_owned()).into(),
                );
            }
            Ok(remainder.into_inner())
        }
        .boxed();

        let unbundle_fut = hgcmds.unbundle(heads, bundle2stream, respondlightly, replaydata);

        let resps = try_stream! {
            yield SingleResponse::ReadyForStream;
            yield SingleResponse::Unbundle(unbundle_fut.await?);
        }
        .boxed();

        (resps, remainder)
    }

    // @wireprotocommand('debugwireargs', 'one two *')
    // Handled here because this is a meta debugging command that isn't
    // specific to a repo.
    fn debugwireargs(
        &self,
        one: Vec<u8>,
        two: Vec<u8>,
        all_args: HashMap<Vec<u8>, Vec<u8>>,
    ) -> HgCommandRes<Bytes> {
        let mut out = Vec::<u8>::new();
        out.extend_from_slice(&one[..]);
        out.push(b' ');
        out.extend_from_slice(&two[..]);
        out.push(b' ');
        out.extend_from_slice(get_or_none(&all_args, &b"three"[..]));
        out.push(b' ');
        out.extend_from_slice(get_or_none(&all_args, &b"four"[..]));
        out.push(b' ');
        // Note that "five" isn't actually read off the wire -- instead, the
        // default value "None" is used.
        out.extend_from_slice(NONE);

        async { anyhow::Ok(out.into()) }.boxed()
    }
}

const NONE: &[u8] = b"None";

fn decode_getpack_arg_stream<S>(
    input: StreamReader<S, Bytes>,
) -> (
    BoxStream<'static, Result<(NonRootMPath, Vec<HgFileNodeId>)>>,
    BoxFuture<'static, Result<StreamReader<S, Bytes>>>,
)
where
    S: Stream<Item = Result<Bytes, io::Error>> + Send + Unpin + 'static,
{
    let (tx, rx) = futures::channel::oneshot::channel();

    let stream = try_stream! {
        let mut decoded = tokio_util::codec::FramedRead::new(input, Getpackv1ArgDecoder::new());
        {
            let decoded = &mut decoded;
            pin_mut!(decoded);
            while let Some(item) = decoded.try_next().await? {
                match item {
                    Some(item) => yield item,
                    None => break,
                }
            }
        }
        tx.send(decoded.into_inner()).map_err(|_| anyhow::anyhow!("Failed to send decoded stream"))?;
    };

    let remainder = async move { Ok(rx.await?) };

    (stream.boxed(), remainder.boxed())
}

#[derive(Clone)]
enum GetPackv1ParsingState {
    Start,
    ParsingFilename(u16),
    ParsedFilename(NonRootMPath),
    ParsingFileNodes(NonRootMPath, u32, Vec<HgFileNodeId>),
}

// Request format:
//
// [<filerequest>,...]\0\0
// filerequest = <filename len: 2 byte><filename><count: 4 byte>
//               [<node: 20 byte>,...]
//
// Getpackv1ArgDecoder parses one `filerequest` entry i.e. one filename and a few filenodes
struct Getpackv1ArgDecoder {
    state: GetPackv1ParsingState,
}

impl Getpackv1ArgDecoder {
    #[allow(unused)]
    pub fn new() -> Self {
        Self {
            state: GetPackv1ParsingState::Start,
        }
    }
}

impl Decoder for Getpackv1ArgDecoder {
    // If None has been decoded, then that means that client has sent all the data
    type Item = Option<(NonRootMPath, Vec<HgFileNodeId>)>;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>> {
        use self::GetPackv1ParsingState::*;

        let mut state = self.state.clone();
        let (result, state) = loop {
            let new_state = match state {
                Start => {
                    let prefix_len = 2;
                    if src.len() < prefix_len {
                        break (Ok(None), Start);
                    }
                    let len_bytes = src.split_to(prefix_len);
                    let len = Cursor::new(len_bytes.freeze()).get_u16();
                    if len == 0 {
                        // Finished parsing the stream
                        // 'Ok' means no error, 'Some' means that no more bytes needed,
                        // None means that getfiles stream has finished
                        return Ok(Some(None));
                    }
                    ParsingFilename(len)
                }
                ParsingFilename(filelen) => {
                    let filelen = filelen as usize;
                    if src.len() < filelen {
                        break (Ok(None), ParsingFilename(filelen as u16));
                    }

                    let filename_bytes = src.split_to(filelen);
                    ParsedFilename(NonRootMPath::new(&filename_bytes)?)
                }
                ParsedFilename(file) => {
                    let prefix_len = 4;
                    if src.len() < prefix_len {
                        break (Ok(None), ParsedFilename(file));
                    }

                    let len_bytes = src.split_to(prefix_len);
                    let nodes_count = Cursor::new(len_bytes.freeze()).get_u32();

                    ParsingFileNodes(file, nodes_count, vec![])
                }
                ParsingFileNodes(file, file_nodes_count, mut file_nodes) => {
                    if file_nodes_count as usize == file_nodes.len() {
                        return Ok(Some(Some((file, file_nodes))));
                    }
                    let node_size = 20;
                    if src.len() < node_size {
                        break (
                            Ok(None),
                            ParsingFileNodes(file, file_nodes_count, file_nodes),
                        );
                    }

                    let node = src.split_to(node_size);
                    let node = HgFileNodeId::from_bytes(&node)?;
                    file_nodes.push(node);
                    ParsingFileNodes(file, file_nodes_count, file_nodes)
                }
            };

            state = new_state;
        };

        self.state = state;
        result
    }
}

fn extract_remainder_from_bundle2<R>(
    bundle2: Bundle2Stream<R>,
) -> (
    BoxStream<'static, Result<Bundle2Item<'static>, Error>>,
    BoxFuture<'static, Result<bundle2::Remainder<R>, Error>>,
)
where
    R: AsyncBufRead + Send + Unpin + 'static,
{
    let (send, recv) = oneshot::channel();
    let mut send = Some(send);

    let bundle2items = bundle2
        .try_filter_map(move |stream_event| {
            match stream_event {
                StreamEvent::Next(bundle2item) => future::ok(Some(bundle2item)),
                StreamEvent::Done(remainder) => {
                    match send.take() {
                        None => future::err(
                            ErrorKind::Bundle2Invalid("stream remainder was sent twice".into())
                                .into(),
                        ),
                        Some(send) => {
                            // Receiving end will deal with failures
                            let _ = send.send(remainder);
                            future::ok(None)
                        }
                    }
                }
            }
        })
        .boxed();

    (
        bundle2items,
        async move { recv.await.context("Failed to extract bundle2 remainder") }.boxed(),
    )
}

#[inline]
fn get_or_none<'a>(map: &'a HashMap<Vec<u8>, Vec<u8>>, key: &'a [u8]) -> &'a [u8] {
    match map.get(key) {
        Some(val) => val,
        None => NONE,
    }
}

#[inline]
fn unimplemented<S, T>(op: S) -> HgCommandRes<T>
where
    S: Into<String>,
    T: Send + 'static,
{
    let msg = op.into();
    async move { Err(ErrorKind::Unimplemented(msg).into()) }.boxed()
}

// Async response from an Hg command
pub type HgCommandRes<T> = BoxFuture<'static, Result<T, Error>>;

// Trait representing Mercurial protocol operations, generic across protocols
// Derived from hg/mercurial/wireprotocol.py, functions with the `@wireprotocommand`
// decorator.
//
// XXX Do we need to do all of these? Are some historical/obsolete and can be ignored?
//
// TODO: placeholder types are generally `()`
pub trait HgCommands {
    // @wireprotocommand('between', 'pairs')
    fn between(
        &self,
        _pairs: Vec<(HgChangesetId, HgChangesetId)>,
    ) -> HgCommandRes<Vec<Vec<HgChangesetId>>> {
        unimplemented("between")
    }

    // @wireprotocommand('branchmap')
    fn branchmap(&self) -> HgCommandRes<HashMap<String, HashSet<HgChangesetId>>> {
        // We have no plans to support mercurial branches and hence no plans for branchmap,
        // so just return fake response.
        async { Ok(HashMap::new()) }.boxed()
    }

    // @wireprotocommand('capabilities')
    fn capabilities(&self) -> HgCommandRes<Vec<String>> {
        unimplemented("capabilities")
    }

    // @wireprotocommand('clienttelemetry')
    fn clienttelemetry(&self, _args: HashMap<Vec<u8>, Vec<u8>>) -> HgCommandRes<String> {
        unimplemented("clienttelemetry")
    }

    // @wireprotocommand('getbundle', '*')
    // TODO: make this streaming
    fn getbundle(&self, _args: GetbundleArgs) -> BoxStream<'static, Result<Bytes, Error>> {
        once(async { Err(ErrorKind::Unimplemented("getbundle".into()).into()) }).boxed()
    }

    // @wireprotocommand('heads')
    fn heads(&self) -> HgCommandRes<HashSet<HgChangesetId>> {
        unimplemented("heads")
    }

    // @wireprotocommand('hello')
    fn hello(&self) -> HgCommandRes<HashMap<String, Vec<String>>> {
        unimplemented("hello")
    }

    // @wireprotocommand('listkeys', 'namespace')
    fn listkeys(&self, _namespace: String) -> HgCommandRes<HashMap<Vec<u8>, Vec<u8>>> {
        unimplemented("listkeys")
    }

    // @wireprotocommand('listkeyspatterns', 'namespace', 'patterns *')
    fn listkeyspatterns(
        &self,
        _namespace: String,
        _patterns: Vec<String>,
    ) -> HgCommandRes<BTreeMap<String, HgChangesetId>> {
        unimplemented("listkeyspatterns")
    }

    // @wireprotocommand('lookup', 'key')
    fn lookup(&self, _key: String) -> HgCommandRes<Bytes> {
        unimplemented("lookup")
    }

    // @wireprotocommand('known', 'nodes *')
    fn known(&self, _nodes: Vec<HgChangesetId>) -> HgCommandRes<Vec<bool>> {
        unimplemented("known")
    }

    // @wireprotocommand('known', 'nodes *')
    fn knownnodes(&self, _nodes: Vec<HgChangesetId>) -> HgCommandRes<Vec<bool>> {
        unimplemented("knownnodes")
    }

    // @wireprotocommand('unbundle', 'heads')
    fn unbundle(
        &self,
        _heads: Vec<String>,
        _stream: BoxStream<'static, Result<Bundle2Item<'static>, Error>>,
        _respondlightly: Option<bool>,
        _replaydata: Option<String>,
    ) -> HgCommandRes<Bytes> {
        unimplemented("unbundle")
    }

    // @wireprotocommand('gettreepack', 'rootdir mfnodes basemfnodes directories')
    fn gettreepack(&self, _params: GettreepackArgs) -> BoxStream<'static, Result<Bytes, Error>> {
        once(async { Err(ErrorKind::Unimplemented("gettreepack".into()).into()) }).boxed()
    }

    // @wireprotocommand('stream_out_shallow', '*')
    fn stream_out_shallow(&self, _tag: Option<String>) -> BoxStream<'static, Result<Bytes, Error>> {
        once(async { Err(ErrorKind::Unimplemented("stream_out_shallow".into()).into()) }).boxed()
    }

    // @wireprotocommand()
    fn getpackv1(
        &self,
        _params: BoxStream<'static, Result<(NonRootMPath, Vec<HgFileNodeId>), Error>>,
    ) -> BoxStream<'static, Result<Bytes, Error>> {
        once(async { Err(ErrorKind::Unimplemented("getpackv1".into()).into()) }).boxed()
    }

    fn getpackv2(
        &self,
        _params: BoxStream<'static, Result<(NonRootMPath, Vec<HgFileNodeId>), Error>>,
    ) -> BoxStream<'static, Result<Bytes, Error>> {
        once(async { Err(ErrorKind::Unimplemented("getpackv2".into()).into()) }).boxed()
    }

    // @wireprotocommand('getcommitdata', 'nodes *')
    fn getcommitdata(
        &self,
        _nodes: Vec<HgChangesetId>,
    ) -> BoxStream<'static, Result<Bytes, Error>> {
        once(async { Err(ErrorKind::Unimplemented("getcommitdata".into()).into()) }).boxed()
    }
}

#[cfg(test)]
mod test {
    use bytes::BufMut;
    use futures::stream;
    use mononoke_macros::mononoke;
    use slog::o;
    use slog::Discard;

    use super::*;

    struct Dummy;
    impl HgCommands for Dummy {
        fn hello(&self) -> HgCommandRes<HashMap<String, Vec<String>>> {
            let mut res = HashMap::new();
            res.insert("capabilities".into(), vec!["something".into()]);

            async move { anyhow::Ok(res) }.boxed()
        }
    }

    fn assert_one<T>(vs: Vec<T>) -> T {
        assert_eq!(vs.len(), 1);
        vs.into_iter().next().unwrap()
    }

    fn hash_ones() -> HgFileNodeId {
        HgFileNodeId::new("1111111111111111111111111111111111111111".parse().unwrap())
    }

    #[tokio::test]
    async fn hello() -> Result<()> {
        let logger = Logger::root(Discard, o!());
        let handler = HgCommandHandler::new(logger, Dummy, None, None);

        let (r, _) = handler.handle(SingleRequest::Hello, StreamReader::new(stream::empty()));
        let r = assert_one(r.collect::<Vec<_>>().await);
        println!("hello r = {:?}", r);

        let mut res: HashMap<String, Vec<String>> = HashMap::new();
        res.insert("capabilities".into(), vec!["something".into()]);

        match r {
            Ok(SingleResponse::Hello(ref r)) if r == &res => {}
            bad => panic!("Bad result {:?}", bad),
        }

        Ok(())
    }

    #[tokio::test]
    async fn unimpl() -> Result<()> {
        let logger = Logger::root(Discard, o!());
        let handler = HgCommandHandler::new(logger, Dummy, None, None);

        let (r, _) = handler.handle(SingleRequest::Heads, StreamReader::new(stream::empty()));
        let r = assert_one(r.collect::<Vec<_>>().await);
        println!("heads r = {:?}", r);

        match r {
            Err(ref err) => println!("got expected error {:?}", err),
            bad => panic!("Bad result {:?}", bad),
        }

        Ok(())
    }

    #[mononoke::test]
    fn getpackv1decoder() {
        let mut decoder = Getpackv1ArgDecoder::new();
        let mut buf = vec![];
        buf.put_u16(0);
        assert_eq!(
            decoder
                .decode(&mut BytesMut::from(buf.as_slice()))
                .expect("unexpected error"),
            Some(None)
        );

        let mut buf = vec![];
        let path = NonRootMPath::new("file".as_bytes()).unwrap();
        buf.put_u16(4);
        buf.put_slice(&path.to_vec());
        buf.put_u32(1);
        buf.put_slice(hash_ones().as_bytes());
        assert_eq!(
            decoder
                .decode(&mut BytesMut::from(buf.as_slice()))
                .expect("unexpected error"),
            Some(Some((path, vec![hash_ones()])))
        );
    }

    #[tokio::test]
    async fn getpackv1() {
        let input = "\u{0}\u{4}path\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}";
        let input = async move { Ok(Bytes::from(input)) };
        let stream = StreamReader::new(stream::once(input.boxed()));
        let (paramstream, _input) = decode_getpack_arg_stream(stream);
        let res = paramstream.try_collect::<Vec<_>>().await.unwrap();
        assert_eq!(res, vec![(NonRootMPath::new("path").unwrap(), vec![])]);
    }
}
