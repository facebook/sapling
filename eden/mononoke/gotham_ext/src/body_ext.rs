/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bytes::Bytes;
use bytes::BytesMut;
use futures::future;
use futures::future::MapOk;
use futures::future::Ready;
use futures::future::TryFutureExt;
use futures::stream::Stream;
use futures::stream::TryFold;
use futures::stream::TryStreamExt;
use http::header::HeaderMap;
use http::header::CONTENT_LENGTH;
use std::str;

type BodyFuture<S, E> = MapOk<
    TryFold<
        S,
        Ready<Result<BytesMut, E>>,
        BytesMut,
        fn(BytesMut, Bytes) -> Ready<Result<BytesMut, E>>,
    >,
    fn(BytesMut) -> Bytes,
>;

fn extend_buff<E>(mut buff: BytesMut, chunk: Bytes) -> Ready<Result<BytesMut, E>> {
    buff.extend_from_slice(chunk.as_ref());
    future::ready(Ok(buff))
}

pub trait BodyExt<E>: Stream<Item = Result<Bytes, E>> + Sized {
    fn try_concat_body_opt(
        self,
        headers: Option<&HeaderMap>,
    ) -> Result<BodyFuture<Self, E>, Error> {
        match headers {
            Some(headers) => self.try_concat_body(headers),
            None => {
                let headers = HeaderMap::default();
                self.try_concat_body(&headers)
            }
        }
    }

    fn try_concat_body(self, headers: &HeaderMap) -> Result<BodyFuture<Self, E>, Error> {
        let buff = if let Some(val) = headers.get(CONTENT_LENGTH) {
            let size = str::from_utf8(val.as_bytes())?.parse()?;
            BytesMut::with_capacity(size)
        } else {
            BytesMut::new()
        };

        Ok(self
            .try_fold(
                buff,
                extend_buff as fn(BytesMut, Bytes) -> Ready<Result<BytesMut, E>>,
            )
            .map_ok(BytesMut::freeze as fn(BytesMut) -> Bytes))
    }
}

impl<S, E> BodyExt<E> for S where S: Stream<Item = Result<Bytes, E>> {}

#[cfg(test)]
mod test {
    use super::*;
    use futures::stream;

    fn make_stream() -> impl Stream<Item = Result<Bytes, Error>> {
        stream::iter(vec![
            Result::<_, Error>::Ok(Bytes::from("1")),
            Result::<_, Error>::Ok(Bytes::from("2")),
        ])
    }

    #[tokio::test]
    async fn test_no_content_length() -> Result<(), Error> {
        let h = HeaderMap::new();
        let v = make_stream().try_concat_body(&h)?.await?;
        assert_eq!(v, Bytes::from("12"));
        Ok(())
    }

    #[tokio::test]
    async fn test_correct_content_length() -> Result<(), Error> {
        let mut h = HeaderMap::new();
        h.insert(CONTENT_LENGTH, "2".parse()?);

        let v = make_stream().try_concat_body(&h)?.await?;
        assert_eq!(v, Bytes::from("12"));

        Ok(())
    }

    #[tokio::test]
    async fn test_incorrect_content_length() -> Result<(), Error> {
        let mut h = HeaderMap::new();
        h.insert(CONTENT_LENGTH, "123".parse()?);

        let v = make_stream().try_concat_body(&h)?.await?;
        assert_eq!(v, Bytes::from("12"));

        Ok(())
    }

    #[tokio::test]
    async fn test_invalid_content_length() -> Result<(), Error> {
        let mut h = HeaderMap::new();
        h.insert(CONTENT_LENGTH, "foobar".parse()?);

        let v = make_stream().try_concat_body(&h);
        assert!(v.is_err());

        Ok(())
    }
}
