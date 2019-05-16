// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::commands::HgCommandHandler;
use crate::errors::*;
use crate::{HgCommands, Request, Response};
use bytes::Bytes;
use context::CoreContext;
use failure::FutureFailureErrorExt;
use futures::future::{err, ok, Either};
use futures::sync::oneshot;
use futures::{stream, Future, Poll, Stream};
use futures_ext::{BoxFuture, BoxStream, BytesStream, FutureExt, StreamExt};
use std::io;
use std::sync::{Arc, Mutex};
use tokio_io::codec::Decoder;

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
    wireproto_calls: Arc<Mutex<Vec<String>>>,
}

impl HgProtoHandler {
    pub fn new<'a, In, H, Dec, Enc>(
        ctx: CoreContext,
        input: In,
        commands: H,
        reqdec: Dec,
        respenc: Enc,
        wireproto_calls: Arc<Mutex<Vec<String>>>,
    ) -> Self
    where
        In: Stream<Item = Bytes, Error = io::Error> + Send + 'static,
        H: HgCommands + Send + Sync + 'static,
        Dec: Decoder<Item = Request> + Clone + Send + Sync + 'static,
        Dec::Error: From<io::Error> + Send + 'static,
        Enc: ResponseEncoder + Clone + Send + Sync + 'static,
        Error: From<Dec::Error>,
    {
        let inner = Arc::new(HgProtoHandlerInner {
            commands_handler: HgCommandHandler::new(ctx, commands),
            reqdec,
            respenc,
            wireproto_calls,
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
                                )
                                .into())
                            }),
                            Some(req) => {
                                let (resps, remainder) =
                                    handle_request(req, remainder, handler.clone());
                                Either::B(ok((
                                    Some(
                                        resps
                                            .map(move |resp| handler.respenc.encode(resp))
                                            .flatten()
                                            .boxify(),
                                    ),
                                    Some(remainder),
                                )))
                            }
                        }
                    });
                Either::B(future)
            }
        });

        Some(future)
    })
    .filter_map(|x| x)
    .flatten()
    .boxify()
}

/// Handles a singular request regardless if it contains multiple batched commands or a single one
/// It returns stream of responses that should be send to the client as soon as they are produced
/// and a future containing the remainder of the input that might contain more requests and that
/// will become available once the stream of responses is consumed.
fn handle_request<In, H, Dec, Enc>(
    req: Request,
    input: BytesStream<In>,
    handler: Arc<HgProtoHandlerInner<H, Dec, Enc>>,
) -> (
    BoxStream<Response, Error>,
    BoxFuture<BytesStream<In>, Error>,
)
where
    In: Stream<Item = Bytes, Error = io::Error> + Send + 'static,
    H: HgCommands + Send + Sync + 'static,
    Dec: Decoder<Item = Request> + Clone + Send + Sync + 'static,
    Dec::Error: From<io::Error> + Send + 'static,
    Enc: ResponseEncoder + Clone + Send + Sync + 'static,
    Error: From<Dec::Error>,
{
    req.record_request(&handler.wireproto_calls);
    match req {
        Request::Batch(reqs) => {
            let (send, recv) = oneshot::channel();
            let responses = stream::unfold(
                (reqs.into_iter(), ok(input).boxify(), send),
                move |(mut reqs, input, send)| match reqs.next() {
                    None => {
                        let _ = send.send(input);
                        None
                    }
                    Some(req) => Some(input.map({
                        let handler = handler.clone();
                        move |input| {
                            let (resps, remainder) = handler.commands_handler.handle(req, input);
                            (resps, (reqs, remainder, send))
                        }
                    })),
                },
            )
            .flatten()
            .collect()
            .map(Response::Batch)
            .into_stream();
            (
                responses.boxify(),
                recv.from_err()
                    .with_context(|_| format!("While handling batch command"))
                    .from_err()
                    .and_then(|input| input)
                    .boxify(),
            )
        }
        Request::Single(req) => {
            let (resps, remainder) = handler.commands_handler.handle(req, input);
            (resps.map(Response::Single).boxify(), remainder)
        }
    }
}
