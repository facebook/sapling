/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use bytes::Buf;
use types::Blake3;
use types::Sha1;

#[derive(Clone, Debug)]
pub enum ScmBlob {
    Bytes(minibytes::Bytes),
    #[cfg(fbcode_build)]
    IOBuf(iobuf::IOBufShared),
}

impl ScmBlob {
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

    #[cfg(fbcode_build)]
    pub fn into_iobuf(self) -> iobuf::IOBufShared {
        match self {
            Self::Bytes(bytes) => iobuf::IOBufShared::from(bytes),
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

#[cfg(test)]
mod test {
    use super::*;

    #[cfg(fbcode_build)]
    #[test]
    fn test_iobuf_sha1_and_blake3() {
        let blob1 = ScmBlob::Bytes(minibytes::Bytes::from("hello world!"));

        let blob2 = {
            let mut iobuf = iobuf::IOBufShared::from("hello");
            iobuf.append_to_end(iobuf::IOBufShared::from(" world!"));
            ScmBlob::IOBuf(iobuf)
        };

        assert_eq!(blob1.sha1(), blob2.sha1());
        assert_eq!(blob1.blake3(), blob2.blake3());
    }
}
