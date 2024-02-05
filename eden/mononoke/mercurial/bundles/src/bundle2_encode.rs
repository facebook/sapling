/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::io::Cursor;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use anyhow::bail;
use anyhow::Context as _;
use anyhow::Result;
use byteorder::BigEndian;
use byteorder::ByteOrder;
use bytes::BufMut;
use futures::Sink;
use futures::SinkExt;
use futures::StreamExt;
use mercurial_types::percent_encode;
use pin_project::pin_project;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio_util::codec::FramedWrite;

use crate::chunk::Chunk;
use crate::chunk::NewChunkEncoder;
use crate::errors::ErrorKind;
use crate::part_encode::PartEncode;
use crate::part_encode::PartEncodeBuilder;
use crate::part_header::PartId;
use crate::types::StreamHeader;
use crate::utils::capitalize_first;
use crate::utils::is_mandatory_param;

/// This is a general wrapper around a Sink to prevent closing of the underlying Sink. This is
/// useful when using Sink::send_all, because in addition to writing and flushing the data it also
/// closes the sink, which may result in IO errors if called repeatedly on the same Sink
#[derive(Debug)]
#[pin_project]
struct UncloseableSink<S> {
    #[pin]
    pub inner: S,
}

impl<S: Sink<Item>, Item> Sink<Item> for UncloseableSink<S> {
    type Error = S::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        this.inner.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
        let this = self.project();
        this.inner.start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        this.inner.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

/// Builder to generate a bundle2.
pub struct Bundle2EncodeBuilder<W> {
    writer: W,
    header: StreamHeader,
    parts: Vec<PartEncode>,
}

impl<W> Bundle2EncodeBuilder<W>
where
    W: AsyncWrite + Send + Unpin,
{
    pub fn new(writer: W) -> Self {
        Bundle2EncodeBuilder {
            writer,
            header: StreamHeader {
                m_stream_params: HashMap::new(),
                a_stream_params: HashMap::new(),
            },
            parts: Vec::new(),
        }
    }

    pub fn add_stream_param(&mut self, key: String, val: String) -> Result<&mut Self> {
        if &key.to_lowercase() == "compression" {
            let msg = "stream compression is not implemented";
            bail!(ErrorKind::Bundle2Encode(msg.into()));
        }
        if is_mandatory_param(&key)
            .with_context(|| ErrorKind::Bundle2Encode("stream key is invalid".into()))?
        {
            self.header.m_stream_params.insert(key.to_lowercase(), val);
        } else {
            self.header.a_stream_params.insert(key.to_lowercase(), val);
        }
        Ok(self)
    }

    pub fn add_part(&mut self, part: PartEncodeBuilder) -> &mut Self {
        let part_id = self.parts.len() as PartId;
        self.parts.push(part.build(part_id));
        self
    }

    pub async fn build(mut self) -> Result<W> {
        let mut mparams = self.header.m_stream_params;

        // We never compress the bundle.
        mparams.insert("compression".into(), "UN".into());

        // Build the buffer required for the stream header.
        let mut header_buf: Vec<u8> = Vec::new();

        header_buf.put_slice(b"HG20");
        // Reserve 4 bytes for the length.
        header_buf.put_u32(0);
        // Now write out the stream header.

        let params = mparams
            .into_iter()
            .map(|x| (capitalize_first(x.0), x.1))
            .chain(self.header.a_stream_params);
        Self::build_stream_params(params, &mut header_buf);

        let header_len = (header_buf.len() - 8) as u32;
        BigEndian::write_u32(&mut header_buf[4..], header_len);

        self.writer
            .write_all_buf(&mut Cursor::new(header_buf))
            .await?;

        let mut sink = UncloseableSink {
            inner: FramedWrite::new(self.writer, NewChunkEncoder),
        };

        for part in self.parts {
            part.forward(&mut sink).await?;
        }

        sink.send(Chunk::empty()).await?;
        sink.flush().await?;

        Ok(sink.inner.into_inner())
    }

    fn build_stream_params<I>(params: I, header_buf: &mut Vec<u8>)
    where
        I: Iterator<Item = (String, String)>,
    {
        let mut first = true;
        for (key, val) in params {
            if !first {
                header_buf.put_u8(b' ');
            }
            first = false;
            header_buf.put(percent_encode(&key).into_bytes().as_slice());
            header_buf.put_u8(b'=');
            header_buf.put(percent_encode(&val).into_bytes().as_slice());
        }
    }
}

/// Ensure that Bundle2Encode is Send.
fn _assert_send() {
    fn _assert<T: Send>(_val: &T) {}

    let builder = Bundle2EncodeBuilder::new(Cursor::new(Vec::new()));
    _assert(&builder);
    _assert(&builder.build());
}
