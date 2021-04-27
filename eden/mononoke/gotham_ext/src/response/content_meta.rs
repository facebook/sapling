/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::content_encoding::ContentEncoding;

use super::stream::{CompressedResponseStream, ResponseStream};
use super::stream_ext::{EndOnErr, ForwardErr};

pub trait ContentMeta {
    /// Provide the content (i.e. Content-Encoding) for the underlying content. This will be sent
    /// to the client.
    fn content_encoding(&self) -> ContentEncoding;

    /// Provide the length of the content in this stream, if available (i.e. Content-Length). If
    /// provided, this must be the actual length of the stream. If missing, the transfer will be
    /// chunked.
    fn content_length(&self) -> Option<u64>;
}

impl<S> ContentMeta for ResponseStream<S> {
    fn content_length(&self) -> Option<u64> {
        ResponseStream::content_length(self)
    }

    fn content_encoding(&self) -> ContentEncoding {
        ContentEncoding::Identity
    }
}

impl ContentMeta for CompressedResponseStream<'_> {
    fn content_length(&self) -> Option<u64> {
        None
    }

    fn content_encoding(&self) -> ContentEncoding {
        ContentEncoding::Compressed(self.content_compression())
    }
}

/// Provide an implementation of ContentMeta that propagates through Either (i.e. left_stream(),
/// right_stream()).
impl<A, B> ContentMeta for futures::future::Either<A, B>
where
    A: ContentMeta,
    B: ContentMeta,
{
    fn content_length(&self) -> Option<u64> {
        // left_stream(), right_stream() doesn't change the stream data.
        match self {
            Self::Left(a) => a.content_length(),
            Self::Right(b) => b.content_length(),
        }
    }

    fn content_encoding(&self) -> ContentEncoding {
        // left_stream(), right_stream() doesn't change the stream data.
        match self {
            Self::Left(a) => a.content_encoding(),
            Self::Right(b) => b.content_encoding(),
        }
    }
}

impl<S, F> ContentMeta for futures::stream::InspectOk<S, F>
where
    S: ContentMeta,
{
    fn content_length(&self) -> Option<u64> {
        // inspect_ok doesn't change the stream data.
        self.get_ref().content_length()
    }

    fn content_encoding(&self) -> ContentEncoding {
        // inspect_ok doesn't change the stream data.
        self.get_ref().content_encoding()
    }
}

impl<St, Si, E> ContentMeta for ForwardErr<St, Si, E>
where
    St: ContentMeta,
{
    fn content_length(&self) -> Option<u64> {
        // forward_err doesn't change the stream data.
        self.get_ref().content_length()
    }

    fn content_encoding(&self) -> ContentEncoding {
        // forward_err doesn't change the stream data.
        self.get_ref().content_encoding()
    }
}

impl<S, F> ContentMeta for EndOnErr<S, F>
where
    S: ContentMeta,
{
    fn content_length(&self) -> Option<u64> {
        // If an error occurs, the stream will end prematurely, so the content
        // length here may not be correct. However, this should be OK as it
        // would allow the client to detect an incomplete response and raise
        // an appropriate error.
        self.get_ref().content_length()
    }

    fn content_encoding(&self) -> ContentEncoding {
        // end_on_err doesn't change the data's encoding.
        self.get_ref().content_encoding()
    }
}
