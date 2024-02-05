/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Overall coordinator for parsing bundle2 streams.

use std::fmt;
use std::fmt::Debug;
use std::fmt::Formatter;
use std::io::Cursor;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Error;
use anyhow::Result;
use async_stream::try_stream;
use bytes::BytesMut;
use futures::pin_mut;
use futures::stream::BoxStream;
use futures::StreamExt;
use futures::TryStreamExt;
use slog::Logger;
use tokio::io::AsyncBufRead;
use tokio::io::AsyncReadExt;
use tokio_util::codec::FramedRead;

use crate::errors::ErrorKind;
use crate::part_inner::inner_stream;
use crate::part_outer::outer_stream;
use crate::part_outer::OuterFrame;
use crate::stream_start::StartDecoder;
use crate::Bundle2Item;

pub enum StreamEvent<I, S> {
    Next(I),
    Done(S),
}

impl<I, S> StreamEvent<I, S> {
    pub fn into_next(self) -> Result<I, Self> {
        if let StreamEvent::Next(v) = self {
            Ok(v)
        } else {
            Err(self)
        }
    }
}

impl<I, S> Debug for StreamEvent<I, S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            StreamEvent::Next(_) => write!(f, "Next(...)"),
            StreamEvent::Done(_) => write!(f, "Done(...)"),
        }
    }
}

pub type Remainder<R> = (BytesMut, R);

pub type Bundle2Stream<R> =
    BoxStream<'static, Result<StreamEvent<Bundle2Item<'static>, Remainder<R>>>>;

pub fn bundle2_stream<R>(
    logger: Logger,
    read: R,
    app_errors: Option<Arc<Mutex<Vec<ErrorKind>>>>,
) -> Bundle2Stream<R>
where
    R: AsyncBufRead + Unpin + Send + 'static,
{
    try_stream! {
        let mut stream = Box::pin(FramedRead::new(read, StartDecoder));

        if let Some(start) = stream.try_next().await? {
            let mut stream = Pin::into_inner(stream);
            let read_buf = stream.read_buffer_mut().split();
            let io = stream.into_inner();

            let mut stream = outer_stream(logger.clone(), &start, Cursor::new(read_buf).chain(io))?;

            yield StreamEvent::Next(Bundle2Item::Start(start));

            while let Some(res) = stream.try_next().await? {
                eprintln!("XXXX {:?}", res);
                match res {
                    Err(e) => match e.downcast::<ErrorKind>() {
                        Ok(ek) => {
                            eprintln!("IS EK");
                            if ek.is_app_error() {
                                eprintln!("IS AE");
                                if let Some(app_errors) = app_errors.as_ref() {
                                    app_errors.lock().unwrap().push(ek);
                                }
                                continue;
                            }
                            Err(Error::from(ek))?;
                        }
                        Err(e) => {
                            Err(e)?;
                        }
                    },
                    Ok(OuterFrame::Header(header)) => {
                        let (bundle2item, remainder) = inner_stream(logger.clone(), header, stream);
                        yield StreamEvent::Next(bundle2item);
                        pin_mut!(remainder);
                        stream = remainder.await?;
                    }
                    Ok(OuterFrame::Discard) => {}
                    Ok(OuterFrame::StreamEnd) => {
                        // No more parts to go.
                        let mut stream = Pin::into_inner(stream);
                        let mut read_buf = stream.read_buffer_mut().split();
                        let (cursor, io) = stream.into_inner().into_inner().into_inner();
                        read_buf
                            .extend_from_slice(&cursor.get_ref()[(cursor.position() as usize)..]);

                        yield StreamEvent::Done((read_buf, io));
                        return;
                    }
                    _ => panic!("Expected a header or stream end"),
                }
            }
        }
    }
    .boxed()
}
