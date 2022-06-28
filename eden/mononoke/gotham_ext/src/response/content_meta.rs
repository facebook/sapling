/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::content_encoding::ContentEncoding;

use super::stream::CompressedResponseStream;
use super::stream::ResponseStream;
use super::stream_ext::CaptureFirstErr;
use super::stream_ext::EndOnErr;

pub trait ContentMetaProvider {
    /// Provide the content (i.e. Content-Encoding) for the underlying content. This will be sent
    /// to the client.
    fn content_encoding(&self) -> ContentEncoding;

    /// Provide the length of the content in this stream, if available (i.e. Content-Length). If
    /// provided, this must be the actual length of the stream. If missing, the transfer will be
    /// chunked.
    fn content_length(&self) -> Option<u64>;
}

impl<S> ContentMetaProvider for ResponseStream<S> {
    fn content_length(&self) -> Option<u64> {
        ResponseStream::content_length(self)
    }

    fn content_encoding(&self) -> ContentEncoding {
        ContentEncoding::Identity
    }
}

impl ContentMetaProvider for CompressedResponseStream<'_> {
    fn content_length(&self) -> Option<u64> {
        None
    }

    fn content_encoding(&self) -> ContentEncoding {
        ContentEncoding::Compressed(self.content_compression())
    }
}

/// Provide an implementation of ContentMetaProvider that propagates through Either (i.e. left_stream(),
/// right_stream()).
impl<A, B> ContentMetaProvider for futures::future::Either<A, B>
where
    A: ContentMetaProvider,
    B: ContentMetaProvider,
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

impl<S, F> ContentMetaProvider for futures::stream::InspectOk<S, F>
where
    S: ContentMetaProvider,
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

impl<S, E> ContentMetaProvider for EndOnErr<S, E>
where
    S: ContentMetaProvider,
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

impl<S, E> ContentMetaProvider for CaptureFirstErr<S, E>
where
    S: ContentMetaProvider,
{
    fn content_length(&self) -> Option<u64> {
        // Since errors may be filtered out without intending to fail the response, we cannot
        // provide a content length.
        None
    }

    fn content_encoding(&self) -> ContentEncoding {
        // CaptureFirstErr does not change the stream's encoding.
        self.get_ref().content_encoding()
    }
}
