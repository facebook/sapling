// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Mercurial protocol service framework
//!
//! To implement a Mercurial service, implement `HgCommands` and then use it to handle incominng
//! connections.
use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead};

use slog::Logger;

use bytes::Bytes;
use futures::future::{self, err, ok, Either, Future};
use futures::stream::{futures_ordered, Stream};
use futures::sync::oneshot;

use dechunker::Dechunker;
use futures_ext::{BoxFuture, BoxStream, BytesStream, FutureExt, StreamExt};
use mercurial_bundles::bundle2::{self, Bundle2Stream};
use mercurial_types::NodeHash;
use tokio_io::AsyncRead;

use {BranchRes, GetbundleArgs, SingleRequest, SingleResponse};

use errors::*;

pub struct HgCommandHandler<H> {
    commands: H,
    logger: Logger,
}

impl<H: HgCommands + Send + 'static> HgCommandHandler<H> {
    pub fn new(commands: H, logger: Logger) -> Self {
        HgCommandHandler { commands, logger }
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
        debug!(self.logger, "Got request: {:?}", req);
        let hgcmds = &self.commands;

        match req {
            SingleRequest::Between { pairs } => (
                hgcmds
                    .between(pairs)
                    .map(SingleResponse::Between)
                    .map_err(self::Error::into)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Branches { nodes } => (
                hgcmds
                    .branches(nodes)
                    .map(SingleResponse::Branches)
                    .map_err(self::Error::into)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Branchmap => (
                hgcmds
                    .branchmap()
                    .map(SingleResponse::Branchmap)
                    .map_err(self::Error::into)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Clonebundles => (
                hgcmds
                    .clonebundles()
                    .map(SingleResponse::Clonebundles)
                    .map_err(self::Error::into)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Capabilities => (
                hgcmds
                    .capabilities()
                    .map(SingleResponse::Capabilities)
                    .map_err(self::Error::into)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Changegroup { roots } => (
                hgcmds
                    .changegroup(roots)
                    .map(|_| SingleResponse::Changegroup)
                    .map_err(self::Error::into)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Changegroupsubset { bases, heads } => (
                hgcmds
                    .changegroupsubset(bases, heads)
                    .map(|_| SingleResponse::Changegroupsubset)
                    .map_err(self::Error::into)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Debugwireargs { one, two, all_args } => (
                self.debugwireargs(one, two, all_args)
                    .map(SingleResponse::Debugwireargs)
                    .map_err(self::Error::into)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Getbundle(args) => (
                hgcmds
                    .getbundle(args)
                    .map(SingleResponse::Getbundle)
                    .map_err(self::Error::into)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Heads => (
                hgcmds
                    .heads()
                    .map(SingleResponse::Heads)
                    .map_err(self::Error::into)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Hello => (
                hgcmds
                    .hello()
                    .map(SingleResponse::Hello)
                    .map_err(self::Error::into)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Listkeys { namespace } => (
                hgcmds
                    .listkeys(namespace)
                    .map(SingleResponse::Listkeys)
                    .map_err(self::Error::into)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Lookup { key } => (
                hgcmds
                    .lookup(key)
                    .map(SingleResponse::Lookup)
                    .map_err(self::Error::into)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Known { nodes } => (
                hgcmds
                    .known(nodes)
                    .map(SingleResponse::Known)
                    .map_err(self::Error::into)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Pushkey {
                namespace,
                key,
                old,
                new,
            } => (
                hgcmds
                    .pushkey(namespace, key, old, new)
                    .map(|_| SingleResponse::Pushkey)
                    .map_err(self::Error::into)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Streamout => (
                hgcmds
                    .stream_out()
                    .map(|_| SingleResponse::Streamout)
                    .map_err(self::Error::into)
                    .into_stream()
                    .boxify(),
                ok(instream).boxify(),
            ),
            SingleRequest::Unbundle { heads } => {
                let (send, recv) = oneshot::channel();
                let resps = futures_ordered(vec![
                    Either::A(ok(SingleResponse::ReadyForStream)),
                    Either::B(
                        hgcmds
                            .unbundle(
                                heads,
                                Bundle2Stream::new(Dechunker::new(instream), self.logger.new(o!())),
                            )
                            .then(|rest| {
                                let (bytes, remainder) = match rest {
                                    Err(e) => return Either::A(err(e)),
                                    Ok(rest) => rest,
                                };
                                if !bytes.is_empty() {
                                    Either::A(err(ErrorKind::UnconsumedData(
                                        String::from_utf8_lossy(bytes.as_ref()).into_owned(),
                                    ).into()))
                                } else {
                                    Either::B(remainder.check_is_done().from_err())
                                }
                            })
                            .then(
                                |check_is_done: Result<(bool, Dechunker<_>)>| match check_is_done {
                                    Ok((true, remainder)) => {
                                        let _ = send.send(remainder.into_inner());
                                        ok(SingleResponse::Unbundle)
                                    }
                                    Ok((false, mut remainder)) => match remainder.fill_buf() {
                                        Err(e) => err(e.into()),
                                        Ok(buf) => err(ErrorKind::UnconsumedData(
                                            String::from_utf8_lossy(buf).into_owned(),
                                        ).into()),
                                    },
                                    Err(e) => err(e.into()),
                                },
                            ),
                    ),
                ]);
                (resps.boxify(), recv.from_err().boxify())
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
    fn between(&self, _pairs: Vec<(NodeHash, NodeHash)>) -> HgCommandRes<Vec<Vec<NodeHash>>> {
        unimplemented("between")
    }

    // @wireprotocommand('branchmap')
    fn branchmap(&self) -> HgCommandRes<HashMap<String, HashSet<NodeHash>>> {
        unimplemented("branchmap")
    }

    // @wireprotocommand('branches', 'nodes')
    fn branches(&self, _nodes: Vec<NodeHash>) -> HgCommandRes<Vec<BranchRes>> {
        unimplemented("branches")
    }

    // @wireprotocommand('clonebundles', '')
    fn clonebundles(&self) -> HgCommandRes<String> {
        unimplemented("clonebundles")
    }

    // @wireprotocommand('capabilities')
    fn capabilities(&self) -> HgCommandRes<Vec<String>> {
        unimplemented("capabilities")
    }

    // @wireprotocommand('changegroup', 'roots')
    fn changegroup(&self, _roots: Vec<NodeHash>) -> HgCommandRes<()> {
        // TODO: streaming something
        unimplemented("changegroup")
    }

    // @wireprotocommand('changegroupsubset', 'bases heads')
    fn changegroupsubset(&self, _bases: Vec<NodeHash>, _heads: Vec<NodeHash>) -> HgCommandRes<()> {
        unimplemented("changegroupsubset")
    }

    // @wireprotocommand('getbundle', '*')
    // TODO: make this streaming
    fn getbundle(&self, _args: GetbundleArgs) -> HgCommandRes<Bytes> {
        unimplemented("getbundle")
    }

    // @wireprotocommand('heads')
    fn heads(&self) -> HgCommandRes<HashSet<NodeHash>> {
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

    // @wireprotocommand('lookup', 'key')
    fn lookup(&self, _key: String) -> HgCommandRes<NodeHash> {
        unimplemented("lookup")
    }

    // @wireprotocommand('known', 'nodes *')
    fn known(&self, _nodes: Vec<NodeHash>) -> HgCommandRes<Vec<bool>> {
        unimplemented("known")
    }

    // @wireprotocommand('pushkey', 'namespace key old new')
    fn pushkey(
        &self,
        _namespace: String,
        _key: String,
        _old: NodeHash,
        _new: NodeHash,
    ) -> HgCommandRes<()> {
        unimplemented("pushkey")
    }

    // @wireprotocommand('stream_out')
    fn stream_out(&self) -> HgCommandRes<BoxStream<Vec<u8>, Error>> {
        // XXX raw streaming?
        unimplemented("stream_out")
    }

    // @wireprotocommand('unbundle', 'heads')
    fn unbundle<R>(
        &self,
        _heads: Vec<String>,
        _stream: Bundle2Stream<'static, R>,
    ) -> HgCommandRes<bundle2::Remainder<R>>
    where
        R: AsyncRead + BufRead + 'static + Send,
    {
        unimplemented("unbundle")
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use futures::{future, stream};
    use slog::Discard;

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

    #[test]
    fn hello() {
        let logger = Logger::root(Discard, o!());
        let handler = HgCommandHandler::new(Dummy, logger);

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
        let handler = HgCommandHandler::new(Dummy, logger);

        let (r, _) = handler.handle(SingleRequest::Heads, BytesStream::new(stream::empty()));
        let r = assert_one(r.wait().collect::<Vec<_>>());
        println!("heads r = {:?}", r);

        match r {
            Err(ref err) => println!("got expected error {:?}", err),
            bad => panic!("Bad result {:?}", bad),
        }
    }
}
