/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Mercurial protocol service framework
//!
//! To implement a Mercurial service, implement `HgCommands` and then use it to handle incominng
//! connections.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{self, BufRead, Cursor};
use std::mem;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Error, Result};
use bytes::{Buf, Bytes, BytesMut};
use failure_ext::FutureFailureErrorExt;
use futures::future::{self, err, ok, Either, Future};
use futures::stream::{self, futures_ordered, once, Stream};
use futures::sync::oneshot;
use futures::IntoFuture;
use futures_ext::{BoxFuture, BoxStream, BytesStream, FutureExt, StreamExt};
use slog::Logger;
use tokio_io::codec::Decoder;
use tokio_io::AsyncRead;

use crate::dechunker::Dechunker;
use crate::errors::*;
use crate::{GetbundleArgs, GettreepackArgs, SingleRequest, SingleResponse};
use mercurial_bundles::bundle2::{self, Bundle2Stream, StreamEvent};
use mercurial_bundles::Bundle2Item;
use mercurial_types::{HgChangesetId, HgFileNodeId, HgNodeHash, MPath};

const HASH_SIZE: usize = 40;

pub struct HgCommandHandler<H> {
    logger: Logger,
    commands: H,
}

impl<H: HgCommands + Send + 'static> HgCommandHandler<H> {
    pub fn new(logger: Logger, commands: H) -> Self {
        HgCommandHandler { logger, commands }
    }

    /// Handles a single command (not batched) by returning a stream of responses and a future
    /// resolving to the remainder unused input available only after the entire stream of responses
    /// have been consumed.
    pub fn handle<S>(
        &self,
        req: SingleRequest,
        instream: BytesStream<S>,
    ) -> (
        BoxStream<SingleResponse, Error>,
        BoxFuture<BytesStream<S>, Error>,
    )
    where
        S: Stream<Item = Bytes, Error = io::Error> + Send + 'static,
    {
        let hgcmds = &self.commands;

        match req {
            SingleRequest::Between { pairs } => (
                hgcmds
                    .between(pairs)
                    .map(SingleResponse::Between)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Branchmap => (
                hgcmds
                    .branchmap()
                    .map(SingleResponse::Branchmap)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Capabilities => (
                hgcmds
                    .capabilities()
                    .map(SingleResponse::Capabilities)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::ClientTelemetry { args } => (
                hgcmds
                    .clienttelemetry(args)
                    .map(SingleResponse::ClientTelemetry)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Debugwireargs { one, two, all_args } => (
                self.debugwireargs(one, two, all_args)
                    .map(SingleResponse::Debugwireargs)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Getbundle(args) => (
                hgcmds
                    .getbundle(args)
                    .map(SingleResponse::Getbundle)
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Heads => (
                hgcmds
                    .heads()
                    .map(SingleResponse::Heads)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Hello => (
                hgcmds
                    .hello()
                    .map(SingleResponse::Hello)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Listkeys { namespace } => (
                hgcmds
                    .listkeys(namespace)
                    .map(SingleResponse::Listkeys)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::ListKeysPatterns {
                namespace,
                patterns,
            } => (
                hgcmds
                    .listkeyspatterns(namespace, patterns)
                    .map(SingleResponse::ListKeysPatterns)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Lookup { key } => (
                hgcmds
                    .lookup(key)
                    .map(SingleResponse::Lookup)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Known { nodes } => (
                hgcmds
                    .known(nodes)
                    .map(SingleResponse::Known)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Knownnodes { nodes } => (
                hgcmds
                    .knownnodes(nodes)
                    .map(SingleResponse::Known)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Unbundle { heads } => {
                let dechunker = Dechunker::new(instream);
                let (dechunker, maybe_full_content) = if hgcmds.should_preserve_raw_bundle2() {
                    let full_bundle2_content = Arc::new(Mutex::new(Bytes::new()));
                    (
                        dechunker.with_full_content(full_bundle2_content.clone()),
                        Some(full_bundle2_content),
                    )
                } else {
                    (dechunker, None)
                };

                let bundle2stream = Bundle2Stream::new(self.logger.clone(), dechunker);
                let (bundle2stream, remainder) = extract_remainder_from_bundle2(bundle2stream);

                let remainder = remainder
                    .then(|rest| {
                        let (bytes, remainder) = match rest {
                            Err(e) => return Either::A(err(e)),
                            Ok(rest) => rest,
                        };
                        if !bytes.is_empty() {
                            Either::A(err(ErrorKind::UnconsumedData(
                                String::from_utf8_lossy(bytes.as_ref()).into_owned(),
                            )
                            .into()))
                        } else {
                            Either::B(remainder.check_is_done().from_err())
                        }
                    })
                    .then(
                        |check_is_done: Result<(bool, Dechunker<_>)>| match check_is_done {
                            Ok((true, remainder)) => ok(remainder.into_inner()),
                            Ok((false, mut remainder)) => match remainder.fill_buf() {
                                Err(e) => err(e.into()),
                                Ok(buf) => err(ErrorKind::UnconsumedData(
                                    String::from_utf8_lossy(buf).into_owned(),
                                )
                                .into()),
                            },
                            Err(e) => err(e.into()),
                        },
                    )
                    .boxify();

                let resps = futures_ordered(vec![
                    Either::A(ok(SingleResponse::ReadyForStream)),
                    Either::B(
                        hgcmds
                            .unbundle(heads, bundle2stream, maybe_full_content)
                            .map(|bytes| SingleResponse::Unbundle(bytes)),
                    ),
                ]);
                (resps.boxify(), remainder)
            }
            SingleRequest::Gettreepack(args) => (
                hgcmds
                    .gettreepack(args)
                    .map(SingleResponse::Gettreepack)
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Getfiles => {
                let (reqs, instream) =
                    decode_getfiles_arg_stream(instream, || GetfilesArgDecoder {});
                (
                    hgcmds.getfiles(reqs).map(SingleResponse::Getfiles).boxify(),
                    instream,
                )
            }
            SingleRequest::StreamOutShallow => (
                hgcmds
                    .stream_out_shallow()
                    .map(SingleResponse::StreamOutShallow)
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::GetpackV1 => {
                let (reqs, instream) =
                    decode_getfiles_arg_stream(instream, || Getpackv1ArgDecoder::new());
                (
                    hgcmds
                        .getpackv1(reqs)
                        .map(SingleResponse::Getpackv1)
                        .boxify(),
                    instream,
                )
            }
            SingleRequest::GetpackV2 => {
                let (reqs, instream) =
                    decode_getfiles_arg_stream(instream, || Getpackv1ArgDecoder::new());
                (
                    hgcmds
                        .getpackv2(reqs)
                        .map(SingleResponse::Getpackv2)
                        .boxify(),
                    instream,
                )
            }
        }
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

        future::ok(out.into()).boxify()
    }
}

const NONE: &[u8] = b"None";

// getfiles args format:
// (nodepath\n)*\n
// nodepath := node path
// node = hex hash
// Example:
// 1111111111111111111111111111111111111111path1\n2222222222222222222222222222222222222222path2\n\n
struct GetfilesArgDecoder {}

// Parses one (hash, path) pair
impl Decoder for GetfilesArgDecoder {
    // If None has been decoded, then that means that client has sent all the data
    type Item = Option<(HgFileNodeId, MPath)>;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>> {
        let maybeindex = src
            .iter()
            .enumerate()
            .find(|item| item.1 == &b'\n')
            .map(|(index, _)| index);

        let index = match maybeindex {
            Some(index) => index,
            None => {
                // Need more bytes
                return Ok(None);
            }
        };

        // Consume input and \n
        let mut buf = src.split_to(index + 1);
        debug_assert!(buf.len() > 0);
        let new_len = buf.len() - 1;
        buf.truncate(new_len);

        if buf.is_empty() {
            // Finished parsing the stream
            // 'Ok' means no error, 'Some' means that no more bytes needed,
            // None means that getfiles stream has finished
            Ok(Some(None))
        } else {
            if buf.len() < HASH_SIZE {
                bail!("Expected node hash")
            } else {
                let nodehashbytes = buf.split_to(HASH_SIZE);
                if buf.is_empty() {
                    bail!("Expected non-empty file")
                } else {
                    let nodehashstr = String::from_utf8(nodehashbytes.to_vec())?;
                    let nodehash = HgNodeHash::from_str(&nodehashstr)?;
                    // Some here means that new entry has been parsed
                    let parsed_res = Some((HgFileNodeId::new(nodehash), MPath::new(&buf)?));
                    // 'Ok' means no error, 'Some' means that no more bytes needed.
                    Ok(Some(parsed_res))
                }
            }
        }
    }
}

fn decode_getfiles_arg_stream<D, RType, S>(
    input: BytesStream<S>,
    create_decoder: impl Fn() -> D + Send + 'static,
) -> (BoxStream<RType, Error>, BoxFuture<BytesStream<S>, Error>)
where
    D: Decoder<Item = Option<RType>, Error = Error> + Send + 'static,
    RType: Send + 'static,
    S: Stream<Item = Bytes, Error = io::Error> + Send + 'static,
{
    let (send, recv) = oneshot::channel();

    // stream::unfold() requires us to to return None if it's finished, or Some(Future) if not.
    // We can't say if node file stream is finished before we parse the entry, that means that
    // we can't stop unfolding by returning None. Instead we return a "fake" error. This fake
    // error is a Result. If this fake error is Ok(...) then no real error happened.
    // Note that fake error also contains input stream that will be send back to the future that
    // waits for it.
    let entry_stream: BoxStream<_, Result<BytesStream<S>, (_, BytesStream<S>)>> =
        stream::unfold(input, move |input| {
            let fut_decode = input.into_future_decode(create_decoder());
            let fut = fut_decode
                .map_err(|err| Err(err)) // Real error happened, wrap it in result
                .and_then(|(maybe_item, instream)| match maybe_item {
                    None => {
                        // None here means we hit EOF, but that shouldn't happen
                        Err(Err((Error::msg("unexpected EOF"), instream)))
                            .into_future()
                            .boxify()
                    }
                    Some(maybe_nodehash) => {
                        match maybe_nodehash {
                            None => {
                                // None here means that we've read all the node-file pairs
                                // that client has sent us. Return fake error that means that
                                // we've successfully parsed the stream.
                                Err(Ok(instream)).into_future().boxify()
                            }
                            Some(nodehash) => {
                                // Parsed one more entry - continue
                                Ok((nodehash, instream)).into_future().boxify()
                            }
                        }
                    }
                });

            Some(fut)
        })
        .boxify();

    let try_send_instream =
        |wrapped_send: &mut Option<oneshot::Sender<_>>, instream: BytesStream<S>| -> Result<()> {
            let send = mem::replace(wrapped_send, None);
            let send = send.ok_or(Error::msg(
                "internal error: tried to send input stream twice",
            ))?;
            match send.send(instream) {
                Ok(_) => Ok(()), // Finished
                Err(_) => bail!("internal error while sending input stream back"),
            }
        };

    // We are parsing errors (both fake and real), and sending instream to the future
    // that awaits it. Note: instream should be send only once!
    let entry_stream = entry_stream.then({
        let mut wrapped_send = Some(send);
        move |val| {
            match val {
                Ok(nodefile) => Ok(Some(nodefile)),
                Err(Ok(instream)) => try_send_instream(&mut wrapped_send, instream).map(|_| None),
                Err(Err((err, instream))) => {
                    match try_send_instream(&mut wrapped_send, instream) {
                        // TODO(stash): if send fails, then Mononoke is deadlocked
                        // ignore send errors
                        Ok(_) => Err(err),
                        Err(_) => Err(err),
                    }
                }
            }
        }
    });

    // Finally, filter out last None value
    let entry_stream = entry_stream.filter_map(|val| val);
    (
        entry_stream.boxify(),
        recv.map_err(|err| Error::from(err)).boxify(),
    )
}

#[derive(Clone)]
enum GetPackv1ParsingState {
    Start,
    ParsingFilename(u16),
    ParsedFilename(MPath),
    ParsingFileNodes(MPath, u32, Vec<HgFileNodeId>),
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
    type Item = Option<(MPath, Vec<HgFileNodeId>)>;
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
                    let len = Cursor::new(len_bytes.freeze()).get_u16_be();
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
                    ParsedFilename(MPath::new(&filename_bytes)?)
                }
                ParsedFilename(file) => {
                    let prefix_len = 4;
                    if src.len() < prefix_len {
                        break (Ok(None), ParsedFilename(file));
                    }

                    let len_bytes = src.split_to(prefix_len);
                    let nodes_count = Cursor::new(len_bytes.freeze()).get_u32_be();

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
    BoxStream<Bundle2Item, Error>,
    BoxFuture<bundle2::Remainder<R>, Error>,
)
where
    R: AsyncRead + BufRead + 'static + Send,
{
    let (send, recv) = oneshot::channel();
    let mut send = Some(send);

    let bundle2items = bundle2
        .then(move |res_stream_event| match res_stream_event {
            Ok(StreamEvent::Next(bundle2item)) => Ok(Some(bundle2item)),
            Ok(StreamEvent::Done(remainder)) => {
                let send = send.take().ok_or(ErrorKind::Bundle2Invalid(
                    "stream remainder was sent twice".into(),
                ))?;
                // Receiving end will deal with failures
                let _ = send.send(remainder);
                Ok(None)
            }
            Err(err) => Err(err),
        })
        .filter_map(|val| val)
        .boxify();

    (
        bundle2items,
        recv.from_err()
            .context("While extracting bundle2 remainder")
            .from_err()
            .boxify(),
    )
}

#[inline]
fn get_or_none<'a>(map: &'a HashMap<Vec<u8>, Vec<u8>>, key: &'a [u8]) -> &'a [u8] {
    match map.get(key) {
        Some(ref val) => val,
        None => &NONE,
    }
}

#[inline]
fn unimplemented<S, T>(op: S) -> HgCommandRes<T>
where
    S: Into<String>,
    T: Send + 'static,
{
    future::err(ErrorKind::Unimplemented(op.into()).into()).boxify()
}

// Async response from an Hg command
pub type HgCommandRes<T> = BoxFuture<T, Error>;

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
        future::ok(HashMap::new()).boxify()
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
    fn getbundle(&self, _args: GetbundleArgs) -> BoxStream<Bytes, Error> {
        once(Err(ErrorKind::Unimplemented("getbundle".into()).into())).boxify()
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
        _stream: BoxStream<Bundle2Item, Error>,
        _maybe_full_content: Option<Arc<Mutex<Bytes>>>,
    ) -> HgCommandRes<Bytes> {
        unimplemented("unbundle")
    }

    // @wireprotocommand('gettreepack', 'rootdir mfnodes basemfnodes directories')
    fn gettreepack(&self, _params: GettreepackArgs) -> BoxStream<Bytes, Error> {
        once(Err(ErrorKind::Unimplemented("gettreepack".into()).into())).boxify()
    }

    // @wireprotocommand('getfiles', 'files*')
    fn getfiles(
        &self,
        _params: BoxStream<(HgFileNodeId, MPath), Error>,
    ) -> BoxStream<Bytes, Error> {
        once(Err(ErrorKind::Unimplemented("getfiles".into()).into())).boxify()
    }

    // @wireprotocommand('stream_out_shallow', '*')
    fn stream_out_shallow(&self) -> BoxStream<Bytes, Error> {
        once(Err(
            ErrorKind::Unimplemented("stream_out_shallow".into()).into()
        ))
        .boxify()
    }

    // @wireprotocommand()
    fn getpackv1(
        &self,
        _params: BoxStream<(MPath, Vec<HgFileNodeId>), Error>,
    ) -> BoxStream<Bytes, Error> {
        once(Err(ErrorKind::Unimplemented("getpackv1".into()).into())).boxify()
    }

    fn getpackv2(
        &self,
        _params: BoxStream<(MPath, Vec<HgFileNodeId>), Error>,
    ) -> BoxStream<Bytes, Error> {
        once(Err(ErrorKind::Unimplemented("getpackv2".into()).into())).boxify()
    }

    // whether raw bundle2 contents should be preverved in the blobstore
    fn should_preserve_raw_bundle2(&self) -> bool {
        unimplemented!("should_preserve_raw_bundle2")
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use bytes::{BufMut, BytesMut};
    use futures::{future, stream};
    use slog::{o, Discard};

    struct Dummy;
    impl HgCommands for Dummy {
        fn hello(&self) -> HgCommandRes<HashMap<String, Vec<String>>> {
            let mut res = HashMap::new();
            res.insert("capabilities".into(), vec!["something".into()]);

            future::ok(res).boxify()
        }
    }

    fn assert_one<T>(vs: Vec<T>) -> T {
        assert_eq!(vs.len(), 1);
        vs.into_iter().next().unwrap()
    }

    fn hash_ones() -> HgFileNodeId {
        HgFileNodeId::new("1111111111111111111111111111111111111111".parse().unwrap())
    }

    fn hash_twos() -> HgFileNodeId {
        HgFileNodeId::new("2222222222222222222222222222222222222222".parse().unwrap())
    }

    #[test]
    fn hello() {
        let logger = Logger::root(Discard, o!());
        let handler = HgCommandHandler::new(logger, Dummy);

        let (r, _) = handler.handle(SingleRequest::Hello, BytesStream::new(stream::empty()));
        let r = assert_one(r.wait().collect::<Vec<_>>());
        println!("hello r = {:?}", r);

        let mut res: HashMap<String, Vec<String>> = HashMap::new();
        res.insert("capabilities".into(), vec!["something".into()]);

        match r {
            Ok(SingleResponse::Hello(ref r)) if r == &res => (),
            bad => panic!("Bad result {:?}", bad),
        }
    }

    #[test]
    fn unimpl() {
        let logger = Logger::root(Discard, o!());
        let handler = HgCommandHandler::new(logger, Dummy);

        let (r, _) = handler.handle(SingleRequest::Heads, BytesStream::new(stream::empty()));
        let r = assert_one(r.wait().collect::<Vec<_>>());
        println!("heads r = {:?}", r);

        match r {
            Err(ref err) => println!("got expected error {:?}", err),
            bad => panic!("Bad result {:?}", bad),
        }
    }

    #[test]
    fn getfilesdecoder() {
        let mut decoder = GetfilesArgDecoder {};
        let mut input = BytesMut::from(format!("{}path\n", hash_ones()).as_bytes());
        let res = decoder
            .decode(&mut input)
            .expect("unexpected error")
            .expect("empty result");
        assert_eq!(Some((hash_ones(), MPath::new("path").unwrap())), res);

        let mut input = BytesMut::from(format!("{}path", hash_ones()).as_bytes());
        assert!(decoder
            .decode(&mut input)
            .expect("unexpected error")
            .is_none());

        let mut input =
            BytesMut::from(format!("{}path\n{}path2\n", hash_ones(), hash_twos()).as_bytes());

        let res = decoder
            .decode(&mut input)
            .expect("unexpected error")
            .expect("empty result");
        assert_eq!(Some((hash_ones(), MPath::new("path").unwrap())), res);

        let res = decoder
            .decode(&mut input)
            .expect("unexpected error")
            .expect("empty result");
        assert_eq!(Some((hash_twos(), MPath::new("path2").unwrap())), res);

        let mut input = BytesMut::from(format!("{}\n", hash_ones()).as_bytes());
        assert!(decoder.decode(&mut input).is_err());

        let mut input = BytesMut::from(format!("{}", hash_ones()).as_bytes());
        assert!(decoder
            .decode(&mut input)
            .expect("unexpected error")
            .is_none());

        let mut input = BytesMut::from("11111path\n".as_bytes());
        assert!(decoder.decode(&mut input).is_err());
    }

    #[test]
    fn getpackv1decoder() {
        let mut decoder = Getpackv1ArgDecoder::new();
        let mut buf = vec![];
        buf.put_u16_be(0);
        assert_eq!(
            decoder
                .decode(&mut BytesMut::from(buf))
                .expect("unexpected error"),
            Some(None)
        );

        let mut buf = vec![];
        let path = MPath::new("file".as_bytes()).unwrap();
        buf.put_u16_be(4);
        buf.put_slice(&path.to_vec());
        buf.put_u32_be(1);
        buf.put_slice(hash_ones().as_bytes());
        assert_eq!(
            decoder
                .decode(&mut BytesMut::from(buf))
                .expect("unexpected error"),
            Some(Some((path, vec![hash_ones()])))
        );
    }

    #[test]
    fn getfilesargs() {
        let input = format!("{}path\n{}path2\n\n", hash_ones(), hash_twos());
        let (paramstream, _input) = decode_getfiles_arg_stream(
            BytesStream::new(stream::once(Ok(Bytes::from(input)))),
            || GetfilesArgDecoder {},
        );

        let res = paramstream.collect().wait().unwrap();
        assert_eq!(
            res,
            vec![
                (hash_ones(), MPath::new("path").unwrap()),
                (hash_twos(), MPath::new("path2").unwrap()),
            ]
        );

        // Unexpected end of file
        let (paramstream, _input) =
            decode_getfiles_arg_stream(BytesStream::new(stream::empty()), || GetfilesArgDecoder {});
        assert!(paramstream.collect().wait().is_err());
    }

    #[test]
    fn getpackv1() {
        let input = "\u{0}\u{4}path\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}";
        let (paramstream, _input) = decode_getfiles_arg_stream(
            BytesStream::new(stream::once(Ok(Bytes::from(input)))),
            || Getpackv1ArgDecoder::new(),
        );
        let res = paramstream.collect().wait().unwrap();
        assert_eq!(res, vec![(MPath::new("path").unwrap(), vec![])]);
    }
}
