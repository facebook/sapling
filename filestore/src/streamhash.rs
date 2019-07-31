// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crypto::{digest::Digest, sha1::Sha1, sha2::Sha256};
use futures::{Future, Stream};

use mononoke_types::{hash, typed_hash, ContentId};

/// Hash a stream to compute its ContentId
#[allow(dead_code)]
pub fn content_id_hasher<E: Send + 'static>(
    s: impl Stream<Item = impl AsRef<[u8]>, Error = E> + Send + 'static,
) -> impl Future<Item = ContentId, Error = E> + Send + 'static {
    s.fold(typed_hash::ContentIdContext::new(), |mut ctxt, bytes| {
        ctxt.update(bytes);
        Ok(ctxt)
    })
    .map(|ctxt| ctxt.finish())
}

/// Hash a stream to compute its SHA-1
pub fn sha1_hasher<E: Send + 'static>(
    s: impl Stream<Item = impl AsRef<[u8]>, Error = E> + Send + 'static,
) -> impl Future<Item = hash::Sha1, Error = E> + Send + 'static {
    s.fold(Sha1::new(), |mut ctxt, bytes| {
        ctxt.input(bytes.as_ref());
        Ok(ctxt)
    })
    .map(|mut ctxt| {
        let mut hash = [0u8; 20];
        ctxt.result(&mut hash);
        hash::Sha1::from_byte_array(hash)
    })
}

/// Hash a stream to compute its SHA-1
pub fn git_sha1_hasher<E: Send + 'static>(
    size: u64,
    s: impl Stream<Item = impl AsRef<[u8]>, Error = E> + Send + 'static,
) -> impl Future<Item = hash::GitSha1, Error = E> + Send + 'static {
    let mut hasher = Sha1::new();
    let prototype = hash::GitSha1::from_byte_array([0; 20], "blob", size);

    hasher.input(&prototype.prefix());

    s.fold(hasher, |mut ctxt, bytes| {
        ctxt.input(bytes.as_ref());
        Ok(ctxt)
    })
    .map(move |mut ctxt| {
        let mut hash = [0u8; 20];
        ctxt.result(&mut hash);
        hash::GitSha1::from_byte_array(hash, "blob", size)
    })
}

/// Hash a stream to compute its SHA-256
pub fn sha256_hasher<E: Send + 'static>(
    s: impl Stream<Item = impl AsRef<[u8]>, Error = E> + Send + 'static,
) -> impl Future<Item = hash::Sha256, Error = E> + Send + 'static {
    s.fold(Sha256::new(), |mut ctxt, bytes| {
        ctxt.input(bytes.as_ref());
        Ok(ctxt)
    })
    .map(|mut ctxt| {
        let mut hash = [0u8; 32];
        ctxt.result(&mut hash);
        hash::Sha256::from_byte_array(hash)
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use bytes::Bytes;
    use futures::{future, stream};

    #[test]
    fn sha1_simple() {
        let data = Bytes::from(&b"hello, world"[..]); // b7e23ec29af22b0b4e41da31e868d57226121c84
        let s = stream::once(Ok::<_, ()>(data));

        let mut rt = tokio::runtime::Runtime::new().unwrap();

        let res = rt.block_on(future::lazy(|| sha1_hasher(s))).unwrap();

        assert_eq!(
            res,
            hash::Sha1::from_bytes([
                0xb7, 0xe2, 0x3e, 0xc2, 0x9a, 0xf2, 0x2b, 0x0b, 0x4e, 0x41, 0xda, 0x31, 0xe8, 0x68,
                0xd5, 0x72, 0x26, 0x12, 0x1c, 0x84
            ])
            .unwrap()
        );
    }

    #[test]
    fn sha1_chunks() {
        let data = vec![&b"hello"[..], &b", "[..], &b"world"[..]] // b7e23ec29af22b0b4e41da31e868d57226121c84
            .into_iter()
            .map(Bytes::from);
        let s = stream::iter_ok::<_, ()>(data);

        let mut rt = tokio::runtime::Runtime::new().unwrap();

        let res = rt.block_on(future::lazy(|| sha1_hasher(s))).unwrap();

        assert_eq!(
            res,
            hash::Sha1::from_bytes([
                0xb7, 0xe2, 0x3e, 0xc2, 0x9a, 0xf2, 0x2b, 0x0b, 0x4e, 0x41, 0xda, 0x31, 0xe8, 0x68,
                0xd5, 0x72, 0x26, 0x12, 0x1c, 0x84
            ])
            .unwrap()
        );
    }

    #[test]
    fn git_sha1_simple() {
        let data = Bytes::from(&b"hello, world"[..]); // 8c01d89ae06311834ee4b1fab2f0414d35f01102
        let s = stream::once(Ok::<_, ()>(data));

        let mut rt = tokio::runtime::Runtime::new().unwrap();

        let res = rt
            .block_on(future::lazy(|| git_sha1_hasher(12, s)))
            .unwrap();

        assert_eq!(
            res,
            hash::GitSha1::from_bytes(
                [
                    0x8c, 0x01, 0xd8, 0x9a, 0xe0, 0x63, 0x11, 0x83, 0x4e, 0xe4, 0xb1, 0xfa, 0xb2,
                    0xf0, 0x41, 0x4d, 0x35, 0xf0, 0x11, 0x02
                ],
                "blob",
                12
            )
            .unwrap()
        );
    }

    #[test]
    fn git_sha1_chunks() {
        let data = vec![&b"hello"[..], &b", "[..], &b"world"[..]] // 8c01d89ae06311834ee4b1fab2f0414d35f01102
            .into_iter()
            .map(Bytes::from);
        let s = stream::iter_ok::<_, ()>(data);

        let mut rt = tokio::runtime::Runtime::new().unwrap();

        let res = rt
            .block_on(future::lazy(|| git_sha1_hasher(12, s)))
            .unwrap();

        assert_eq!(
            res,
            hash::GitSha1::from_bytes(
                [
                    0x8c, 0x01, 0xd8, 0x9a, 0xe0, 0x63, 0x11, 0x83, 0x4e, 0xe4, 0xb1, 0xfa, 0xb2,
                    0xf0, 0x41, 0x4d, 0x35, 0xf0, 0x11, 0x02
                ],
                "blob",
                12
            )
            .unwrap()
        );
    }

    #[test]
    fn sha256_simple() {
        let data = Bytes::from(&b"hello, world"[..]); // 09ca7e4eaa6e8ae9c7d261167129184883644d07dfba7cbfbc4c8a2e08360d5b
        let s = stream::once(Ok::<_, ()>(data));

        let mut rt = tokio::runtime::Runtime::new().unwrap();

        let res = rt.block_on(future::lazy(|| sha256_hasher(s))).unwrap();

        assert_eq!(
            res,
            hash::Sha256::from_bytes([
                0x09, 0xca, 0x7e, 0x4e, 0xaa, 0x6e, 0x8a, 0xe9, 0xc7, 0xd2, 0x61, 0x16, 0x71, 0x29,
                0x18, 0x48, 0x83, 0x64, 0x4d, 0x07, 0xdf, 0xba, 0x7c, 0xbf, 0xbc, 0x4c, 0x8a, 0x2e,
                0x08, 0x36, 0x0d, 0x5b,
            ],)
            .unwrap()
        );
    }

    #[test]
    fn sha256_chunks() {
        let data = vec![&b"hello"[..], &b", "[..], &b"world"[..]] // 09ca7e4eaa6e8ae9c7d261167129184883644d07dfba7cbfbc4c8a2e08360d5b
            .into_iter()
            .map(Bytes::from);
        let s = stream::iter_ok::<_, ()>(data);

        let mut rt = tokio::runtime::Runtime::new().unwrap();

        let res = rt.block_on(future::lazy(|| sha256_hasher(s))).unwrap();

        assert_eq!(
            res,
            hash::Sha256::from_bytes([
                0x09, 0xca, 0x7e, 0x4e, 0xaa, 0x6e, 0x8a, 0xe9, 0xc7, 0xd2, 0x61, 0x16, 0x71, 0x29,
                0x18, 0x48, 0x83, 0x64, 0x4d, 0x07, 0xdf, 0xba, 0x7c, 0xbf, 0xbc, 0x4c, 0x8a, 0x2e,
                0x08, 0x36, 0x0d, 0x5b,
            ],)
            .unwrap()
        );
    }

    #[test]
    fn content_id_simple() {
        let data = Bytes::from(&b"hello, world"[..]); // 19d95f338fa0f547f773c12353eb6dacb1a7ce9b0402515484e8276055774b35
        let s = stream::once(Ok::<_, ()>(data));

        let mut rt = tokio::runtime::Runtime::new().unwrap();

        let res = rt.block_on(future::lazy(|| content_id_hasher(s))).unwrap();

        assert_eq!(
            res,
            typed_hash::ContentId::from_bytes([
                0x19, 0xd9, 0x5f, 0x33, 0x8f, 0xa0, 0xf5, 0x47, 0xf7, 0x73, 0xc1, 0x23, 0x53, 0xeb,
                0x6d, 0xac, 0xb1, 0xa7, 0xce, 0x9b, 0x04, 0x02, 0x51, 0x54, 0x84, 0xe8, 0x27, 0x60,
                0x55, 0x77, 0x4b, 0x35,
            ],)
            .unwrap()
        );
    }

    #[test]
    fn content_id_chunks() {
        let data = vec![&b"hello"[..], &b", "[..], &b"world"[..]] // 19d95f338fa0f547f773c12353eb6dacb1a7ce9b0402515484e8276055774b35
            .into_iter()
            .map(Bytes::from);
        let s = stream::iter_ok::<_, ()>(data);

        let mut rt = tokio::runtime::Runtime::new().unwrap();

        let res = rt.block_on(future::lazy(|| content_id_hasher(s))).unwrap();

        assert_eq!(
            res,
            typed_hash::ContentId::from_bytes([
                0x19, 0xd9, 0x5f, 0x33, 0x8f, 0xa0, 0xf5, 0x47, 0xf7, 0x73, 0xc1, 0x23, 0x53, 0xeb,
                0x6d, 0xac, 0xb1, 0xa7, 0xce, 0x9b, 0x04, 0x02, 0x51, 0x54, 0x84, 0xe8, 0x27, 0x60,
                0x55, 0x77, 0x4b, 0x35,
            ],)
            .unwrap()
        );
    }
}
