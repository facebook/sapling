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

use slog::Logger;

use bytes::{Bytes, BytesMut};
use futures::future::{self, Future};
use futures::stream::Stream;

use futures_ext::{futures_ordered, BoxFuture, BoxStream, FutureExt};
use mercurial_types::NodeHash;

use {BranchRes, GetbundleArgs, Request, Response};
use errors::*;
use sshproto;

pub struct HgCommandHandler<H> {
    commands: H,
    logger: Logger,
}

impl<H: HgCommands> HgCommandHandler<H> {
    pub fn new(commands: H, logger: Logger) -> Self {
        HgCommandHandler { commands, logger }
    }

    pub fn handle(&self, req: Request) -> BoxFuture<Response, Error>
    where
        H: HgCommands,
    {
        match &req {
            &Request::Batch { .. } => (),
            req => debug!(self.logger, "Got request: {:?}", req),
        }

        let hgcmds = &self.commands;

        match req {
            Request::Batch { cmds } => self.batch(cmds)
                .map(Response::Batch)
                .map_err(self::Error::into)
                .boxify(),
            Request::Between { pairs } => hgcmds
                .between(pairs)
                .map(Response::Between)
                .map_err(self::Error::into)
                .boxify(),
            Request::Branchmap => hgcmds
                .branchmap()
                .map(Response::Branchmap)
                .map_err(self::Error::into)
                .boxify(),
            Request::Branches { nodes } => hgcmds
                .branches(nodes)
                .map(Response::Branches)
                .map_err(self::Error::into)
                .boxify(),
            Request::Clonebundles => hgcmds
                .clonebundles()
                .map(Response::Clonebundles)
                .map_err(self::Error::into)
                .boxify(),
            Request::Capabilities => hgcmds
                .capabilities()
                .map(Response::Capabilities)
                .map_err(self::Error::into)
                .boxify(),
            Request::Changegroup { roots } => hgcmds
                .changegroup(roots)
                .map(|_| Response::Changegroup)
                .map_err(self::Error::into)
                .boxify(),
            Request::Changegroupsubset { bases, heads } => hgcmds
                .changegroupsubset(bases, heads)
                .map(|_| Response::Changegroupsubset)
                .map_err(self::Error::into)
                .boxify(),
            Request::Debugwireargs { one, two, all_args } => self.debugwireargs(one, two, all_args)
                .map(Response::Debugwireargs)
                .map_err(self::Error::into)
                .boxify(),
            Request::Getbundle(args) => hgcmds
                .getbundle(args)
                .map(Response::Getbundle)
                .map_err(self::Error::into)
                .boxify(),
            Request::Heads => hgcmds
                .heads()
                .map(Response::Heads)
                .map_err(self::Error::into)
                .boxify(),
            Request::Hello => hgcmds
                .hello()
                .map(Response::Hello)
                .map_err(self::Error::into)
                .boxify(),
            Request::Listkeys { namespace } => hgcmds
                .listkeys(namespace)
                .map(Response::Listkeys)
                .map_err(self::Error::into)
                .boxify(),
            Request::Lookup { key } => hgcmds
                .lookup(key)
                .map(Response::Lookup)
                .map_err(self::Error::into)
                .boxify(),
            Request::Known { nodes } => hgcmds
                .known(nodes)
                .map(Response::Known)
                .map_err(self::Error::into)
                .boxify(),
            Request::Pushkey {
                namespace,
                key,
                old,
                new,
            } => hgcmds
                .pushkey(namespace, key, old, new)
                .map(|_| Response::Pushkey)
                .map_err(self::Error::into)
                .boxify(),
            Request::Streamout => hgcmds
                .stream_out()
                .map(|_| Response::Streamout)
                .map_err(self::Error::into)
                .boxify(),
            Request::Unbundle { heads, stream } => hgcmds
                .unbundle(heads, stream)
                .map(|_| Response::Unbundle)
                .map_err(self::Error::into)
                .boxify(), //_ => unimplemented!()
        }
    }

    // @wireprotocommand('batch', 'cmds *'), but the '*' is ignored.
    // This is handled here because it needs to spin off additional commands.
    fn batch(&self, cmds: Vec<(Vec<u8>, Vec<u8>)>) -> HgCommandRes<Vec<Bytes>> {
        let mut parsed_cmds = Vec::with_capacity(cmds.len());
        for cmd in cmds {
            // XXX This is somewhat ugly -- we're using the SSH protocol's rules
            // to handle this even though this is actually somewhat
            // protocol-agnostic.
            //
            // Ideally, the parser in sshproto/request.rs would be split up into
            // separate ssh-specific and general wireproto command parsers, but
            // we want to rewrite it soon anyway so it's not really worth doing
            // at the moment.
            let mut full_cmd = BytesMut::from([cmd.0, cmd.1].join(&b'\n'));
            let parsed = match sshproto::request::parse_batch(&mut full_cmd) {
                // TODO: collect all parsing errors, not just the first one?
                Err(err) => return future::err(err).boxify(),
                Ok(None) => {
                    return future::err(
                        ErrorKind::BatchInvalid(
                            String::from_utf8_lossy(full_cmd.as_ref()).into_owned(),
                        ).into(),
                    ).boxify();
                }
                Ok(Some(cmd)) => cmd,
            };
            info!(self.logger, "batch command: {:?}", parsed);
            parsed_cmds.push(parsed);
        }

        // Spin up all the individual commands. We must force evaluation of the
        // iterator because otherwise the closure will have captured self for
        // too long.
        let response_futures: Vec<_> = parsed_cmds
            .into_iter()
            .map(|cmd| self.handle(cmd))
            .collect();

        let encoded_futures = response_futures
            .into_iter()
            .map(|cmd| cmd.map(|res| sshproto::response::encode_cmd(&res)));
        futures_ordered(encoded_futures).collect().boxify()
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
    fn unbundle(&self, _heads: Vec<String>, _stream: Bytes) -> HgCommandRes<()> {
        unimplemented("unbundle")
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::future;

    use slog::Discard;

    struct Dummy;
    impl HgCommands for Dummy {
        fn hello(&self) -> HgCommandRes<HashMap<String, Vec<String>>> {
            let mut res = HashMap::new();
            res.insert("capabilities".into(), vec!["something".into()]);

            future::ok(res).boxify()
        }
    }

    #[test]
    fn hello() {
        let logger = Logger::root(Discard, o!());
        let handler = HgCommandHandler::new(Dummy, logger);

        let r = handler.handle(Request::Hello).wait();
        println!("hello r = {:?}", r);
        let mut res: HashMap<String, Vec<String>> = HashMap::new();
        res.insert("capabilities".into(), vec!["something".into()]);

        match r {
            Ok(Response::Hello(ref r)) if r == &res => (),
            bad => panic!("Bad result {:?}", bad),
        }
    }

    #[test]
    fn unimpl() {
        let logger = Logger::root(Discard, o!());
        let handler = HgCommandHandler::new(Dummy, logger);

        let r = handler.handle(Request::Heads).wait();
        println!("heads r = {:?}", r);

        match r {
            Err(ref err) => println!("got expected error {:?}", err),
            bad => panic!("Bad result {:?}", bad),
        }
    }
}
