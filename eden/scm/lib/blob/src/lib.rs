/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![allow(unexpected_cfgs)]

#[cfg(fbcode_build)]
use bytes::Buf;
pub use minibytes::Bytes;
use types::Blake3;
use types::Sha1;

#[derive(Clone, Debug)]
pub enum Blob {
    Bytes(minibytes::Bytes),
    #[cfg(fbcode_build)]
    IOBuf(iobuf::IOBufShared),
}

impl Blob {
    pub fn to_bytes(&self) -> minibytes::Bytes {
        match self {
            Self::Bytes(bytes) => bytes.clone(),
            #[cfg(fbcode_build)]
            Self::IOBuf(buf) => minibytes::Bytes::from(Vec::<u8>::from(buf.clone())),
        }
    }

    pub fn into_bytes(self) -> minibytes::Bytes {
        match self {
            Self::Bytes(bytes) => bytes,
            #[cfg(fbcode_build)]
            Self::IOBuf(buf) => minibytes::Bytes::from(Vec::<u8>::from(buf)),
        }
    }

    pub fn into_vec(self) -> Vec<u8> {
        match self {
            Self::Bytes(bytes) => bytes.into(),
            #[cfg(fbcode_build)]
            Self::IOBuf(buf) => Vec::<u8>::from(buf),
        }
    }

    #[cfg(fbcode_build)]
    pub fn into_iobuf(self) -> iobuf::IOBufShared {
        match self {
            // safety: `minibytes::Bytes`'s deref as `[u8]` is valid when `bytes` is alive.
            Self::Bytes(bytes) => iobuf_from_bytes(bytes),
            Self::IOBuf(buf) => buf,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Bytes(bytes) => bytes.len(),
            #[cfg(fbcode_build)]
            Self::IOBuf(buf) => buf.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::Bytes(bytes) => bytes.is_empty(),
            #[cfg(fbcode_build)]
            Self::IOBuf(buf) => buf.is_empty(),
        }
    }

    pub fn sha1(&self) -> types::Sha1 {
        use sha1::Digest;

        let mut hasher = sha1::Sha1::new();

        match self {
            Self::Bytes(bytes) => {
                hasher.update(bytes);
            }
            #[cfg(fbcode_build)]
            Self::IOBuf(buf) => {
                let mut cur = buf.clone().cursor();

                while cur.has_remaining() {
                    let b = cur.chunk();
                    hasher.update(b);
                    cur.advance(b.len());
                }
            }
        }

        let bytes: [u8; Sha1::len()] = hasher.finalize().into();
        Sha1::from(bytes)
    }

    pub fn blake3(&self) -> types::Blake3 {
        use blake3::Hasher;

        #[cfg(fbcode_build)]
        let key = blake3_constants::BLAKE3_HASH_KEY;
        #[cfg(not(fbcode_build))]
        let key = b"20220728-2357111317192329313741#";

        let mut hasher = Hasher::new_keyed(key);

        match self {
            Self::Bytes(bytes) => {
                hasher.update(bytes);
            }
            #[cfg(fbcode_build)]
            Self::IOBuf(buf) => {
                let mut cur = buf.clone().cursor();

                while cur.has_remaining() {
                    let b = cur.chunk();
                    hasher.update(b);
                    cur.advance(b.len());
                }
            }
        }

        let hashed_bytes: [u8; Blake3::len()] = hasher.finalize().into();
        Blake3::from(hashed_bytes)
    }
}

#[cfg(fbcode_build)]
fn iobuf_from_bytes(bytes: minibytes::Bytes) -> iobuf::IOBufShared {
    unsafe { iobuf::IOBufShared::from_owner(bytes) }
}

impl PartialEq for Blob {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bytes(l), Self::Bytes(r)) => l == r,
            #[cfg(fbcode_build)]
            (Self::IOBuf(l), Self::IOBuf(r)) => l == r,
            #[cfg(fbcode_build)]
            (Self::IOBuf(buf), Self::Bytes(bytes)) => {
                buf.len() == bytes.len() && buf == &iobuf_from_bytes(bytes.clone())
            }
            #[cfg(fbcode_build)]
            (Self::Bytes(bytes), Self::IOBuf(buf)) => {
                buf.len() == bytes.len() && buf == &iobuf_from_bytes(bytes.clone())
            }
        }
    }
}

impl From<minibytes::Bytes> for Blob {
    fn from(value: minibytes::Bytes) -> Self {
        Self::Bytes(value)
    }
}

/// Builds a Blob, using IOBuf to chain chunks when possible.
pub enum Builder {
    // capacity
    Empty(usize),
    Bytes(Vec<u8>),
    #[cfg(fbcode_build)]
    IOBuf(iobuf::IOBufShared),
}

impl Builder {
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    pub fn with_capacity(size: usize) -> Self {
        Self::Empty(size)
    }

    pub fn append(&mut self, chunk: Bytes) {
        match self {
            Builder::Empty(size) => {
                #[cfg(fbcode_build)]
                {
                    // Using IOBuf - ignore size for pre-allocation.
                    let _ = size;
                    *self = Self::IOBuf(iobuf_from_bytes(chunk));
                }

                #[cfg(not(fbcode_build))]
                {
                    // Not using IOBuf - pre-allocate with given size.
                    let mut data = Vec::with_capacity(*size);
                    data.extend_from_slice(chunk.as_ref());
                    *self = Self::Bytes(data);
                }
            }
            Builder::Bytes(data) => data.extend_from_slice(chunk.as_ref()),
            #[cfg(fbcode_build)]
            Builder::IOBuf(iobuf) => iobuf.append_to_end(iobuf_from_bytes(chunk)),
        }
    }

    pub fn into_blob(self) -> Blob {
        match self {
            Builder::Empty(_) => Blob::Bytes(Bytes::new()),
            Builder::Bytes(data) => Blob::Bytes(data.into()),
            #[cfg(fbcode_build)]
            Builder::IOBuf(iobuf) => Blob::IOBuf(iobuf),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[cfg(fbcode_build)]
    #[test]
    fn test_iobuf_sha1_and_blake3() {
        let blob1 = Blob::Bytes(minibytes::Bytes::from("hello world!"));

        let blob2 = {
            let mut iobuf = iobuf::IOBufShared::from("hello");
            iobuf.append_to_end(iobuf::IOBufShared::from(" world!"));
            Blob::IOBuf(iobuf)
        };

        assert_eq!(blob1.sha1(), blob2.sha1());
        assert_eq!(blob1.blake3(), blob2.blake3());
    }

    #[test]
    fn test_blob_eq() {
        let a = Blob::Bytes(minibytes::Bytes::from("hello world!"));
        let b = Blob::Bytes(minibytes::Bytes::from("hello world!"));
        assert_eq!(a, b);

        let a = Blob::Bytes(minibytes::Bytes::from("hello world!"));
        let b = Blob::Bytes(minibytes::Bytes::from("oops"));
        assert!(a != b);

        #[cfg(fbcode_build)]
        {
            let a = Blob::Bytes(minibytes::Bytes::from("hello world!"));
            let b = Blob::IOBuf(iobuf::IOBufShared::from("hello world!"));
            assert_eq!(a, b);
            assert_eq!(b, a);

            let a = Blob::Bytes(minibytes::Bytes::from("hello world!"));
            let b = Blob::IOBuf(iobuf::IOBufShared::from("oops"));
            assert!(a != b);
            assert!(b != a);
        }
    }

    #[test]
    fn test_iobuf_builder() {
        let b = Builder::new();
        assert_eq!(b.into_blob().len(), 0);

        let mut b = Builder::new();
        b.append(Bytes::from_static(b"hello"));
        assert_eq!(b.into_blob().into_bytes(), b"hello");

        let mut b = Builder::new();
        b.append(Bytes::from_static(b"hello"));
        b.append(Bytes::from_static(b" there"));
        assert_eq!(b.into_blob().into_bytes(), b"hello there");
    }
}
