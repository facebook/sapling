/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::anyhow;
use anyhow::Error;
use async_stream::try_stream;
use bytes::Bytes;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream::BoxStream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use qps::Qps;
use slog::Logger;
use tokio_util::codec::Decoder;
use tokio_util::codec::FramedRead;
use tokio_util::io::StreamReader;

use crate::commands::HgCommandHandler;
use crate::errors::ErrorKind;
use crate::HgCommands;
use crate::Request;
use crate::Response;

pub type OutputStream = BoxStream<'static, Result<Bytes, Error>>;

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
    pub fn new<In, H, Dec, Enc>(
        logger: Logger,
        input: In,
        commands: H,
        reqdec: Dec,
        respenc: Enc,
        wireproto_calls: Arc<Mutex<Vec<String>>>,
        qps: Option<Arc<Qps>>,
        src_region: Option<String>,
    ) -> Self
    where
        In: Stream<Item = Result<Bytes, io::Error>> + Send + Unpin + 'static,
        H: HgCommands + Send + Sync + 'static,
        Dec: Decoder<Item = Request> + Clone + Send + Sync + 'static,
        Dec::Error: From<io::Error> + From<Error> + From<ErrorKind> + Send + 'static,
        Enc: ResponseEncoder + Clone + Send + Sync + 'static,
        Error: From<Dec::Error>,
    {
        let inner = Arc::new(HgProtoHandlerInner {
            commands_handler: HgCommandHandler::new(logger, commands, qps, src_region),
            reqdec,
            respenc,
            wireproto_calls,
        });

        HgProtoHandler {
            outstream: handle(input, inner),
        }
    }

    pub fn into_stream(self) -> OutputStream {
        self.outstream
    }
}

fn handle<In, H, Dec, Enc>(
    input: In,
    handler: Arc<HgProtoHandlerInner<H, Dec, Enc>>,
) -> OutputStream
where
    In: Stream<Item = Result<Bytes, io::Error>> + Send + Unpin + 'static,
    H: HgCommands + Send + Sync + 'static,
    Dec: Decoder<Item = Request> + Clone + Send + Sync + 'static,
    Dec::Error: From<io::Error> + From<Error> + From<ErrorKind> + Send + 'static,
    Enc: ResponseEncoder + Clone + Send + Sync + 'static,
    Error: From<Dec::Error>,
{
    let input = StreamReader::new(input);
    let mut input = FramedRead::new(input, handler.reqdec.clone());

    try_stream! {
        while let Some(req) = input.try_next().await? {
            let remainder = input.into_inner();
            let (mut resps, remainder) = handle_request(req, remainder, handler.clone());
            while let Some(resp) = resps.try_next().await? {
                yield handler.respenc.encode(resp);
            }
            input = FramedRead::new(remainder.await?, handler.reqdec.clone());
        }

        let remainder = input.read_buffer();
        if !remainder.is_empty() {
            Err(ErrorKind::UnconsumedData(String::from_utf8_lossy(remainder).into_owned()))?;
        }
    }
    .try_flatten()
    .boxed()
}

/// Handles a singular request regardless if it contains multiple batched commands or a single one
/// It returns stream of responses that should be send to the client as soon as they are produced
/// and a future containing the remainder of the input that might contain more requests and that
/// will become available once the stream of responses is consumed.
fn handle_request<In, H, Dec, Enc>(
    req: Request,
    mut input: StreamReader<In, Bytes>,
    handler: Arc<HgProtoHandlerInner<H, Dec, Enc>>,
) -> (
    BoxStream<'static, Result<Response, Error>>,
    BoxFuture<'static, Result<StreamReader<In, Bytes>, Error>>,
)
where
    In: Stream<Item = Result<Bytes, io::Error>> + Send + Unpin + 'static,
    H: HgCommands + Send + Sync + 'static,
    Dec: Decoder<Item = Request> + Clone + Send + Sync + 'static,
    Dec::Error: From<io::Error> + Send + 'static,
    Enc: ResponseEncoder + Clone + Send + Sync + 'static,
    Error: From<Dec::Error>,
{
    req.record_request(&handler.wireproto_calls);
    match req {
        Request::Batch(reqs) => {
            let (sender, receiver) = oneshot::channel();
            (
                try_stream! {
                    let mut all_resps = Vec::new();
                    for req in reqs {
                        let (mut resps, remainder) = handler.commands_handler.handle(req, input);
                        while let Some(resp) = resps.try_next().await? {
                            all_resps.push(resp)
                        }
                        input = remainder.await?;
                    }
                    yield Response::Batch(all_resps);
                    sender.send(input).map_err(|_| anyhow!("failed to return input"))?;
                }
                .boxed(),
                receiver
                    .map_err(|_| anyhow!("Batch command failed"))
                    .boxed(),
            )
        }
        Request::Single(req) => {
            let (resps, remainder) = handler.commands_handler.handle(req, input);
            (resps.map_ok(Response::Single).boxed(), remainder)
        }
    }
}
