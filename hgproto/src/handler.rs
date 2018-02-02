// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io;
use std::sync::Arc;

use futures::{stream, Future, Poll, Stream};
use futures::future::{err, ok, Either};
use futures::sync::oneshot;
use futures_ext::{BoxFuture, BoxStream, BytesStream, FutureExt, StreamExt};
use tokio_io::codec::Decoder;

use bytes::Bytes;
use slog::{self, Logger};

use {HgCommands, Request, Response};
use commands::HgCommandHandler;

use errors::*;

pub type OutputStream = BoxStream<Bytes, Error>;

pub trait ResponseEncoder {
    fn encode(&self, response: Response) -> OutputStream;
}

pub struct HgProtoHandler {
    outstream: OutputStream,
}

struct HgProtoHandlerInner<H, Dec, Enc> {
    commands_handler: HgCommandHandler<H>,
    reqdec: Dec,
    respenc: Enc,
    _logger: Logger,
}

impl HgProtoHandler {
    pub fn new<'a, In, H, Dec, Enc, L>(
        input: In,
        commands: H,
        reqdec: Dec,
        respenc: Enc,
        logger: L,
    ) -> Self
    where
        In: Stream<Item = Bytes, Error = io::Error> + Send + 'static,
        H: HgCommands + Send + Sync + 'static,
        Dec: Decoder<Item = Request> + Clone + Send + Sync + 'static,
        Dec::Error: From<io::Error> + Send + 'static,
        Enc: ResponseEncoder + Clone + Send + Sync + 'static,
        Error: From<Dec::Error>,
        L: Into<Option<&'a Logger>>,
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
            outstream: handle(input, inner),
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

fn handle<In, H, Dec, Enc>(
    input: In,
    handler: Arc<HgProtoHandlerInner<H, Dec, Enc>>,
) -> OutputStream
where
    In: Stream<Item = Bytes, Error = io::Error> + Send + 'static,
    H: HgCommands + Send + Sync + 'static,
    Dec: Decoder<Item = Request> + Clone + Send + Sync + 'static,
    Dec::Error: From<io::Error> + Send + 'static,
    Enc: ResponseEncoder + Clone + Send + Sync + 'static,
    Error: From<Dec::Error>,
{
    let input = BytesStream::new(input);

    stream::unfold(Some(ok(input).boxify()), move |input| {
        let input = match input {
            None => return None,
            Some(input) => input,
        };

        let future = input.and_then({
            let handler = handler.clone();
            move |input| {
                if input.is_empty() {
                    return Either::A(ok((None, None)));
                }

                let future = input
                    .into_future_decode(handler.reqdec.clone())
                    .map_err(|(err, _)| -> Error { err.into() })
                    .and_then({
                        let handler = handler.clone();
                        move |(req, remainder)| match req {
                            None => Either::A(if remainder.is_empty() {
                                ok((None, None))
                            } else {
                                let (bytes, _) = remainder.into_parts();
                                err(ErrorKind::UnconsumedData(
                                    String::from_utf8_lossy(bytes.as_ref()).into_owned(),
                                ).into())
                            }),
                            Some(req) => {
                                Either::B(handle_request(req, remainder, handler.clone()).map(
                                    move |(resp, remainder)| {
                                        (Some(handler.respenc.encode(resp)), Some(remainder))
                                    },
                                ))
                            }
                        }
                    });
                Either::B(future)
            }
        });

        Some(future)
    }).filter_map(|x| x)
        .flatten()
        .boxify()
}

fn handle_request<In, H, Dec, Enc>(
    req: Request,
    input: BytesStream<In>,
    handler: Arc<HgProtoHandlerInner<H, Dec, Enc>>,
) -> BoxFuture<(Response, BoxFuture<BytesStream<In>, Error>), Error>
where
    In: Stream<Item = Bytes, Error = io::Error> + Send + 'static,
    H: HgCommands + Send + Sync + 'static,
    Dec: Decoder<Item = Request> + Clone + Send + Sync + 'static,
    Dec::Error: From<io::Error> + Send + 'static,
    Enc: ResponseEncoder + Clone + Send + Sync + 'static,
    Error: From<Dec::Error>,
{
    let future = match req {
        Request::Batch(reqs) => {
            let (send, recv) = oneshot::channel();
            Either::A(
                stream::unfold(
                    (reqs.into_iter(), ok(input).boxify(), send),
                    move |(mut reqs, input, send)| match reqs.next() {
                        None => {
                            let _ = send.send(input);
                            None
                        }
                        Some(req) => Some(input.and_then({
                            let handler = handler.clone();
                            move |input| {
                                handler
                                    .commands_handler
                                    .handle(req, input)
                                    .map(|(res, remainder)| (res, (reqs, remainder, send)))
                            }
                        })),
                    },
                ).collect()
                    .and_then(|resps| {
                        recv.from_err()
                            .map(|remainder| (Response::Batch(resps), remainder))
                    }),
            )
        }
        Request::Single(req) => Either::B(
            handler
                .commands_handler
                .handle(req, input)
                .map(|(res, remainder)| (Response::Single(res), remainder)),
        ),
    };
    future.boxify()
}
