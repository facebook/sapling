// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io;
use std::sync::Arc;

use futures::{Future, Poll, Stream};
use futures::future::Either;
use futures::stream::futures_ordered;
use futures_ext::{BoxStream, StreamExt, StreamLayeredExt};
use tokio_io::codec::{Decoder, Encoder};

use bytes::Bytes;
use slog::{self, Logger};

use {HgCommands, Request, Response};
use commands::HgCommandHandler;

use errors::*;

type BytesStream = BoxStream<Bytes, io::Error>;

pub struct HgProtoHandler {
    outstream: BoxStream<Bytes, Error>,
}

struct HgProtoHandlerInner<H, Dec, Enc> {
    commands_handler: HgCommandHandler<H>,
    reqdec: Dec,
    respenc: Enc,
    _logger: Logger,
}

impl HgProtoHandler {
    pub fn new<'a, H, Dec, Enc, L: Into<Option<&'a Logger>>>(
        instream: BytesStream,
        commands: H,
        reqdec: Dec,
        respenc: Enc,
        logger: L,
    ) -> Self
    where
        H: HgCommands + Send + Sync + 'static,
        Dec: Decoder<Item = Request> + Clone + Send + Sync + 'static,
        Dec::Error: From<io::Error> + Send + 'static,
        Enc: Encoder<Item = Response> + Clone + Send + Sync + 'static,
        Enc::Error: From<Error> + Send + 'static,
        Error: From<Dec::Error> + From<Enc::Error>,
    {
        let logger = match logger.into() {
            None => Logger::root(slog::Discard, o!()),
            Some(logger) => logger.new(o!()),
        };

        let inner = Arc::new(HgProtoHandlerInner {
            commands_handler: HgCommandHandler::new(commands, logger.new(o!())),
            reqdec,
            respenc,
            _logger: logger,
        });

        HgProtoHandler {
            outstream: handle(instream, inner),
        }
    }
}

impl Stream for HgProtoHandler {
    type Item = Bytes;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.outstream.poll()
    }
}

fn handle<H, Dec, Enc>(
    instream: BytesStream,
    handler: Arc<HgProtoHandlerInner<H, Dec, Enc>>,
) -> BoxStream<Bytes, Error>
where
    H: HgCommands + Send + Sync + 'static,
    Dec: Decoder<Item = Request> + Clone + Send + Sync + 'static,
    Dec::Error: From<io::Error> + Send + 'static,
    Enc: Encoder<Item = Response> + Clone + Send + Sync + 'static,
    Enc::Error: From<Error> + Send + 'static,
    Error: From<Dec::Error> + From<Enc::Error>,
{
    instream
        .decode(handler.reqdec.clone())
        .from_err()
        .and_then({
            let handler = handler.clone();
            move |req| match req {
                Request::Batch(reqs) => Either::A(
                    futures_ordered(
                        reqs.into_iter()
                            .map(|req| handler.commands_handler.handle(req)),
                    ).collect()
                        .map(Response::Batch),
                ),
                Request::Single(req) => {
                    Either::B(handler.commands_handler.handle(req).map(Response::Single))
                }
            }
        })
        .encode(handler.respenc.clone())
        .from_err()
        .boxify()
}
